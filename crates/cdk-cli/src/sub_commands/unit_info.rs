use anyhow::Result;
use cdk::mint_url::MintUrl;
use cdk::nuts::CurrencyUnit;
use cdk::wallet::multi_mint_wallet::MultiMintWallet;
use cdk::wallet::types::WalletKey;
use clap::Args;
use std::str::FromStr;

#[derive(Args)]
pub struct UnitInfoSubCommand {
    /// Mint URL
    pub mint_url: String,
    /// Currency unit e.g. sat, msat, usd, eur
    #[arg(short, long, default_value = "sat")]
    pub unit: String,
}

pub async fn unit_info(
    multi_mint_wallet: &MultiMintWallet,
    sub_command_args: &UnitInfoSubCommand,
) -> Result<()> {
    let mint_url = MintUrl::from_str(&sub_command_args.mint_url)?;
    let unit = CurrencyUnit::from_str(&sub_command_args.unit)?;
    let wallet_key = WalletKey::new(mint_url.clone(), unit.clone());

    println!("Fetching unit metadata for {}:{}", mint_url, unit);

    // Try to get the wallet, create it if it doesn't exist
    let wallet = match multi_mint_wallet.get_wallet(&wallet_key).await {
        Some(wallet) => {
            println!("Using existing wallet for {}:{}", mint_url, unit);
            wallet
        }
        None => {
            println!("Creating new wallet for {}:{}", mint_url, unit);
            println!("This may take a moment as we fetch mint info...");

            // Use the existing method but handle potential hanging
            match tokio::time::timeout(
                std::time::Duration::from_secs(30),
                multi_mint_wallet.create_and_add_wallet(
                    &sub_command_args.mint_url,
                    unit.clone(),
                    None,
                ),
            )
            .await
            {
                Ok(Ok(wallet)) => wallet,
                Ok(Err(e)) => {
                    println!("Error creating wallet: {}", e);
                    return Err(e.into());
                }
                Err(_) => {
                    println!(
                        "Timeout while creating wallet. The mint might be slow or unreachable."
                    );
                    println!("Try again or check if the mint URL is correct.");
                    return Err(anyhow::anyhow!("Timeout while creating wallet"));
                }
            }
        }
    };

    // Get unit metadata
    println!("Fetching unit metadata from mint...");
    match tokio::time::timeout(
        std::time::Duration::from_secs(10),
        wallet.get_unit_metadata(),
    )
    .await
    {
        Ok(Ok(Some(metadata))) => {
            println!("Unit Metadata for {}:{}", mint_url, unit);
            println!(
                "  Front Card Background: {:?}",
                metadata.front_card_background
            );
            println!(
                "  Back Card Background: {:?}",
                metadata.back_card_background
            );
            println!("  Title: {:?}", metadata.title);
            println!("  Description: {:?}", metadata.description);
            println!("  Kind: {:?}", metadata.kind);
            println!("  Show Individually: {}", metadata.show_individually);
        }
        Ok(Ok(None)) => {
            println!("No unit metadata available for {}:{}", mint_url, unit);
            println!("This could mean:");
            println!("  - The mint doesn't support unit metadata");
            println!("  - The unit is not supported by this mint");
            println!("  - The mint endpoint is not available");
        }
        Ok(Err(e)) => {
            println!("Error fetching unit metadata: {}", e);
            return Err(e.into());
        }
        Err(_) => {
            println!("Timeout while fetching unit metadata.");
            println!("The mint might be slow or the endpoint might not be available.");
            return Err(anyhow::anyhow!("Timeout while fetching unit metadata"));
        }
    }

    Ok(())
}
