//! Environment variables module
//!
//! This module contains all environment variable definitions and parsing logic
//! organized by component.

mod common;
mod info;
mod ln;
mod mint_info;

#[cfg(feature = "auth")]
mod auth;
#[cfg(feature = "cln")]
mod cln;
#[cfg(feature = "fakewallet")]
mod fake_wallet;
#[cfg(feature = "grpc-processor")]
mod grpc_processor;
#[cfg(feature = "lnbits")]
mod lnbits;
#[cfg(feature = "lnd")]
mod lnd;
#[cfg(feature = "management-rpc")]
mod management_rpc;
#[cfg(feature = "portalwallet")]
mod portal_wallet;

use std::env;
use std::str::FromStr;

use anyhow::{anyhow, bail, Result};
#[cfg(feature = "auth")]
pub use auth::*;
#[cfg(feature = "cln")]
pub use cln::*;
pub use common::*;
#[cfg(feature = "fakewallet")]
pub use fake_wallet::*;
#[cfg(feature = "grpc-processor")]
pub use grpc_processor::*;
pub use ln::*;
#[cfg(feature = "lnbits")]
pub use lnbits::*;
#[cfg(feature = "lnd")]
pub use lnd::*;
#[cfg(feature = "management-rpc")]
pub use management_rpc::*;
pub use mint_info::*;
#[cfg(feature = "portalwallet")]
pub use portal_wallet::*;

use crate::config::{Database, DatabaseEngine, LnBackend, Settings};

impl Settings {
    pub fn from_env(&mut self) -> Result<Self> {
        if let Ok(database) = env::var(DATABASE_ENV_VAR) {
            let engine = DatabaseEngine::from_str(&database).map_err(|err| anyhow!(err))?;
            self.database = Database { engine };
        }

        self.info = self.info.clone().from_env();
        self.mint_info = self.mint_info.clone().from_env();
        self.ln = self.ln.clone().from_env();

        #[cfg(feature = "auth")]
        {
            // Check env vars for auth config even if None
            let auth = self.auth.clone().unwrap_or_default().from_env();

            // Only set auth if env vars are present and have non-default values
            if auth.openid_discovery.is_some()
                || auth.openid_client_id.is_some()
                || auth.static_token.is_some()
                || auth.mint_max_bat != 0
                || auth.enabled_mint
                || auth.enabled_melt
                || auth.enabled_swap
            {
                self.auth = Some(auth);
            } else {
                self.auth = None;
            }
        }

        #[cfg(feature = "management-rpc")]
        {
            self.mint_management_rpc = Some(
                self.mint_management_rpc
                    .clone()
                    .unwrap_or_default()
                    .from_env(),
            );
        }

        match self.ln.ln_backend {
            #[cfg(feature = "cln")]
            LnBackend::Cln => {
                self.cln = Some(self.cln.clone().unwrap_or_default().from_env());
            }
            #[cfg(feature = "lnbits")]
            LnBackend::LNbits => {
                self.lnbits = Some(self.lnbits.clone().unwrap_or_default().from_env());
            }
            #[cfg(feature = "fakewallet")]
            LnBackend::FakeWallet => {
                self.fake_wallet = Some(self.fake_wallet.clone().unwrap_or_default().from_env());
            }
            #[cfg(feature = "portalwallet")]
            LnBackend::PortalWallet => {
                self.portal_wallet = Some(
                    self.portal_wallet
                        .clone()
                        .expect("Portal wallet configuration must be set")
                        .from_env(),
                );
            }
            #[cfg(feature = "lnd")]
            LnBackend::Lnd => {
                self.lnd = Some(self.lnd.clone().unwrap_or_default().from_env());
            }
            #[cfg(feature = "grpc-processor")]
            LnBackend::GrpcProcessor => {
                self.grpc_processor =
                    Some(self.grpc_processor.clone().unwrap_or_default().from_env());
            }
            LnBackend::None => bail!("Ln backend must be set"),
            #[allow(unreachable_patterns)]
            _ => bail!("Selected Ln backend is not enabled in this build"),
        }

        Ok(self.clone())
    }
}
