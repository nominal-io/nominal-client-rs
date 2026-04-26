use std::path::PathBuf;
use std::str::FromStr;

use anyhow::Context;
use chrono::{DateTime, Utc};
use clap::{ArgGroup, Args, Subcommand};
use nominal::core::{
    AvroStreamIngest, CsvIngest, DataflashIngest, DatasetCreate, DatasetTarget, IngestJob,
    JournalJsonIngest, McapIngest, NominalClient, ParquetIngest, TimeUnit, Timestamp, VideoCreate,
    VideoIngest, VideoTarget,
};

#[derive(Subcommand)]
pub enum IngestCommands {
    /// Upload a CSV file and ingest it into a dataset
    Csv(CsvArgs),
    /// Upload a Parquet file and ingest it into a dataset
    Parquet(ParquetArgs),
    /// Upload an MCAP file and ingest its protobuf timeseries messages into a dataset
    Mcap(McapArgs),
    /// Upload a journald JSON (.jsonl / .jsonl.gz) file and ingest it
    JournalJson(JournalJsonArgs),
    /// Upload a Nominal Avro-stream (.avro) file and ingest it
    AvroStream(AvroStreamArgs),
    /// Upload an ArduPilot DataFlash (.bin) file and ingest it
    ArdupilotDataflash(DataflashArgs),
    /// Upload a video file (.mp4 / .mkv / .avi / .ts) and ingest it
    Video(VideoArgs),
    /// Upload an MCAP file and ingest a single video stream from it by topic
    McapVideo(McapVideoArgs),
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
pub struct McapArgs {
    #[command(flatten)]
    target: TargetArgs,

    /// Only ingest the given topic. Repeatable. Mutually exclusive with --exclude-topic.
    #[arg(long = "include-topic", value_name = "TOPIC")]
    include_topics: Vec<String>,

    /// Skip the given topic during ingest. Repeatable. Mutually exclusive with --include-topic.
    #[arg(long = "exclude-topic", value_name = "TOPIC")]
    exclude_topics: Vec<String>,

    /// Apply a fixed tag to every point: --file-tag KEY VALUE. Repeatable
    #[arg(
        long = "file-tag",
        value_names = ["KEY", "VALUE"],
        num_args = 2,
        action = clap::ArgAction::Append,
    )]
    file_tags: Vec<String>,

    /// Skip invalid topics instead of failing the ingest
    #[arg(long)]
    ignore_invalid_topics: bool,
}

#[derive(Args)]
pub struct JournalJsonArgs {
    #[command(flatten)]
    target: TargetArgs,

    /// Channel name to land log lines in. Defaults to 'logs' server-side.
    #[arg(long, value_name = "NAME")]
    channel: Option<String>,
}

#[derive(Args)]
pub struct AvroStreamArgs {
    #[command(flatten)]
    target: TargetArgs,
}

#[derive(Args)]
pub struct DataflashArgs {
    #[command(flatten)]
    target: TargetArgs,

    /// Apply a fixed tag to every point: --file-tag KEY VALUE. Repeatable
    #[arg(
        long = "file-tag",
        value_names = ["KEY", "VALUE"],
        num_args = 2,
        action = clap::ArgAction::Append,
    )]
    file_tags: Vec<String>,
}

#[derive(Args)]
pub struct VideoArgs {
    #[command(flatten)]
    target: VideoTargetArgs,

    /// RFC3339 timestamp of the first frame
    #[arg(long, value_name = "RFC3339")]
    start: DateTime<Utc>,
}

#[derive(Args)]
pub struct McapVideoArgs {
    #[command(flatten)]
    target: VideoTargetArgs,

    /// MCAP topic carrying the single video stream to ingest
    #[arg(long)]
    topic: String,
}

#[derive(Args)]
#[command(group(
    ArgGroup::new("video_target").required(true).args(["video", "name"])
))]
struct VideoTargetArgs {
    /// Path to the file to upload
    path: PathBuf,

    /// Existing video RID to ingest into
    #[arg(long, value_name = "RID")]
    video: Option<String>,

    /// Name for a new video resource created atomically with the ingest
    #[arg(long)]
    name: Option<String>,

