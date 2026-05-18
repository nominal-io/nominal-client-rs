use anyhow::Context;
use clap::Subcommand;
use nominal::core::{Channel, ChannelDataType, ChannelQuery, ChannelUpdate, NominalClient};

#[derive(Subcommand)]
pub enum ChannelCommands {
    /// List every channel on a data source
    List {
        /// RID of the data source (dataset, video, connection, etc.)
        data_source_rid: String,
    },
    /// Search channels across one or more data sources
    Search {
        /// Restrict to a specific data source. Repeatable
        #[arg(short, long = "data-source", value_name = "RID")]
        data_sources: Vec<String>,

        /// Require channel name to contain this substring (case-insensitive). Repeatable
        #[arg(short = 's', long = "substring", value_name = "SUBSTR")]
        substring_matches: Vec<String>,

        /// Restrict to a specific data type. Repeatable.
        /// One of: double, int, uint, string, log, double-array, string-array, struct, video
        #[arg(long = "data-type", value_name = "TYPE")]
        data_types: Vec<String>,
    },
    /// Get a single channel's metadata
    Get {
        /// RID of the data source that owns the channel
        data_source_rid: String,
        /// Channel name
        name: String,
    },
    /// Set a channel's metadata (description and/or unit)
    Set {
        /// RID of the data source that owns the channel
        data_source_rid: String,
        /// Channel name
        name: String,

        /// Channel data type. One of: double, int, uint, string, log, double-array, string-array, struct, video
        #[arg(long = "data-type", value_name = "TYPE")]
        data_type: String,

        /// Set the channel description
        #[arg(short, long)]
        description: Option<String>,

        /// Set the channel unit (e.g. "m/s", "celsius"). Mutually exclusive with --clear-unit
        #[arg(short, long, conflicts_with = "clear_unit")]
        unit: Option<String>,

        /// Clear any unit previously set on the channel
        #[arg(long, conflicts_with = "unit")]
        clear_unit: bool,
    },
}

pub async fn handle(cmd: ChannelCommands, client: NominalClient) -> anyhow::Result<()> {
    match cmd {
        ChannelCommands::List { data_source_rid } => {
            let channels = client
                .catalog()
                .list_channels(&data_source_rid)
                .await
                .with_context(|| format!("Failed to list channels on '{data_source_rid}'"))?;
            for channel in &channels {
                println!("{}", channel.name());
            }
        }
        ChannelCommands::Search {
            data_sources,
            substring_matches,
            data_types,
        } => {
            let mut query = ChannelQuery::new();
            if !data_sources.is_empty() {
                query = query.data_sources(data_sources);
            }
            for m in substring_matches {
                query = query.substring_match(m);
            }
            for dt in data_types {
                query = query.data_type(parse_data_type(&dt)?);
            }
            let channels = client
                .catalog()
                .search_channels(query)
                .await
                .context("Failed to search channels")?;
            for channel in &channels {
                println!("{}\t{}", channel.data_source_rid(), channel.name());
            }
        }
        ChannelCommands::Get {
            data_source_rid,
            name,
        } => {
            let channel = client
                .catalog()
                .get_channel(&data_source_rid, &name)
                .await
                .with_context(|| {
                    format!("Failed to get channel '{name}' on '{data_source_rid}'")
                })?;
            print_channel(&channel);
        }
        ChannelCommands::Set {
            data_source_rid,
            name,
            data_type,
            description,
            unit,
            clear_unit,
        } => {
            let mut update = ChannelUpdate::new(parse_data_type(&data_type)?);
            if let Some(d) = description {
                update = update.description(d);
            }
            if let Some(u) = unit {
                update = update.unit(u);
            } else if clear_unit {
                update = update.clear_unit();
            }
            let channel = client
                .catalog()
                .set_channel_metadata(&data_source_rid, &name, update)
                .await
                .with_context(|| {
                    format!("Failed to set metadata for channel '{name}' on '{data_source_rid}'")
                })?;
            print_channel(&channel);
        }
    }
    Ok(())
}

fn parse_data_type(s: &str) -> anyhow::Result<ChannelDataType> {
    match s.to_ascii_lowercase().as_str() {
        "double" => Ok(ChannelDataType::Double),
        "int" => Ok(ChannelDataType::Int),
        "uint" => Ok(ChannelDataType::Uint),
        "string" => Ok(ChannelDataType::String),
        "log" => Ok(ChannelDataType::Log),
        "double-array" | "double_array" => Ok(ChannelDataType::DoubleArray),
        "string-array" | "string_array" => Ok(ChannelDataType::StringArray),
        "struct" => Ok(ChannelDataType::Struct),
        "video" => Ok(ChannelDataType::Video),
        other => Err(anyhow::anyhow!(
            "unknown data type '{other}': expected one of double, int, uint, string, log, double-array, string-array, struct, video"
        )),
    }
}

fn print_channel(channel: &Channel) {
    println!("Data source RID: {}", channel.data_source_rid());
    println!("Name: {}", channel.name());
    if let Some(description) = channel.description() {
        println!("Description: {description}");
    }
    if let Some(unit) = channel.unit() {
        println!("Unit: {unit}");
    }
    println!("Data type: {}", data_type_str(channel.data_type()));
}

fn data_type_str(t: &ChannelDataType) -> String {
    match t {
        ChannelDataType::Double => "Double".into(),
        ChannelDataType::Int => "Int".into(),
        ChannelDataType::Uint => "Uint".into(),
        ChannelDataType::String => "String".into(),
        ChannelDataType::Log => "Log".into(),
        ChannelDataType::DoubleArray => "DoubleArray".into(),
        ChannelDataType::StringArray => "StringArray".into(),
        ChannelDataType::Struct => "Struct".into(),
        ChannelDataType::Video => "Video".into(),
        ChannelDataType::Unknown(s) => format!("Unknown({s})"),
    }
}
