mod containerized;
mod jobs;
mod native;
mod render;

use crate::commands::load_client;
use clap::Subcommand;
use native::*;

#[derive(Subcommand)]
pub enum IngestCommands {
    /// Run a containerized extractor with named input files
    Containerized(containerized::ContainerizedArgs),
    /// Inspect, wait for, and cancel ingest jobs
    Job {
        #[command(subcommand)]
        command: jobs::JobCommands,
    },
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

pub async fn handle(cmd: IngestCommands, profile: Option<&str>) -> anyhow::Result<()> {
    match cmd {
        IngestCommands::Containerized(a) => containerized::handle(a, profile).await,
        IngestCommands::Job { command } => jobs::handle(command, load_client(profile)?).await,
        IngestCommands::Csv(args) => handle_csv(args, load_client(profile)?).await,
        IngestCommands::Parquet(args) => handle_parquet(args, load_client(profile)?).await,
        IngestCommands::Mcap(args) => handle_mcap(args, load_client(profile)?).await,
        IngestCommands::JournalJson(args) => handle_journal_json(args, load_client(profile)?).await,
        IngestCommands::AvroStream(args) => handle_avro_stream(args, load_client(profile)?).await,
        IngestCommands::ArdupilotDataflash(args) => {
            handle_dataflash(args, load_client(profile)?).await
        }
        IngestCommands::Video(args) => handle_video(args, load_client(profile)?).await,
        IngestCommands::McapVideo(args) => handle_mcap_video(args, load_client(profile)?).await,
    }
}
