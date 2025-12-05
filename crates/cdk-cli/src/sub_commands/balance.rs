use std::collections::BTreeMap;

use anyhow::Result;
use cdk::mint_url::MintUrl;
use cdk::nuts::CurrencyUnit;
use cdk::wallet::MultiMintWallet;
use cdk::Amount;
use clap::Args;

use std::str::FromStr;

#[derive(Args)]
pub struct BalanceSubCommand {
    /// Currency unit e.g. sat, msat, usd, eur
    #[arg(short, long)]
    pub unit: String,
}

pub async fn balance(
    multi_mint_wallet: &MultiMintWallet,
    sub_command_args: &BalanceSubCommand,
) -> Result<()> {
    println!("Balance for unit: {}", sub_command_args.unit);

    let unit = CurrencyUnit::from_str(&sub_command_args.unit)?;
    // Show individual mint balances
    let mint_balances = mint_balances(multi_mint_wallet).await?;

    // Show total balance using the new unified interface
    let total = multi_mint_wallet.total_balance().await?;
    if !mint_balances.is_empty() {
        println!();
        println!("Total balance across all wallets: {} {}", total, unit);
    }

    Ok(())
}

pub async fn mint_balances(
    multi_mint_wallet: &MultiMintWallet,
) -> Result<Vec<(MintUrl, (Amount, CurrencyUnit))>> {
    let wallets: BTreeMap<MintUrl, (Amount, CurrencyUnit)> =
        multi_mint_wallet.get_balances().await?;

    let mut wallets_vec = Vec::with_capacity(wallets.len());

    for (i, (mint_url, (amount, unit))) in wallets
        .iter()
        .filter(|(_, (a, _))| a > &&Amount::ZERO)
        .enumerate()
    {
        let mint_url = mint_url.clone();
        println!("{i}: {mint_url} {amount} {unit}");
        wallets_vec.push((mint_url, (amount.clone(), unit.clone())));
    }
    Ok(wallets_vec)
}
