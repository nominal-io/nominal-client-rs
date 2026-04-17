use std::path::PathBuf;
use std::str::FromStr;

use anyhow::Context;
use chrono::{DateTime, Utc};
use clap::{ArgGroup, Args, Subcommand};
use nominal::core::{
    CsvIngest, DatasetCreate, DatasetTarget, IngestJob, IngestJobStatus, NominalClient,
    ParquetIngest, TimeUnit, Timestamp,
};

#[derive(Subcommand)]
pub enum IngestCommands {
    /// Upload a CSV file and ingest it into a dataset
    Csv(CsvArgs),
    /// Upload a Parquet file and ingest it into a dataset
    Parquet(ParquetArgs),
}

#[derive(Args)]
pub struct CsvArgs {
    #[command(flatten)]
    common: UploadArgs,
}

#[derive(Args)]
pub struct ParquetArgs {
    #[command(flatten)]
    common: UploadArgs,

    /// Treat the file as an archive (.tar, .tar.gz, .zip) of parquet files
    #[arg(long)]
    archive: bool,
}

#[derive(Args)]
#[command(group(
    ArgGroup::new("target").required(true).args(["dataset", "name"])
))]
struct UploadArgs {
    /// Path to the file to upload
    path: PathBuf,

    // ── Target ───────────────────────────────────────────────────────────────
    /// Existing dataset RID to ingest into
    #[arg(long, value_name = "RID")]
    dataset: Option<String>,

    /// Name for a new dataset created atomically with the ingest
    #[arg(long)]
    name: Option<String>,

    /// Description for the new dataset. Requires --name.
    #[arg(long, requires = "name")]
    description: Option<String>,

    /// Add a label to the new dataset. Repeatable. Requires --name.
    #[arg(long = "label", value_name = "LABEL", requires = "name")]
    labels: Vec<String>,

    /// Add a property to the new dataset as KEY VALUE. Repeatable. Requires --name.
    #[arg(
        long = "property",
        value_names = ["KEY", "VALUE"],
        num_args = 2,
        action = clap::ArgAction::Append,
        requires = "name",
    )]
    properties: Vec<String>,

    // ── Timestamp ────────────────────────────────────────────────────────────
    /// Name of the column that contains timestamps
    #[arg(long, value_name = "COLUMN")]
    timestamp_column: String,

    /// Timestamp encoding (iso8601 / epoch:<unit> / relative:<unit>)
    #[arg(
        long,
        value_name = "SPEC",
        long_help = "Timestamp encoding. One of:\n\
            \x20\x20iso8601\n\
            \x20\x20epoch:<unit>       (e.g. epoch:milliseconds, epoch:ns)\n\
            \x20\x20relative:<unit>    (e.g. relative:us)"
    )]
    timestamp_type: TimestampSpec,

    /// Start time (RFC3339) for relative timestamps.
    #[arg(long, value_name = "RFC3339")]
    timestamp_offset: Option<DateTime<Utc>>,

    // ── Ingest options ───────────────────────────────────────────────────────
    /// Prepend this prefix to every channel name in the file
    #[arg(long, value_name = "PREFIX")]
    channel_prefix: Option<String>,

    /// Derive a tag's value from a column: --tag-column TAG COLUMN. Repeatable
    #[arg(
        long = "tag-column",
        value_names = ["TAG", "COLUMN"],
        num_args = 2,
        action = clap::ArgAction::Append,
    )]
    tag_columns: Vec<String>,

    /// Apply a fixed tag to every point: --file-tag KEY VALUE. Repeatable
    #[arg(
        long = "file-tag",
        value_names = ["KEY", "VALUE"],
        num_args = 2,
        action = clap::ArgAction::Append,
    )]
    file_tags: Vec<String>,

    /// Exclude a column from ingestion. Repeatable
    #[arg(long = "exclude-column", value_name = "COLUMN")]
    exclude_columns: Vec<String>,

    // ── Control ──────────────────────────────────────────────────────────────
    /// Block until the ingest job reaches a terminal state
    #[arg(long)]
    wait: bool,
}

#[derive(Clone, Debug)]
enum TimestampSpec {
    Iso8601,
    Epoch(TimeUnit),
    Relative(TimeUnit),
}

impl FromStr for TimestampSpec {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let lowered = s.to_ascii_lowercase();
        if lowered == "iso8601" {
            return Ok(Self::Iso8601);
        }
        if let Some(unit) = lowered.strip_prefix("epoch:") {
            return parse_time_unit(unit).map(Self::Epoch);
        }
        if let Some(unit) = lowered.strip_prefix("relative:") {
            return parse_time_unit(unit).map(Self::Relative);
        }
        Err(format!(
            "unknown timestamp type '{s}': expected one of iso8601, epoch:<unit>, relative:<unit>"
        ))
    }
}

