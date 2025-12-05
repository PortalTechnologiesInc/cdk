//! PortalWallet environment variables

use std::env;

use cdk::nuts::CurrencyUnit;

use crate::config::PortalWallet;

use cdk_common::common::UnitMetadata;

// Fake Wallet environment variables
pub const ENV_PORTAL_WALLET_SUPPORTED_UNITS: &str = "CDK_MINTD_PORTAL_WALLET_SUPPORTED_UNITS";
pub const ENV_PORTAL_WALLET_UNIT_INFO: &str = "CDK_MINTD_PORTAL_WALLET_UNIT_INFO";

#[derive(Debug)]
struct UnitInfo {
    unit: CurrencyUnit,
    description: String,
    url: String,
    is_non_fungible: bool,
}

impl core::str::FromStr for UnitInfo {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (unit, remaining) = s.split_once('=').ok_or("Invalid format")?;
        let mut parts = remaining.split('\n');

        let description = parts.next().ok_or("Invalid format")?;
        let url = parts.next().ok_or("Invalid format")?;

        let is_non_fungible = parts
            .next()
            .ok_or("Invalid format")?
            .parse()
            .map_err(|_| "Invalid is_non_fungible")?;

        Ok(Self {
            unit: unit.parse().map_err(|_| "Invalid unit")?,
            description: description.to_string(),
            url: url.to_string(),
            is_non_fungible,
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
                .collect::<Result<Vec<CurrencyUnit>, _>>()
            {
                self.supported_units = units;
            }
        }

        // Unit Info - expects comma-separated list
        if let Ok(unit_info_str) = env::var(ENV_PORTAL_WALLET_UNIT_INFO) {
            if let Ok(unit_info) = unit_info_str
                .split(',')
                .map(|s| s.parse())
                .collect::<Result<Vec<UnitInfo>, _>>()
            {
                self.unit_info = unit_info
                    .into_iter()
                    .map(|u| {
                        (
                            u.unit,
                            UnitMetadata {
                                description: u.description,
                                url: u.url,
                                is_non_fungible: u.is_non_fungible,
                            },
                        )
                    })
                    .collect();
            }
        }

        self
    }
}
