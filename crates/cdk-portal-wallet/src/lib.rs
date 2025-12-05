//! CDK portal wallet

pub mod error;

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use futures::Stream;
use futures::StreamExt;
use rand::Rng;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::sync::Mutex;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use cdk_common::amount::Amount;
use cdk_common::nuts::{CurrencyUnit, MeltQuoteState};
use cdk_common::payment::{
    self, Bolt11Settings, CreateIncomingPaymentResponse, Event, IncomingPaymentOptions,
    MakePaymentResponse, MintPayment, OutgoingPaymentOptions, PaymentIdentifier,
    PaymentQuoteResponse, WaitPaymentResponse,
};
use serde_json::Value;

pub struct SimpleWallet {
    sender: Sender<[u8; 32]>,
    receiver: Arc<Mutex<Option<Receiver<[u8; 32]>>>>,
    invoices: Arc<Mutex<HashMap<[u8; 32], (String, bool, Amount, CurrencyUnit)>>>, // payment_hash -> (invoice_id, paid, amount, unit)
    wait_invoice_cancel_token: CancellationToken,
    wait_invoice_is_active: Arc<AtomicBool>,
    settings: Bolt11Settings,
}

impl SimpleWallet {
    pub fn new(currency_unit: CurrencyUnit) -> Self {
        let (sender, receiver) = mpsc::channel(32);
        Self {
            sender,
            receiver: Arc::new(Mutex::new(Some(receiver))),
            invoices: Arc::new(Mutex::new(HashMap::new())),
            wait_invoice_cancel_token: CancellationToken::new(),
            wait_invoice_is_active: Arc::new(AtomicBool::new(false)),
            settings: Bolt11Settings {
                mpp: false,
                unit: currency_unit,
                invoice_description: true,
                amountless: false,
                bolt12: false,
            },
        }
    }
}

#[async_trait]
impl MintPayment for SimpleWallet {
    type Err = payment::Error;

    async fn get_settings(&self) -> Result<Value, Self::Err> {
        Ok(serde_json::to_value(&self.settings)?)
    }

    fn is_wait_invoice_active(&self) -> bool {
        self.wait_invoice_is_active.load(Ordering::SeqCst)
    }

    fn cancel_wait_invoice(&self) {
        self.wait_invoice_cancel_token.cancel()
    }

