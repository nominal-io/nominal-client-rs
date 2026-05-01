use anyhow::Context;
use chrono::{DateTime, SecondsFormat, Utc};
use clap::Subcommand;
use nominal::core::{NominalClient, Run, RunCreate, RunQuery, RunUpdate};

#[derive(Subcommand)]
pub enum RunCommands {
    /// List all runs
    List,
    /// Search runs by substring, label, and/or property. Multiple filters are AND-ed together.
    Search {
        /// Case-insensitive substring match against the run name. Repeatable
        #[arg(short, long = "substring", value_name = "SUBSTR")]
        substrings: Vec<String>,

        /// Filter by label. Repeatable
        #[arg(short, long = "label", value_name = "LABEL")]
        labels: Vec<String>,

        /// Filter by property KEY VALUE pair. Repeatable
        #[arg(short, long = "property", value_names = ["KEY", "VALUE"], num_args = 2, action = clap::ArgAction::Append)]
        properties: Vec<String>,
    },
    /// Get a specific run by RID
    Get {
        /// The RID of the run to retrieve
        rid: String,
    },
    /// Create a new run
    Create {
        /// The run name
        #[arg(short, long)]
        name: String,

        /// RFC3339 timestamp when the run started
        #[arg(short, long, value_name = "RFC3339")]
        start: DateTime<Utc>,

        /// RFC3339 timestamp when the run ended
        #[arg(short, long, value_name = "RFC3339")]
        end: Option<DateTime<Utc>>,

        /// Set the run description
        #[arg(short, long)]
        description: Option<String>,

        /// Add labels. Repeatable
        #[arg(short, long = "label", value_name = "LABEL")]
        labels: Vec<String>,

        /// Add properties as KEY VALUE pairs. Repeatable
        #[arg(short, long = "property", value_names = ["KEY", "VALUE"], num_args = 2, action = clap::ArgAction::Append)]
        properties: Vec<String>,

        /// Associate the run with one or more assets by RID. Repeatable
        #[arg(short, long = "asset", value_name = "RID")]
        assets: Vec<String>,
    },
    /// Attach a dataset to a run under a ref name
    AddDataset {
        /// The RID of the run
        rid: String,
        /// Ref name for the dataset within the run
        name: String,
        /// The RID of the dataset to attach
        dataset_rid: String,
    },
    /// Attach a video to a run under a ref name
    AddVideo {
        /// The RID of the run
        rid: String,
        /// Ref name for the video within the run
        name: String,
        /// The RID of the video to attach
        video_rid: String,
    },
    /// Attach a connection to a run under a ref name
    AddConnection {
        /// The RID of the run
        rid: String,
        /// Ref name for the connection within the run
        name: String,
        /// The RID of the connection to attach
        connection_rid: String,
    },
    /// Update run metadata
    Update {
        /// The RID of the run to update
        rid: String,

        /// Set the run name
        #[arg(short, long)]
        name: Option<String>,

        /// Set the run description
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
    /// Archive a run
    Archive {
        /// The RID of the run to archive
        rid: String,
    },
    /// Unarchive a run
    Unarchive {
        /// The RID of the run to unarchive
        rid: String,
    },
}

pub async fn handle(cmd: RunCommands, client: NominalClient) -> anyhow::Result<()> {
    match cmd {
        RunCommands::List => {
            let runs = client.runs().list().await.context("Failed to list runs")?;
            for run in runs {
                println!("{}", run.rid());
            }
        }
        RunCommands::Search {
            substrings,
            labels,
            properties,
        } => {
            let query = build_run_query(substrings, labels, properties)?;
            let runs = client
                .runs()
                .search(query)
                .await
                .context("Failed to search runs")?;
            for run in runs {
                println!("{}", run.rid());
            }
        }
        RunCommands::Create {
            name,
            start,
            end,
            description,
            labels,
            properties,
            assets,
        } => {
            let mut create = RunCreate::new(name, start);
            if let Some(d) = description {
                create = create.description(d);
            }
            if let Some(e) = end {
                create = create.end(e);
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
            if !assets.is_empty() {
                create = create.assets(assets);
            }

            let run = client
                .runs()
                .create(create)
                .await
                .context("Failed to create run")?;
            print_run(&run);
        }
        RunCommands::Get { rid } => {
            let run = client
                .runs()
                .get(&rid)
                .await
                .with_context(|| format!("Failed to get run '{rid}'"))?;
            print_run(&run);
        }
        RunCommands::AddDataset {
            rid,
            name,
            dataset_rid,
        } => {
            let run = client
                .runs()
                .add_dataset(&rid, &name, &dataset_rid)
                .await
                .with_context(|| format!("Failed to attach dataset to run '{rid}'"))?;
            print_run(&run);
        }
        RunCommands::AddVideo {
            rid,
            name,
            video_rid,
        } => {
            let run = client
                .runs()
                .add_video(&rid, &name, &video_rid)
                .await
                .with_context(|| format!("Failed to attach video to run '{rid}'"))?;
            print_run(&run);
        }
        RunCommands::AddConnection {
            rid,
            name,
            connection_rid,
        } => {
            let run = client
                .runs()
                .add_connection(&rid, &name, &connection_rid)
                .await
                .with_context(|| format!("Failed to attach connection to run '{rid}'"))?;
            print_run(&run);
        }
        RunCommands::Update {
            rid,
            name,
            description,
            labels,
            clear_labels,
            properties,
            clear_properties,
        } => {
            let mut update = RunUpdate::new();

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

            let run = client
                .runs()
                .update(&rid, update)
                .await
                .with_context(|| format!("Failed to update run '{rid}'"))?;
            print_run(&run);
        }
        RunCommands::Archive { rid } => {
            client
                .runs()
                .archive(&rid)
                .await
                .with_context(|| format!("Failed to archive run '{rid}'"))?;
            println!("Archived run: {rid}");
        }
        RunCommands::Unarchive { rid } => {
            client
                .runs()
                .unarchive(&rid)
                .await
                .with_context(|| format!("Failed to unarchive run '{rid}'"))?;
            println!("Unarchived run: {rid}");
        }
    }

    Ok(())
}

fn print_run(run: &Run) {
    println!("RID: {}", run.rid());
    println!("Run #: {}", run.run_number());
    println!("Name: {}", run.name());
    if !run.description().is_empty() {
        println!("Description: {}", run.description());
    }
    println!(
        "Start: {}",
        run.start().to_rfc3339_opts(SecondsFormat::Nanos, true)
    );
    if let Some(end) = run.end() {
        println!("End: {}", end.to_rfc3339_opts(SecondsFormat::Nanos, true));
    }
    if !run.labels().is_empty() {
        println!("Labels: {}", run.labels().join(", "));
    }
    if !run.properties().is_empty() {
        println!("Properties:");
        for (key, value) in run.properties() {
            println!("  {key}: {value}");
        }
    }
    if !run.assets().is_empty() {
        println!("Assets:");
        for asset in run.assets() {
            println!("  {asset}");
        }
    }
    if !run.data_sources().is_empty() {
        println!("Data sources:");
        for (name, ds) in run.data_sources() {
            println!("  {name}: {} ({})", ds.rid(), data_source_kind(ds));
        }
    }
    println!(
        "Created: {}",
        run.created_at().to_rfc3339_opts(SecondsFormat::Nanos, true)
    );
    println!("URL: {}", run.nominal_url());
}

fn build_run_query(
    substrings: Vec<String>,
    labels: Vec<String>,
    properties: Vec<String>,
) -> anyhow::Result<RunQuery> {
    let mut filters: Vec<RunQuery> = Vec::new();
    filters.extend(substrings.into_iter().map(RunQuery::substring_match));
    filters.extend(labels.into_iter().map(RunQuery::label));
    if properties.len() % 2 != 0 {
        anyhow::bail!("--property requires KEY VALUE pairs");
    }
    filters.extend(
        properties
            .chunks(2)
            .map(|p| RunQuery::property(p[0].clone(), p[1].clone())),
    );
    Ok(match filters.len() {
        0 => RunQuery::search_text(""),
        1 => filters.into_iter().next().unwrap(),
        _ => RunQuery::and(filters),
    })
}

fn data_source_kind(ds: &nominal::core::DataSource) -> &'static str {
    match ds {
        nominal::core::DataSource::Dataset(_) => "dataset",
        nominal::core::DataSource::Video(_) => "video",
        nominal::core::DataSource::Connection(_) => "connection",
    }
}