    /// Description for the new video. Requires --name.
    #[arg(long, requires = "name")]
    description: Option<String>,

    /// Add a label to the new video. Repeatable. Requires --name.
    #[arg(long = "label", value_name = "LABEL", requires = "name")]
    labels: Vec<String>,

    /// Add a property to the new video as KEY VALUE. Repeatable. Requires --name.
    #[arg(
        long = "property",
        value_names = ["KEY", "VALUE"],
        num_args = 2,
        action = clap::ArgAction::Append,
        requires = "name",
    )]
    properties: Vec<String>,

    /// Skip waiting for the ingest job to finish; print the ingest job RID
    /// and return immediately. Otherwise the command blocks until the job
    /// reaches a terminal state and prints the resulting video RID.
    #[arg(long)]
    no_wait: bool,
}

#[derive(Args)]
#[command(group(
    ArgGroup::new("target").required(true).args(["dataset", "name"])
))]
struct TargetArgs {
    /// Path to the file to upload
    path: PathBuf,

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

    /// Skip waiting for the ingest job to finish; print the ingest job RID and
    /// return immediately. Otherwise the command blocks until the job reaches
    /// a terminal state and prints the resulting dataset RID.
    #[arg(long)]
    no_wait: bool,
}

#[derive(Args)]
struct UploadArgs {
    #[command(flatten)]
    target: TargetArgs,

    /// Name of the column that contains timestamps
    #[arg(long, value_name = "COLUMN")]
    timestamp_column: String,

    /// Timestamp encoding: iso8601 or a time unit (ns/us/ms/s) for epoch
    #[arg(
        long,
        value_name = "SPEC",
        long_help = "Timestamp encoding. One of:\n\
            \x20\x20iso8601\n\
            \x20\x20<unit>             epoch timestamps (e.g. milliseconds, ns)\n\
            Combine a unit with --relative-to to treat values as offsets from a start time."
    )]
    timestamp_type: TimestampSpec,

    /// Interpret timestamps as offsets from this RFC3339 start time (relative mode)
    #[arg(long, value_name = "RFC3339")]
    relative_to: Option<DateTime<Utc>>,

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
}

#[derive(Clone, Debug)]
enum TimestampSpec {
    Iso8601,
    Epoch(TimeUnit),
}

impl FromStr for TimestampSpec {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let lowered = s.to_ascii_lowercase();
        if lowered == "iso8601" {
            return Ok(Self::Iso8601);
        }
        parse_time_unit(&lowered).map(Self::Epoch)
    }
}

fn parse_time_unit(s: &str) -> Result<TimeUnit, String> {
    match s.trim().to_ascii_lowercase().as_str() {
        "ns" | "nanos" | "nanoseconds" => Ok(TimeUnit::Nanoseconds),
        "us" | "micros" | "microseconds" => Ok(TimeUnit::Microseconds),
        "ms" | "millis" | "milliseconds" => Ok(TimeUnit::Milliseconds),
        "s" | "secs" | "seconds" => Ok(TimeUnit::Seconds),
        other => Err(format!(
            "unknown timestamp type '{other}': expected iso8601 or a time unit (nanoseconds, microseconds, milliseconds, seconds)"
        )),
    }
}

pub async fn handle(cmd: IngestCommands, client: NominalClient) -> anyhow::Result<()> {
    match cmd {
        IngestCommands::Csv(args) => handle_csv(args, client).await,
        IngestCommands::Parquet(args) => handle_parquet(args, client).await,
        IngestCommands::Mcap(args) => handle_mcap(args, client).await,
        IngestCommands::JournalJson(args) => handle_journal_json(args, client).await,
        IngestCommands::AvroStream(args) => handle_avro_stream(args, client).await,
        IngestCommands::ArdupilotDataflash(args) => handle_dataflash(args, client).await,
        IngestCommands::Video(args) => handle_video(args, client).await,
        IngestCommands::McapVideo(args) => handle_mcap_video(args, client).await,
    }
}