    async fn wait_payment_event(
        &self,
    ) -> Result<Pin<Box<dyn Stream<Item = Event> + Send>>, Self::Err> {
        // Take the current receiver out. Only one consumer at a time!
        let mut slot = self.receiver.lock().await;
        let receiver = slot.take();
        let invoices = self.invoices.clone();

        if let Some(receiver) = receiver {
            let stream = ReceiverStream::new(receiver).filter_map(move |payment_hash| {
                let invoices = invoices.clone();

                async move {
                    let guard = invoices.lock().await;
                    if let Some((invoice_id, paid, amount, unit)) = guard.get(&payment_hash) {
                        if *paid {
                            Some(Event::PaymentReceived(WaitPaymentResponse {
                                payment_identifier: PaymentIdentifier::PaymentHash(payment_hash),
                                payment_amount: *amount,
                                unit: unit.clone(),
                                payment_id: invoice_id.clone(),
                            }))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
            });
            Ok(Box::pin(stream))
        } else {
            // Already active
            Ok(Box::pin(futures::stream::empty()))
        }
    }

    async fn get_payment_quote(
        &self,
        _unit: &CurrencyUnit,
        options: OutgoingPaymentOptions,
    ) -> Result<PaymentQuoteResponse, Self::Err> {
        let amount_msat = match options {
            OutgoingPaymentOptions::Bolt11(ref bolt11_options) => {
                match bolt11_options.melt_options {
                    Some(ref amt) => amt.amount_msat(),
                    None => bolt11_options
                        .bolt11
                        .amount_milli_satoshis()
                        .ok_or(payment::Error::Custom("Unknown invoice amount".to_string()))?
                        .into(),
                }
            }
            OutgoingPaymentOptions::Bolt12(_) => return Err(error::Error::UnsupportedBolt12.into()),
        };
        let random_hash: [u8; 32] = rand::rng().random();
        Ok(PaymentQuoteResponse {
            request_lookup_id: Some(PaymentIdentifier::PaymentHash(random_hash)),
            amount: Amount::from(amount_msat),
            fee: Amount::ZERO,
            state: MeltQuoteState::Unpaid,
            unit: CurrencyUnit::Msat,
        })
    }

    async fn make_payment(
        &self,
        _unit: &CurrencyUnit,
        options: OutgoingPaymentOptions,
    ) -> Result<MakePaymentResponse, Self::Err> {
        let invoice_id = match options {
            OutgoingPaymentOptions::Bolt11(ref bolt11_options) => bolt11_options.bolt11.to_string(),
            OutgoingPaymentOptions::Bolt12(_) => return Err(error::Error::UnsupportedBolt12.into()),
        };
        let mut invoices = self.invoices.lock().await;
        if let Some((payment_hash, paid, amount, unit)) =
            invoices
                .iter_mut()
                .find_map(|(hash, (id, paid, amount, unit))| {
                    (id == &invoice_id).then_some((hash, paid, amount, unit))
                })
        {
            *paid = true;
            // Optionally, you could do self.sender.send(*payment_hash).await, but we already auto-send.
            Ok(MakePaymentResponse {
                payment_lookup_id: PaymentIdentifier::PaymentHash(*payment_hash),
                payment_proof: Some(invoice_id.clone()),
                status: MeltQuoteState::Paid,
                total_spent: *amount,
                unit: unit.clone(),
            })
        } else {
            Err(error::Error::PaymentNotFound.into())
        }
    }

    async fn create_incoming_payment_request(
        &self,
        unit: &CurrencyUnit,
        options: IncomingPaymentOptions,
    ) -> Result<CreateIncomingPaymentResponse, Self::Err> {
        let (amount, _expiry) = match options {
            IncomingPaymentOptions::Bolt11(ref bolt11_options) => {
                (Some(bolt11_options.amount), bolt11_options.unix_expiry)
            }
            IncomingPaymentOptions::Bolt12(_) => return Err(error::Error::UnsupportedBolt12.into()),
        };
        let invoice_id = Uuid::new_v4().to_string();
        let random_hash: [u8; 32] = rand::rng().random();
        let payment_amount = amount.unwrap_or(Amount::ZERO);
        // Insert as paid at creation (auto-pay)
        self.invoices.lock().await.insert(
            random_hash,
            (invoice_id.clone(), true, payment_amount, unit.clone()),
        );

        // Notify immediately
        let _ = self.sender.send(random_hash).await;

        Ok(CreateIncomingPaymentResponse {
            request_lookup_id: PaymentIdentifier::PaymentHash(random_hash),
            request: invoice_id.clone(),
            expiry: None,
        })
    }

    async fn check_incoming_payment_status(
        &self,
        payment_identifier: &PaymentIdentifier,
    ) -> Result<Vec<WaitPaymentResponse>, Self::Err> {
        let payment_hash = match payment_identifier {
            PaymentIdentifier::PaymentHash(hash) => hash,
            _ => return Ok(vec![]),
        };
        let guard = self.invoices.lock().await;
        if let Some((invoice_id, paid, amount, unit)) = guard.get(payment_hash) {
            if *paid {
                return Ok(vec![WaitPaymentResponse {
                    payment_identifier: payment_identifier.clone(),
                    payment_amount: *amount,
                    unit: unit.clone(),
                    payment_id: invoice_id.clone(),
                }]);
            }
        }
        Ok(vec![])
    }

    async fn check_outgoing_payment(
        &self,
        payment_identifier: &PaymentIdentifier,
    ) -> Result<MakePaymentResponse, Self::Err> {
        let payment_hash = match payment_identifier {
            PaymentIdentifier::PaymentHash(hash) => hash,
            _ => return Err(error::Error::PaymentNotFound.into()),
        };
        let guard = self.invoices.lock().await;
        if let Some((invoice_id, paid, amount, unit)) = guard.get(payment_hash) {
            let status = if *paid {
                MeltQuoteState::Paid
            } else {
                MeltQuoteState::Unpaid
            };
            return Ok(MakePaymentResponse {
                payment_lookup_id: payment_identifier.clone(),
                payment_proof: Some(invoice_id.clone()),
                status,
                total_spent: *amount,
                unit: unit.clone(),
            });
        }
        Err(error::Error::PaymentNotFound.into())
    }
}
