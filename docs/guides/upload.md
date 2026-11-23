# Upload files

{.lead}
Upload a file into a dataset and ingest that file using Nominal making the file's [channels](https://dev.nominal.io/platform/data-model/#channels) available.

To use this page, first make sure you can [Connect to Nominal](connect.md) and obtain a `client`.

## Upload a file

Getting data from a file into Nominal takes two steps. First, an upload, which sends the file to Nominal, then an ingest job, which processes the uploaded file into time-series data Nominal understands. Each upload method does both, and returns the job so you can wait for it.

```rust
use nominal::core::{CsvIngest, DatasetCreate, Timestamp};

// Create a dataset, and upload the file into it
let (job, dataset_rid) = client
    .ingest()
    .upload_csv("bench.csv", DatasetCreate::new("Bench test"), CsvIngest::new(Timestamp::iso8601("timestamp")))
    .await?;

// Wait for Nominal to ingest the file. This returns an error if the ingest fails.
client.ingest().wait_for_ingest_job(job.rid()).await?;
```

To upload into an existing dataset, pass its RID in place of the {rust:struct}`DatasetCreate`.

## Check an ingest job

{rust:method}`get_ingest_job <IngestClient::get_ingest_job>` checks a job once, without waiting. {rust:method}`wait_for_ingest_job_with_interval <IngestClient::wait_for_ingest_job_with_interval>` waits like `wait_for_ingest_job`, at an interval you choose.

```rust
use std::time::Duration;

use nominal::core::IngestJobStatus;

// Check once, without waiting
let job = client.ingest().get_ingest_job(job.rid()).await?;
match job.status() {
    IngestJobStatus::Completed => println!("ingested"),
    IngestJobStatus::Submitted | IngestJobStatus::Queued | IngestJobStatus::InProgress => println!("still ingesting"),
    other => eprintln!("not ingested: {other:?}"), // Failed, Cancelled, or a status this client doesn't know
}

// Or wait, checking every 10 seconds rather than every 2, such as for a large file
let job = client.ingest().wait_for_ingest_job_with_interval(job.rid(), Duration::from_secs(10)).await?;
```

## File types

