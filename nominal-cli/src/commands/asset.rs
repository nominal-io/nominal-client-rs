use anyhow::Context;
use chrono::SecondsFormat;
use clap::Subcommand;
use nominal::core::{AssetUpdate, NominalClient};

#[derive(Subcommand)]
pub enum AssetCommands {
    /// List all assets
    List,
    /// Get a specific asset by RID
    Get {
        /// The RID of the asset to retrieve
        rid: String,
    },
    /// Update asset metadata
    Update {
        /// The RID of the asset to update
        rid: String,

        /// Set the asset name
        #[arg(short, long)]
        name: Option<String>,

        /// Set the asset description
        #[arg(short, long)]
        description: Option<String>,

        /// Replace all labels. Repeatable. Omit to leave labels unchanged
        #[arg(
            short,
            long = "label",
            value_name = "LABEL",
            conflicts_with = "clear_labels"
        )]
        labels: Vec<String>,

        /// Clear all labels
        #[arg(long, conflicts_with = "labels")]
        clear_labels: bool,

        /// Replace all properties as KEY VALUE pairs. Repeatable. Omit to leave properties unchanged
        #[arg(short, long = "property", value_names = ["KEY", "VALUE"], num_args = 2, action = clap::ArgAction::Append, conflicts_with = "clear_properties")]
        properties: Vec<String>,

        /// Clear all properties
        #[arg(long, conflicts_with = "properties")]
        clear_properties: bool,
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
        AssetCommands::Update {
            rid,
            name,
            description,
            labels,
            clear_labels,
            properties,
            clear_properties,
        } => {
            let mut update = AssetUpdate::new();

            if let Some(n) = name {
                update = update.name(n);
            }

            if let Some(d) = description {
                update = update.description(d);
            }

            if clear_labels {
                update = update.labels([] as [String; 0]);
            } else if !labels.is_empty() {
                update = update.labels(labels);
            }

            if clear_properties {
                update = update.properties([] as [(String, String); 0]);
            } else if !properties.is_empty() {
                let props: std::collections::HashMap<_, _> = properties
                    .chunks(2)
                    .map(|pair| (pair[0].clone(), pair[1].clone()))
                    .collect();
                update = update.properties(props);
            }

            let asset = client
                .assets()
                .update(&rid, update)
                .await
                .with_context(|| format!("Failed to update asset '{rid}'"))?;

            println!("Updated asset: {}", asset.rid());
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
        }
    }

    Ok(())
}