fn parse_time_unit(s: &str) -> Result<TimeUnit, String> {
    match s.trim().to_ascii_lowercase().as_str() {
        "ns" | "nanos" | "nanoseconds" => Ok(TimeUnit::Nanoseconds),
        "us" | "micros" | "microseconds" => Ok(TimeUnit::Microseconds),
        "ms" | "millis" | "milliseconds" => Ok(TimeUnit::Milliseconds),
        "s" | "secs" | "seconds" => Ok(TimeUnit::Seconds),
        other => Err(format!(
            "unknown time unit '{other}': expected one of nanoseconds, microseconds, milliseconds, seconds"
        )),
    }
}

pub async fn handle(cmd: IngestCommands, client: NominalClient) -> anyhow::Result<()> {
    match cmd {
        IngestCommands::Csv(args) => handle_csv(args, client).await,
        IngestCommands::Parquet(args) => handle_parquet(args, client).await,
    }
}

async fn handle_csv(args: CsvArgs, client: NominalClient) -> anyhow::Result<()> {
    let CsvArgs { common } = args;
    let target = build_target(&common);
    let timestamp = build_timestamp(&common);

    let mut ingest = CsvIngest::new(timestamp);
    if let Some(prefix) = &common.channel_prefix {
        ingest = ingest.channel_prefix(prefix);
    }
    for pair in common.tag_columns.chunks(2) {
        ingest = ingest.tag_column(&pair[0], &pair[1]);
    }
    for pair in common.file_tags.chunks(2) {
        ingest = ingest.additional_file_tag(&pair[0], &pair[1]);
    }
    for col in &common.exclude_columns {
        ingest = ingest.exclude_column(col);
    }

    let path = common.path.clone();
    let job = client
        .ingest()
        .upload_csv(&path, target, ingest)
        .await
        .with_context(|| format!("Failed to upload CSV '{}'", path.display()))?;

    print_result(&job, common.wait, client).await
}

async fn handle_parquet(args: ParquetArgs, client: NominalClient) -> anyhow::Result<()> {
    let ParquetArgs { common, archive } = args;
    let target = build_target(&common);
    let timestamp = build_timestamp(&common);

    let mut ingest = ParquetIngest::new(timestamp);
    if let Some(prefix) = &common.channel_prefix {
        ingest = ingest.channel_prefix(prefix);
    }
    for pair in common.tag_columns.chunks(2) {
        ingest = ingest.tag_column(&pair[0], &pair[1]);
    }
    for pair in common.file_tags.chunks(2) {
        ingest = ingest.additional_file_tag(&pair[0], &pair[1]);
    }
    for col in &common.exclude_columns {
        ingest = ingest.exclude_column(col);
    }
    if archive {
        ingest = ingest.is_archive(true);
    }

    let path = common.path.clone();
    let job = client
        .ingest()
        .upload_parquet(&path, target, ingest)
        .await
        .with_context(|| format!("Failed to upload Parquet '{}'", path.display()))?;

    print_result(&job, common.wait, client).await
}

fn build_target(common: &UploadArgs) -> DatasetTarget {
    // clap's ArgGroup enforces exactly one of --dataset / --name is set.
    if let Some(rid) = &common.dataset {
        return DatasetTarget::Existing(rid.clone());
    }
    let name = common
        .name
        .as_ref()
        .expect("ArgGroup requires --dataset or --name");
    let mut create = DatasetCreate::new(name);
    if let Some(d) = &common.description {
        create = create.description(d);
    }
    if !common.labels.is_empty() {
        create = create.labels(common.labels.clone());
    }
    if !common.properties.is_empty() {
        let pairs: Vec<(String, String)> = common
            .properties
            .chunks(2)
            .map(|p| (p[0].clone(), p[1].clone()))
            .collect();
        create = create.properties(pairs);
    }
    DatasetTarget::New(create)
}

fn build_timestamp(common: &UploadArgs) -> Timestamp {
    let col = common.timestamp_column.clone();
    match common.timestamp_type.clone() {
        TimestampSpec::Iso8601 => Timestamp::iso8601(col),
        TimestampSpec::Epoch(unit) => Timestamp::epoch(col, unit),
        TimestampSpec::Relative(unit) => {
            let mut ts = Timestamp::relative(col, unit);
            if let Some(offset) = common.timestamp_offset {
                ts = ts.with_offset(offset);
            }
            ts
        }
    }
}

async fn print_result(job: &IngestJob, wait: bool, client: NominalClient) -> anyhow::Result<()> {
    println!("Ingest job RID: {}", job.rid());
    println!("Status: {}", status_str(job.status()));

    if wait {
        let terminal = client
            .ingest()
            .wait_for_ingest_job(job.rid())
            .await
            .with_context(|| format!("ingest job '{}' did not complete successfully", job.rid()))?;
        println!("Final status: {}", status_str(terminal.status()));
    }
    Ok(())
}

fn status_str(status: &IngestJobStatus) -> String {
    match status {
        IngestJobStatus::Submitted => "Submitted".into(),
        IngestJobStatus::Queued => "Queued".into(),
        IngestJobStatus::InProgress => "InProgress".into(),
        IngestJobStatus::Completed => "Completed".into(),
        IngestJobStatus::Failed => "Failed".into(),
        IngestJobStatus::Cancelled => "Cancelled".into(),
        IngestJobStatus::Unknown(s) => format!("Unknown({s})"),
    }
}