async fn handle_csv(args: CsvArgs, client: NominalClient) -> anyhow::Result<()> {
    let CsvArgs { common } = args;
    let target = build_target(&common.target);
    let timestamp = build_timestamp(&common)?;

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

    let path = common.target.path.clone();
    let (job, dataset_rid) = client
        .ingest()
        .upload_csv(&path, target, ingest)
        .await
        .with_context(|| format!("Failed to upload CSV '{}'", path.display()))?;

    print_result(&job, &dataset_rid, "Dataset", common.target.no_wait, client).await
}

async fn handle_parquet(args: ParquetArgs, client: NominalClient) -> anyhow::Result<()> {
    let ParquetArgs { common, archive } = args;
    let target = build_target(&common.target);
    let timestamp = build_timestamp(&common)?;

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

    let path = common.target.path.clone();
    let (job, dataset_rid) = client
        .ingest()
        .upload_parquet(&path, target, ingest)
        .await
        .with_context(|| format!("Failed to upload Parquet '{}'", path.display()))?;

    print_result(&job, &dataset_rid, "Dataset", common.target.no_wait, client).await
}

async fn handle_mcap(args: McapArgs, client: NominalClient) -> anyhow::Result<()> {
    let McapArgs {
        target: target_args,
        include_topics,
        exclude_topics,
        file_tags,
        ignore_invalid_topics,
    } = args;
    let target = build_target(&target_args);

    let mut ingest = McapIngest::new();
    for topic in include_topics {
        ingest = ingest.include_topic(topic);
    }
    for topic in exclude_topics {
        ingest = ingest.exclude_topic(topic);
    }
    for pair in file_tags.chunks(2) {
        ingest = ingest.additional_file_tag(&pair[0], &pair[1]);
    }
    if ignore_invalid_topics {
        ingest = ingest.ignore_invalid_topics(true);
    }

    let path = target_args.path.clone();
    let (job, dataset_rid) = client
        .ingest()
        .upload_mcap(&path, target, ingest)
        .await
        .with_context(|| format!("Failed to upload MCAP '{}'", path.display()))?;

    print_result(&job, &dataset_rid, "Dataset", target_args.no_wait, client).await
}

async fn handle_journal_json(args: JournalJsonArgs, client: NominalClient) -> anyhow::Result<()> {
    let JournalJsonArgs {
        target: target_args,
        channel,
    } = args;
    let target = build_target(&target_args);

    let mut ingest = JournalJsonIngest::new();
    if let Some(name) = channel {
        ingest = ingest.channel(name);
    }

    let path = target_args.path.clone();
    let (job, dataset_rid) = client
        .ingest()
        .upload_journal_json(&path, target, ingest)
        .await
        .with_context(|| format!("Failed to upload journal JSON '{}'", path.display()))?;

    print_result(&job, &dataset_rid, "Dataset", target_args.no_wait, client).await
}

async fn handle_avro_stream(args: AvroStreamArgs, client: NominalClient) -> anyhow::Result<()> {
    let AvroStreamArgs {
        target: target_args,
    } = args;
    let target = build_target(&target_args);

    let path = target_args.path.clone();
    let (job, dataset_rid) = client
        .ingest()
        .upload_avro_stream(&path, target, AvroStreamIngest::new())
        .await
        .with_context(|| format!("Failed to upload Avro stream '{}'", path.display()))?;

    print_result(&job, &dataset_rid, "Dataset", target_args.no_wait, client).await
}

async fn handle_dataflash(args: DataflashArgs, client: NominalClient) -> anyhow::Result<()> {
    let DataflashArgs {
        target: target_args,
        file_tags,
    } = args;
    let target = build_target(&target_args);

    let mut ingest = DataflashIngest::new();
    for pair in file_tags.chunks(2) {
        ingest = ingest.additional_file_tag(&pair[0], &pair[1]);
    }

    let path = target_args.path.clone();
    let (job, dataset_rid) = client
        .ingest()
        .upload_ardupilot_dataflash(&path, target, ingest)
        .await
        .with_context(|| format!("Failed to upload DataFlash '{}'", path.display()))?;

    print_result(&job, &dataset_rid, "Dataset", target_args.no_wait, client).await
}

