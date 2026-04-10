use anyhow::Context;
use chrono::SecondsFormat;
use clap::Subcommand;
use nominal::NominalClient;

#[derive(Subcommand)]
pub enum AssetCommands {
    /// List all assets
    List,
    /// Get a specific asset by RID
    Get {
        /// The RID of the asset to retrieve
        rid: String,
    },
}

pub async fn handle(cmd: AssetCommands, client: NominalClient) -> anyhow::Result<()> {
    match cmd {
        AssetCommands::List => {
            let assets = client
                .assets()
                .list()
                .await
                .context("Failed to list assets")?;

            for asset in assets {
                println!("{}", asset.rid());
            }
        }
        AssetCommands::Get { rid } => {
            let asset = client
                .assets()
                .get(&rid)
                .await
                .with_context(|| format!("Failed to get asset '{rid}'"))?;

            println!("RID: {}", asset.rid());
            println!("Name: {}", asset.name());
            if let Some(description) = asset.description() {
                println!("Description: {description}");
            }
            if !asset.labels().is_empty() {
                println!("Labels: {}", asset.labels().join(", "));
            }
            if !asset.properties().is_empty() {
                println!("Properties:");
                for (key, value) in asset.properties() {
                    println!("  {key}: {value}");
                }
            }
            println!(
                "Created: {}",
                asset
                    .created_at()
                    .to_rfc3339_opts(SecondsFormat::Nanos, true)
            );
            println!("URL: {}", asset.nominal_url());
        }
    }

    Ok(())
}
