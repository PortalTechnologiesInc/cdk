//! PortalWallet environment variables

use std::env;

use cdk::nuts::CurrencyUnit;

use crate::config::PortalWallet;

// Portal Wallet environment variables
pub const ENV_PORTAL_WALLET_SUPPORTED_UNITS: &str = "CDK_MINTD_PORTAL_WALLET_SUPPORTED_UNITS";

#[derive(Debug)]
struct SupportedUnit {
    unit: CurrencyUnit,
    max_order: u8,
}

impl core::str::FromStr for SupportedUnit {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (unit, max_order) = s.split_once(':').ok_or("Invalid format")?;
        Ok(Self {
            unit: unit.parse().map_err(|_| "Invalid unit")?,
            max_order: max_order.parse().map_err(|_| "Invalid max order")?,
        })
    }
}

impl PortalWallet {
    pub fn from_env(mut self) -> Self {
        // Supported Units - expects comma-separated list
        if let Ok(units_str) = env::var(ENV_PORTAL_WALLET_SUPPORTED_UNITS) {
            if let Ok(units) = units_str
                .split(',')
                .map(|s| s.trim().parse())
                .collect::<Result<Vec<SupportedUnit>, _>>()
            {
                self.supported_units = units.into_iter().map(|u| (u.unit, u.max_order)).collect();
            }
        }

        self
    }
}
