//! CDK Fake LN Backend
//!
//! Used for testing where quotes are auto filled

#![doc = include_str!("../README.md")]
#![warn(missing_docs)]
#![warn(rustdoc::bare_urls)]

use std::collections::{HashMap, HashSet};
use std::pin::Pin;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use bitcoin::hashes::{sha256, Hash};
use bitcoin::secp256k1::rand::{thread_rng, Rng};
use bitcoin::secp256k1::{Secp256k1, SecretKey};
use cdk::amount::Amount;
use cdk::cdk_payment::{
    self, Bolt11Settings, CreateIncomingPaymentResponse, MakePaymentResponse, MintPayment,
    PaymentQuoteResponse,
};
use cdk::mint;
use cdk::nuts::{CurrencyUnit, MeltOptions, MeltQuoteState, MintQuoteState};
use error::Error;
use futures::stream::StreamExt;
use futures::Stream;
use lightning_invoice::{Bolt11Invoice, Currency, InvoiceBuilder, PaymentSecret};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;
use tracing::instrument;

pub mod error;

/// Fake Wallet
#[derive(Clone)]
pub struct PortalWallet {
    sender: tokio::sync::mpsc::Sender<String>,
    receiver: Arc<Mutex<Option<tokio::sync::mpsc::Receiver<String>>>>,
    payment_states: Arc<Mutex<HashMap<String, MeltQuoteState>>>,
    failed_payment_check: Arc<Mutex<HashSet<String>>>,
    wait_invoice_cancel_token: CancellationToken,
    wait_invoice_is_active: Arc<AtomicBool>,
    supported_units: Arc<Mutex<HashMap<CurrencyUnit, u8>>>,
}

impl PortalWallet {
    /// Create new [`PortalWallet`]
    pub fn new(
        payment_states: HashMap<String, MeltQuoteState>,
        fail_payment_check: HashSet<String>,
    ) -> Self {
        let (sender, receiver) = tokio::sync::mpsc::channel(8);

        Self {
            sender,
            receiver: Arc::new(Mutex::new(Some(receiver))),
            payment_states: Arc::new(Mutex::new(payment_states)),
            failed_payment_check: Arc::new(Mutex::new(fail_payment_check)),
            wait_invoice_cancel_token: CancellationToken::new(),
            wait_invoice_is_active: Arc::new(AtomicBool::new(false)),
            supported_units: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Add a supported unit to the portal wallet
    pub async fn add_supported_unit(&self, unit: CurrencyUnit, max_order: u8) {
        let mut supported_units = self.supported_units.lock().await;
        supported_units.insert(unit, max_order);
    }
}

/// Struct for signaling what methods should respond via invoice description
#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct FakeInvoiceDescription {
    /// State to be returned from pay invoice state
    pub pay_invoice_state: MeltQuoteState,
    /// State to be returned by check payment state
    pub check_payment_state: MeltQuoteState,
    /// Should pay invoice error
    pub pay_err: bool,
    /// Should check failure
    pub check_err: bool,
}

impl Default for FakeInvoiceDescription {
    fn default() -> Self {
        Self {
            pay_invoice_state: MeltQuoteState::Paid,
            check_payment_state: MeltQuoteState::Paid,
            pay_err: false,
            check_err: false,
        }
    }
}

#[async_trait]
impl MintPayment for PortalWallet {
    type Err = cdk_payment::Error;

    #[instrument(skip_all)]
    async fn get_settings(&self) -> Result<Value, Self::Err> {
        Ok(serde_json::to_value(Bolt11Settings {
            mpp: true,
            unit: CurrencyUnit::Msat,
            invoice_description: true,
            amountless: false,
        })?)
    }

    #[instrument(skip_all)]
    fn is_wait_invoice_active(&self) -> bool {
        self.wait_invoice_is_active.load(Ordering::SeqCst)
    }

    #[instrument(skip_all)]
    fn cancel_wait_invoice(&self) {
        self.wait_invoice_cancel_token.cancel()
    }

    #[instrument(skip_all)]
    async fn wait_any_incoming_payment(
        &self,
    ) -> Result<Pin<Box<dyn Stream<Item = String> + Send>>, Self::Err> {
        tracing::info!("Starting stream for fake invoices");
        let receiver = self.receiver.lock().await.take().ok_or(Error::NoReceiver)?;
        let receiver_stream = ReceiverStream::new(receiver);
        Ok(Box::pin(receiver_stream.map(|label| label)))
    }

