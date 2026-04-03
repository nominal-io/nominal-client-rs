use clap::Subcommand;
use nominal_client::NominalClient;

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

pub async fn handle(cmd: AssetCommands, client: NominalClient) {
    match cmd {
        AssetCommands::List => {
            let assets = client.list_assets().await.expect("Failed to list assets");

            for asset in assets {
                println!("{}", asset.rid());
            }
        }
        AssetCommands::Get { rid } => {
            let asset = client.get_asset(&rid).await.expect("Failed to get asset");

            println!("RID: {}", asset.rid());
            println!("Name: {}", asset.name());
            if let Some(description) = asset.description() {
                println!("Description: {}", description);
            }
            if !asset.labels().is_empty() {
                println!("Labels: {}", asset.labels().join(", "));
            }
            if !asset.properties().is_empty() {
                println!("Properties:");
                for (key, value) in asset.properties() {
                    println!("  {}: {}", key, value);
                }
            }
            println!("Created: {} ns", asset.created_at());
            println!("URL: {}", asset.nominal_url());
        }
    }
}
