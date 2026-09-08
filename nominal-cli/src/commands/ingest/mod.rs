mod native;

use clap::Subcommand;
use native::*;
use nominal::core::NominalClient;

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