    #[instrument(skip_all)]
    async fn get_payment_quote(
        &self,
        request: &str,
        unit: &CurrencyUnit,
        _options: Option<MeltOptions>,
    ) -> Result<PaymentQuoteResponse, Self::Err> {
        let bolt11 = Bolt11Invoice::from_str(request)?;

        Ok(PaymentQuoteResponse {
            request_lookup_id: bolt11.payment_hash().to_string(),
            amount: Amount::ZERO,
            fee: Amount::ZERO,
            unit: unit.clone(),
            state: MeltQuoteState::Unpaid,
        })
    }

    #[instrument(skip_all)]
    async fn make_payment(
        &self,
        melt_quote: mint::MeltQuote,
        _partial_msats: Option<Amount>,
        _max_fee_msats: Option<Amount>,
    ) -> Result<MakePaymentResponse, Self::Err> {
        let bolt11 = Bolt11Invoice::from_str(&melt_quote.request)?;

        let payment_hash = bolt11.payment_hash().to_string();

        Ok(MakePaymentResponse {
            payment_proof: Some("".to_string()),
            payment_lookup_id: payment_hash,
            status: MeltQuoteState::Paid,
            total_spent: melt_quote.amount,
            unit: melt_quote.unit,
        })
    }

    #[instrument(skip_all)]
    async fn create_incoming_payment_request(
        &self,
        amount: Amount,
        _unit: &CurrencyUnit,
        description: String,
        _unix_expiry: Option<u64>,
    ) -> Result<CreateIncomingPaymentResponse, Self::Err> {
        // Since this is fake we just use the amount no matter the unit to create an invoice
        let amount_msat = amount;

        let invoice = create_fake_invoice(amount_msat.into(), description);

        let payment_hash = invoice.payment_hash();

        if self.sender.send(payment_hash.to_string()).await.is_err() {
            tracing::error!("Failed to send label: {}", payment_hash);
        }

        let expiry = invoice.expires_at().map(|t| t.as_secs());

        Ok(CreateIncomingPaymentResponse {
            request_lookup_id: payment_hash.to_string(),
            request: invoice.to_string(),
            expiry,
        })
    }

    #[instrument(skip_all)]
    async fn check_incoming_payment_status(
        &self,
        _request_lookup_id: &str,
    ) -> Result<MintQuoteState, Self::Err> {
        Ok(MintQuoteState::Paid)
    }

    #[instrument(skip_all)]
    async fn check_outgoing_payment(
        &self,
        request_lookup_id: &str,
    ) -> Result<MakePaymentResponse, Self::Err> {
        // For fake wallet if the state is not explicitly set default to paid
        let states = self.payment_states.lock().await;
        let status = states.get(request_lookup_id).cloned();

        let status = status.unwrap_or(MeltQuoteState::Paid);

        let fail_payments = self.failed_payment_check.lock().await;

        if fail_payments.contains(request_lookup_id) {
            return Err(cdk_payment::Error::InvoicePaymentPending);
        }

        Ok(MakePaymentResponse {
            payment_proof: Some("".to_string()),
            payment_lookup_id: request_lookup_id.to_string(),
            status,
            total_spent: Amount::ZERO,
            unit: CurrencyUnit::Msat,
        })
    }

    async fn max_order(&self, unit: &CurrencyUnit) -> u8 {
        let supported_units = self.supported_units.lock().await;
        supported_units.get(unit).cloned().unwrap_or(32)
    }
}

/// Create fake invoice
#[instrument]
pub fn create_fake_invoice(amount_msat: u64, description: String) -> Bolt11Invoice {
    let private_key = SecretKey::from_slice(
        &[
            0xe1, 0x26, 0xf6, 0x8f, 0x7e, 0xaf, 0xcc, 0x8b, 0x74, 0xf5, 0x4d, 0x26, 0x9f, 0xe2,
            0x06, 0xbe, 0x71, 0x50, 0x00, 0xf9, 0x4d, 0xac, 0x06, 0x7d, 0x1c, 0x04, 0xa8, 0xca,
            0x3b, 0x2d, 0xb7, 0x34,
        ][..],
    )
    .unwrap();

    let mut rng = thread_rng();
    let mut random_bytes = [0u8; 32];
    rng.fill(&mut random_bytes);

    let payment_hash = sha256::Hash::from_slice(&random_bytes).unwrap();
    let payment_secret = PaymentSecret([42u8; 32]);

    InvoiceBuilder::new(Currency::Bitcoin)
        .description(description)
        .payment_hash(payment_hash)
        .payment_secret(payment_secret)
        .amount_milli_satoshis(amount_msat)
        .current_timestamp()
        .min_final_cltv_expiry_delta(144)
        .build_signed(|hash| Secp256k1::new().sign_ecdsa_recoverable(hash, &private_key))
        .unwrap()
}
