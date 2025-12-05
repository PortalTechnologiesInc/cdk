//! Fake Wallet Error

use thiserror::Error;

/// Fake Wallet Error
#[derive(Debug, Error)]
pub enum Error {
    /// Unsupported Bolt12
    #[error("Unsupported Bolt12")]
    UnsupportedBolt12,

    /// Payment not found
    #[error("Payment not found")]
    PaymentNotFound,
}

impl From<Error> for cdk_common::payment::Error {
    fn from(e: Error) -> Self {
        Self::Custom(e.to_string())
    }
}
