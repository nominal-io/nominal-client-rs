use anyhow::Context;
use chrono::SecondsFormat;
use clap::Subcommand;
use nominal::core::{DatasetCreate, DatasetUpdate, NominalClient};

#[derive(Subcommand)]
pub enum DatasetCommands {
    /// List all datasets
    List,
    /// Get a specific dataset by RID
    Get {
        /// The RID of the dataset to retrieve
        rid: String,
    },
    /// Create a new dataset
    Create {
        /// The dataset name
        #[arg(short, long)]
        name: String,

        /// Set the dataset description
        #[arg(short, long)]
        description: Option<String>,

        /// Add labels. Repeatable
        #[arg(short, long = "label", value_name = "LABEL")]
        labels: Vec<String>,

        /// Add properties as KEY VALUE pairs. Repeatable
        #[arg(short, long = "property", value_names = ["KEY", "VALUE"], num_args = 2, action = clap::ArgAction::Append)]
        properties: Vec<String>,
    },
    /// Update dataset metadata
    Update {
        /// The RID of the dataset to update
        rid: String,

        /// Set the dataset name
        #[arg(short, long)]
        name: Option<String>,

        /// Set the dataset description
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
    /// Archive a dataset
    Archive {
        /// The RID of the dataset to archive
        rid: String,
    },
    /// Unarchive a dataset
    Unarchive {
        /// The RID of the dataset to unarchive
        rid: String,
    },
}

pub async fn handle(cmd: DatasetCommands, client: NominalClient) -> anyhow::Result<()> {
    match cmd {
        DatasetCommands::List => {
            let datasets = client
                .catalog()
                .list_datasets()
                .await
                .context("Failed to list datasets")?;

            for dataset in datasets {
                println!("{}", dataset.rid());
            }
        }
        DatasetCommands::Get { rid } => {
            let dataset = client
                .catalog()
                .get_dataset(&rid)
                .await
                .with_context(|| format!("Failed to get dataset '{rid}'"))?;

            print_dataset(&dataset);
        }
        DatasetCommands::Create {
            name,
            description,
            labels,
            properties,
        } => {
            let mut create = DatasetCreate::new(name);

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

            let dataset = client
                .catalog()
                .create_dataset(create)
                .await
                .context("Failed to create dataset")?;

            print_dataset(&dataset);
        }
        DatasetCommands::Update {
            rid,
            name,
            description,
            labels,
            clear_labels,
            properties,
            clear_properties,
        } => {
            let mut update = DatasetUpdate::new();

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

            let dataset = client
                .catalog()
                .update_dataset(&rid, update)
                .await
                .with_context(|| format!("Failed to update dataset '{rid}'"))?;

            print_dataset(&dataset);
        }
        DatasetCommands::Archive { rid } => {
            client
                .catalog()
                .archive_dataset(&rid)
                .await
                .with_context(|| format!("Failed to archive dataset '{rid}'"))?;

            println!("Archived dataset: {rid}");
        }
        DatasetCommands::Unarchive { rid } => {
            client
                .catalog()
                .unarchive_dataset(&rid)
                .await
                .with_context(|| format!("Failed to unarchive dataset '{rid}'"))?;

            println!("Unarchived dataset: {rid}");
        }
    }

    Ok(())
}

fn print_dataset(dataset: &nominal::core::Dataset) {
    println!("RID: {}", dataset.rid());
    println!("Name: {}", dataset.name());
    if let Some(description) = dataset.description() {
        println!("Description: {description}");
    }
    if !dataset.labels().is_empty() {
        println!("Labels: {}", dataset.labels().join(", "));
    }
    if !dataset.properties().is_empty() {
        println!("Properties:");
        for (key, value) in dataset.properties() {
            println!("  {key}: {value}");
        }
    }
    println!(
        "Created: {}",
        dataset
            .created_at()
            .to_rfc3339_opts(SecondsFormat::Nanos, true)
    );
    println!("URL: {}", dataset.nominal_url());
}