async fn handle_video(args: VideoArgs, client: NominalClient) -> anyhow::Result<()> {
    let VideoArgs {
        target: target_args,
        start,
    } = args;
    let target = build_video_target(&target_args);
    let ingest = VideoIngest::starting_at(start);

    let path = target_args.path.clone();
    let (job, video_rid) = client
        .ingest()
        .upload_video(&path, target, ingest)
        .await
        .with_context(|| format!("Failed to upload video '{}'", path.display()))?;

    print_result(&job, &video_rid, "Video", target_args.no_wait, client).await
}

async fn handle_mcap_video(args: McapVideoArgs, client: NominalClient) -> anyhow::Result<()> {
    let McapVideoArgs {
        target: target_args,
        topic,
    } = args;
    let target = build_video_target(&target_args);
    let ingest = VideoIngest::mcap_topic(topic);

    let path = target_args.path.clone();
    let (job, video_rid) = client
        .ingest()
        .upload_video(&path, target, ingest)
        .await
        .with_context(|| format!("Failed to upload MCAP video '{}'", path.display()))?;

    print_result(&job, &video_rid, "Video", target_args.no_wait, client).await
}

fn build_target(target: &TargetArgs) -> DatasetTarget {
    // clap's ArgGroup enforces exactly one of --dataset / --name is set.
    if let Some(rid) = &target.dataset {
        return DatasetTarget::Existing(rid.clone());
    }
    let name = target
        .name
        .as_ref()
        .expect("ArgGroup requires --dataset or --name");
    let mut create = DatasetCreate::new(name);
    if let Some(d) = &target.description {
        create = create.description(d);
    }
    if !target.labels.is_empty() {
        create = create.labels(target.labels.clone());
    }
    if !target.properties.is_empty() {
        let pairs: Vec<(String, String)> = target
            .properties
            .chunks(2)
            .map(|p| (p[0].clone(), p[1].clone()))
            .collect();
        create = create.properties(pairs);
    }
    DatasetTarget::New(create)
}

fn build_video_target(target: &VideoTargetArgs) -> VideoTarget {
    // clap's ArgGroup enforces exactly one of --video / --name is set.
    if let Some(rid) = &target.video {
        return VideoTarget::Existing(rid.clone());
    }
    let name = target
        .name
        .as_ref()
        .expect("ArgGroup requires --video or --name");
    let mut create = VideoCreate::new(name);
    if let Some(d) = &target.description {
        create = create.description(d);
    }
    if !target.labels.is_empty() {
        create = create.labels(target.labels.clone());
    }
    if !target.properties.is_empty() {
        let pairs: Vec<(String, String)> = target
            .properties
            .chunks(2)
            .map(|p| (p[0].clone(), p[1].clone()))
            .collect();
        create = create.properties(pairs);
    }
    VideoTarget::New(create)
}

fn build_timestamp(common: &UploadArgs) -> anyhow::Result<Timestamp> {
    let col = common.timestamp_column.clone();
    match (common.timestamp_type.clone(), common.relative_to) {
        (TimestampSpec::Iso8601, None) => Ok(Timestamp::iso8601(col)),
        (TimestampSpec::Iso8601, Some(_)) => {
            anyhow::bail!("--relative-to cannot be combined with iso8601 timestamps")
        }
        (TimestampSpec::Epoch(unit), None) => Ok(Timestamp::epoch(col, unit)),
        (TimestampSpec::Epoch(unit), Some(offset)) => {
            Ok(Timestamp::relative(col, unit).with_offset(offset))
        }
    }
}

async fn print_result(
    job: &IngestJob,
    resource_rid: &str,
    resource_label: &str,
    no_wait: bool,
    client: NominalClient,
) -> anyhow::Result<()> {
    if no_wait {
        println!("Ingest job RID: {}", job.rid());
        println!("{resource_label} RID: {resource_rid}");
        return Ok(());
    }

    // `wait_for_ingest_job` returns Err for Failed / Cancelled / Unknown, so
    // reaching past it means the job completed successfully.
    client
        .ingest()
        .wait_for_ingest_job(job.rid())
        .await
        .with_context(|| format!("ingest job '{}' did not complete successfully", job.rid()))?;

    println!("{resource_label} RID: {resource_rid}");
    Ok(())
}