Nominal ingests these file types itself, and each has its own upload method and options. [Direct File Ingestion](https://dev.nominal.io/file-ingestion/) describes each format and the options it takes.

| File type | Method | Options |
| --- | --- | --- |
| CSV | {rust:method}`upload_csv <IngestClient::upload_csv>` | {rust:struct}`CsvIngest` |
| Parquet, or a `.tar`, `.tar.gz` or `.zip` of Parquet files | {rust:method}`upload_parquet <IngestClient::upload_parquet>` | {rust:struct}`ParquetIngest` |
| MCAP, with protobuf messages | {rust:method}`upload_mcap <IngestClient::upload_mcap>` | {rust:struct}`McapIngest` |
| journald JSON logs (`.jsonl`, `.jsonl.gz`) | {rust:method}`upload_journal_json <IngestClient::upload_journal_json>` | {rust:struct}`JournalJsonIngest` |
| Avro file from a [stream](stream.md#fallback-file) (`.avro`) | {rust:method}`upload_avro_stream <IngestClient::upload_avro_stream>` | {rust:struct}`AvroStreamIngest` |
| ArduPilot DataFlash (`.bin`) | {rust:method}`upload_ardupilot_dataflash <IngestClient::upload_ardupilot_dataflash>` | {rust:struct}`DataflashIngest` |
| Video (`.mp4`, `.mkv`, `.avi`, `.ts`), or a video in an MCAP file | {rust:method}`upload_video <IngestClient::upload_video>` | {rust:struct}`VideoIngest` |

## CSV and Parquet

Each column except the timestamp column becomes a channel. The {rust:struct}`Timestamp` parameter identifies the timestamp column in the file and the format of the time values.

| Times written as | `Timestamp` |
| --- | --- |
| ISO 8601 | `Timestamp::iso8601("timestamp")` |
| Numbers since the Unix epoch | `Timestamp::epoch("timestamp", TimeUnit::Seconds)` |
| Numbers since the test started | `Timestamp::relative("elapsed", TimeUnit::Milliseconds).with_offset(test_start)` |
| Another format | `Timestamp::custom("timestamp", "<format>")`, in Java `DateTimeFormatter` syntax |

Some legacy formats like IRIG drop information like year or day-of-year. When this happens, set the missing part with {rust:method}`with_default_year <Timestamp::with_default_year>` or {rust:method}`with_default_day_of_year <Timestamp::with_default_day_of_year>`.

Other options allow you to prefix channel names, [tag](https://dev.nominal.io/platform/data-model/organization/tagged-data/) rows, and skip columns:

```rust
use nominal::core::{CsvIngest, TimeUnit, Timestamp};

let ingest = CsvIngest::new(Timestamp::epoch("timestamp", TimeUnit::Seconds))
    .channel_prefix("bench.")                     // voltage becomes bench.voltage
    .tag_column("motor_id", "motor")              // tag each row with motor_id, from the motor column
    .additional_file_tag("test_stand", "stand-2") // tag every row in the file with test_stand=stand-2
    .exclude_column("notes");                     // skip the notes column
client.ingest().upload_csv("bench.csv", "<ri.catalog.instance.dataset.identifier>", ingest).await?;
```

{rust:struct}`ParquetIngest` takes the same options. For an archive of Parquet files, add {rust:method}`is_archive(true) <ParquetIngest::is_archive>`.

## MCAP

Choose an MCAP file's topics with either {rust:method}`include_topic <McapIngest::include_topic>` or {rust:method}`exclude_topic <McapIngest::exclude_topic>`.

```rust
use nominal::core::McapIngest;

let ingest = McapIngest::new()
    .exclude_topic("/diagnostics")
    .ignore_invalid_topics(true); // skip topics Nominal can't read, rather than fail the ingest
client.ingest().upload_mcap("drive-12.mcap", "<ri.catalog.instance.dataset.identifier>", ingest).await?;
```

For the video in an MCAP file, see [Video](#video).

## journald JSON logs

A journald JSON file (`.jsonl` or `.jsonl.gz`) holds one log entry per line, with its time in `__REALTIME_TIMESTAMP` and its text in `MESSAGE`. All of a file's log lines go to one channel, named `logs` unless you name another with {rust:method}`channel <JournalJsonIngest::channel>`.

```rust
use nominal::core::JournalJsonIngest;

// Put the log lines in a channel named system_logs
let ingest = JournalJsonIngest::new().channel("system_logs");
client.ingest().upload_journal_json("system.jsonl", "<ri.catalog.instance.dataset.identifier>", ingest).await?;
```

## Avro streams

An Avro stream file holds data that is already compatible with Nominal and the ingest process has no additional options.

Stream [fallback files](stream.md#fallback-file) use this format.

```rust
use nominal::core::AvroStreamIngest;

// A stream's fallback file, into the dataset the stream writes to
client
    .ingest()
    .upload_avro_stream("fallback_20260101_120000.avro", "<ri.catalog.instance.dataset.identifier>", AvroStreamIngest::new())
    .await?;
```

## ArduPilot DataFlash

Each field of each message in an ArduPilot DataFlash log (`.bin`) becomes a channel named `<message>.<field>`, such as `IMU.GyrX`. {rust:method}`additional_file_tag <DataflashIngest::additional_file_tag>` allows the user to tag every point in the file with additional metadata.

```rust
use nominal::core::DataflashIngest;

// Tag every point with the vehicle that flew
let ingest = DataflashIngest::new().additional_file_tag("vehicle", "QC-456");
client.ingest().upload_ardupilot_dataflash("flight.bin", "<ri.catalog.instance.dataset.identifier>", ingest).await?;
```

## Video

A video goes into a video resource rather than a dataset. Pass a {rust:struct}`VideoCreate` to create one, or an existing video's RID.

```rust
use chrono::{DateTime, Utc};
use nominal::core::{VideoCreate, VideoIngest};

// A standard container format like mp4 only requires a start timestamp
let first_frame: DateTime<Utc> = "2026-01-01T12:00:00Z".parse()?;
client
    .ingest()
    .upload_video("nose-camera.mp4", VideoCreate::new("Nose camera"), VideoIngest::starting_at(first_frame))
    .await?;

// The video on one topic of an MCAP file
client
    .ingest()
    .upload_video("drive-12.mcap", VideoCreate::new("Front camera"), VideoIngest::mcap_topic("/camera/front"))
    .await?;
```

To associate a video with an asset, use {rust:method}`add_video <AssetsClient::add_video>`.

## Upload options and progress

The options of each upload method above, such as {rust:struct}`CsvIngest`, take {rust:struct}`UploadOptions`, which set how the file uploads and report its progress.

```rust
use nominal::core::{CsvIngest, Timestamp, UploadEvent, UploadOptions};

let upload_options = UploadOptions::new()
    .chunk_size(16 * 1024 * 1024) // bytes in each part: 64 MiB by default, and at least 5 MiB
    .max_concurrency(4)           // parts uploaded at once: 8 by default
    .max_retries(5)               // retries for each part: 3 by default
    .on_progress(|event| match event {
        UploadEvent::Started { total_bytes, total_parts } => println!("{total_bytes} bytes in {total_parts} parts"),
        UploadEvent::PartCompleted { part_number, .. } => println!("part {part_number} uploaded"),
        UploadEvent::Completed { .. } => println!("upload complete"),
    });
let ingest = CsvIngest::new(Timestamp::iso8601("timestamp")).upload_options(upload_options);
```

The progress callback runs inside the upload, so keep it quick to avoid slowing down the process.

## Other file types

For other file types, a [containerized extractor](https://dev.nominal.io/extractors/) performs the ingest step. The extractor must be registered in each workspace before it can be used.

From Rust, upload the file and start the extractor's ingest through the REST API, as in the [Ingest with an extractor](../examples/extractor-ingest.md) example.
