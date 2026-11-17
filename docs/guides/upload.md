# Upload files

{.lead}
Upload a file into a dataset, and Nominal ingests its data into [channels](https://dev.nominal.io/platform/data-model/#channels). Each file type has its own upload method and options.

## Upload a file

```rust
use nominal::core::{CsvIngest, DatasetCreate, Timestamp};

let client = nominal::NominalClient::from_profile("default")?;

// Create a dataset, and upload the file into it
let (job, dataset_rid) = client
    .ingest()
    .upload_csv("bench.csv", DatasetCreate::new("Bench test"), CsvIngest::new(Timestamp::iso8601("timestamp")))
    .await?;

// Wait for Nominal to ingest the file. This returns an error if the ingest fails.
client.ingest().wait_for_ingest_job(job.rid()).await?;
```

To upload into an existing dataset, pass its RID in place of the {rust:struct}`DatasetCreate`. To check an ingest's status or follow a large file's upload, see [Ingest jobs](ingest-jobs.md).

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

Each column except the timestamp column becomes a channel. {rust:struct}`Timestamp` names the timestamp column, and says how its times are written:

| Times written as | `Timestamp` |
| --- | --- |
| ISO 8601 | `Timestamp::iso8601("timestamp")` |
| Numbers since the Unix epoch | `Timestamp::epoch("timestamp", TimeUnit::Seconds)` |
| Numbers since the test started | `Timestamp::relative("elapsed", TimeUnit::Milliseconds).with_offset(test_start)` |
| Another format | `Timestamp::custom("timestamp", "<format>")`, in Java `DateTimeFormatter` syntax |

When a custom format leaves out the year or the date, such as IRIG time, set the missing part with {rust:method}`with_default_year <Timestamp::with_default_year>` or {rust:method}`with_default_day_of_year <Timestamp::with_default_day_of_year>`.

The options also prefix channel names, [tag](https://dev.nominal.io/platform/data-model/organization/tagged-data/) rows, and skip columns:

```rust
use nominal::core::{CsvIngest, TimeUnit, Timestamp};

let ingest = CsvIngest::new(Timestamp::epoch("timestamp", TimeUnit::Seconds))
    .channel_prefix("bench.")                     // voltage becomes bench.voltage
    .tag_column("motor_id", "motor")              // tag each row with motor_id, from the motor column
    .additional_file_tag("test_stand", "stand-2") // tag every row in the file
    .exclude_column("notes");                     // skip the notes column
client.ingest().upload_csv("bench.csv", "<ri.catalog.instance.dataset.identifier>", ingest).await?;
```

{rust:struct}`ParquetIngest` takes the same options. For an archive of Parquet files, add {rust:method}`is_archive(true) <ParquetIngest::is_archive>`.

## MCAP

Choose an MCAP file's topics with {rust:method}`include_topic <McapIngest::include_topic>` or {rust:method}`exclude_topic <McapIngest::exclude_topic>`, not both.

```rust
use nominal::core::McapIngest;

let ingest = McapIngest::new()
    .exclude_topic("/diagnostics")
    .ignore_invalid_topics(true); // skip topics Nominal can't read, rather than fail the ingest
client.ingest().upload_mcap("drive-12.mcap", "<ri.catalog.instance.dataset.identifier>", ingest).await?;
```

For the video in an MCAP file, see [Video](#video).

## Logs, Avro streams and DataFlash

A journald JSON file's log lines go to one channel, named `logs` unless you name another.

```rust
use nominal::core::{AvroStreamIngest, DataflashIngest, JournalJsonIngest};

let dataset_rid = "<ri.catalog.instance.dataset.identifier>";
client.ingest().upload_journal_json("system.jsonl", dataset_rid, JournalJsonIngest::new().channel("system_logs")).await?;
client.ingest().upload_avro_stream("fallback.avro", dataset_rid, AvroStreamIngest::new()).await?;
client.ingest().upload_ardupilot_dataflash("flight.bin", dataset_rid, DataflashIngest::new()).await?;
```

## Video

A video goes into a video resource rather than a dataset. Pass a {rust:struct}`VideoCreate` to create one, or an existing video's RID.

```rust
use chrono::{DateTime, Utc};
use nominal::core::{VideoCreate, VideoIngest};

// A video file, from the time of its first frame
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

To show a video with an asset or run, add it with {rust:method}`add_video <AssetsClient::add_video>`, as in [Assets](assets.md#add-data-with-data-scopes).
