use anyhow::Context;
use chrono::SecondsFormat;
use clap::Subcommand;
use nominal::core::{Asset, AssetCreate, AssetQuery, AssetUpdate, NominalClient};

#[derive(Subcommand)]
pub enum AssetCommands {
    /// List all assets
    List,
    /// Search assets by substring, label, and/or property. Multiple filters are AND-ed together.
    Search {
        /// Case-insensitive substring match against the asset name. Repeatable
        #[arg(short, long = "substring", value_name = "SUBSTR")]
        substrings: Vec<String>,

        /// Filter by label. Repeatable
        #[arg(short, long = "label", value_name = "LABEL")]
        labels: Vec<String>,

        /// Filter by property KEY VALUE pair. Repeatable
        #[arg(short, long = "property", value_names = ["KEY", "VALUE"], num_args = 2, action = clap::ArgAction::Append)]
        properties: Vec<String>,
    },
    /// Get a specific asset by RID
    Get {
        /// The RID of the asset to retrieve
        rid: String,
    },
    /// Create a new asset
    Create {
        /// The asset name
        #[arg(short, long)]
        name: String,

        /// Set the asset description
        #[arg(short, long)]
        description: Option<String>,

        /// Add labels. Repeatable
        #[arg(short, long = "label", value_name = "LABEL")]
        labels: Vec<String>,

        /// Add properties as KEY VALUE pairs. Repeatable
        #[arg(short, long = "property", value_names = ["KEY", "VALUE"], num_args = 2, action = clap::ArgAction::Append)]
        properties: Vec<String>,
    },
    /// Attach a dataset to an asset under a scope name
    AddDataset {
        /// The RID of the asset
        rid: String,
        /// Scope name for the dataset within the asset
        name: String,
        /// The RID of the dataset to attach
        dataset_rid: String,
    },
    /// Attach a video to an asset under a scope name
    AddVideo {
        /// The RID of the asset
        rid: String,
        /// Scope name for the video within the asset
        name: String,
        /// The RID of the video to attach
        video_rid: String,
    },
    /// Attach a connection to an asset under a scope name
    AddConnection {
        /// The RID of the asset
        rid: String,
        /// Scope name for the connection within the asset
        name: String,
        /// The RID of the connection to attach
        connection_rid: String,
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
        AssetCommands::Search {
            substrings,
            labels,
            properties,
        } => {
            let query = build_asset_query(substrings, labels, properties)?;
            let assets = client
                .assets()
                .search(query)
                .await
                .context("Failed to search assets")?;

            for asset in assets {
                println!("{}", asset.rid());
            }
        }
        AssetCommands::Create {
            name,
            description,
            labels,
            properties,
        } => {
            let mut create = AssetCreate::new(name);

            if let Some(d) = description {
                create = create.description(d);
            }
            if !labels.is_empty() {
                create = create.labels(labels);
            }
            if !properties.is_empty() {
                let props: std::collections::HashMap<_, _> = properties
                    .chunks(2)
                    .map(|pair| (pair[0].clone(), pair[1].clone()))
                    .collect();
                create = create.properties(props);
            }

            let asset = client
                .assets()
                .create(create)
                .await
                .context("Failed to create asset")?;

            print_asset(&asset);
        }
        AssetCommands::Get { rid } => {
            let asset = client
                .assets()
                .get(&rid)
                .await
                .with_context(|| format!("Failed to get asset '{rid}'"))?;

            print_asset(&asset);
        }
        AssetCommands::AddDataset {
            rid,
            name,
            dataset_rid,
        } => {
            let asset = client
                .assets()
                .add_dataset(&rid, &name, &dataset_rid)
                .await
                .with_context(|| format!("Failed to attach dataset to asset '{rid}'"))?;
            print_asset(&asset);
        }
        AssetCommands::AddVideo {
            rid,
            name,
            video_rid,
        } => {
            let asset = client
                .assets()
                .add_video(&rid, &name, &video_rid)
                .await
                .with_context(|| format!("Failed to attach video to asset '{rid}'"))?;
            print_asset(&asset);
        }
        AssetCommands::AddConnection {
            rid,
            name,
            connection_rid,
        } => {
            let asset = client
                .assets()
                .add_connection(&rid, &name, &connection_rid)
                .await
                .with_context(|| format!("Failed to attach connection to asset '{rid}'"))?;
            print_asset(&asset);
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

            print_asset(&asset);
        }
    }

    Ok(())
}

fn print_asset(asset: &Asset) {
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
    if !asset.data_sources().is_empty() {
        println!("Data sources:");
        for (name, ds) in asset.data_sources() {
            println!("  {name}: {} ({})", ds.rid(), data_source_kind(ds));
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

fn build_asset_query(
    substrings: Vec<String>,
    labels: Vec<String>,
    properties: Vec<String>,
) -> anyhow::Result<AssetQuery> {
    let mut filters: Vec<AssetQuery> = Vec::new();
    filters.extend(substrings.into_iter().map(AssetQuery::substring_match));
    filters.extend(labels.into_iter().map(AssetQuery::label));
    if properties.len() % 2 != 0 {
        anyhow::bail!("--property requires KEY VALUE pairs");
    }
    filters.extend(
        properties
            .chunks(2)
            .map(|p| AssetQuery::property(p[0].clone(), p[1].clone())),
    );
    Ok(match filters.len() {
        0 => AssetQuery::search_text(""),
        1 => filters.into_iter().next().unwrap(),
        _ => AssetQuery::and(filters),
    })
}

fn data_source_kind(ds: &nominal::core::DataSource) -> &'static str {
    match ds {
        nominal::core::DataSource::Dataset(_) => "dataset",
        nominal::core::DataSource::Video(_) => "video",
        nominal::core::DataSource::Connection(_) => "connection",
    }
}
