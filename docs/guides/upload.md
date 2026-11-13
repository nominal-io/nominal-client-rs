# Upload files

{.lead}
Upload a file into a dataset, then wait for Nominal to ingest it. Each file type has its own upload function and options.

## Upload a file

Each upload function takes the file's path, the dataset to ingest into, and the file type's options. It returns the ingest job and the dataset's RID. The dataset can be an existing one's RID, or a `DatasetCreate` to create it with the upload: if the ingest fails, no dataset is left behind.

```rust
use nominal::core::{CsvIngest, DatasetCreate, Timestamp};

let (job, dataset_rid) = client
    .ingest()
    .upload_csv("flight.csv", DatasetCreate::new("Flight telemetry"), CsvIngest::new(Timestamp::iso8601("time")))
    .await?;
client.ingest().wait_for_ingest_job(job.rid()).await?; // errors if the ingest fails
```

Uploading another file to the same dataset adds to its data rather than replacing it. Keep channel names, types and units the same across files.

| File type | Function | Options |
| --- | --- | --- |
| CSV | `upload_csv` | `CsvIngest` |
| Parquet, or an archive of Parquet files | `upload_parquet` | `ParquetIngest` |
| MCAP (protobuf time series) | `upload_mcap` | `McapIngest` |
| Journal JSON logs (`.jsonl`, `.jsonl.gz`) | `upload_journal_json` | `JournalJsonIngest` |
| Nominal Avro stream (`.avro`) | `upload_avro_stream` | `AvroStreamIngest` |
| ArduPilot DataFlash (`.bin`) | `upload_ardupilot_dataflash` | `DataflashIngest` |
| Video, or a video stream in an MCAP file | `upload_video` | `VideoIngest` |

## CSV and Parquet

A CSV or Parquet file has one timestamp column. Every other column becomes a channel. `Timestamp` says which column holds the timestamps and how they're written:

| Timestamps | `Timestamp` |
| --- | --- |
| ISO 8601 strings | `Timestamp::iso8601("time")` |
| Numbers since the Unix epoch | `Timestamp::epoch("time", TimeUnit::Seconds)` |
| Numbers since a start time | `Timestamp::relative("elapsed", TimeUnit::Milliseconds).with_offset(start)` |
| Another format | `Timestamp::custom("time", "<format>")`, in Java `DateTimeFormatter` syntax |

When a custom format has no year or date, such as IRIG time, set it with `with_default_year` or `with_default_day_of_year`.

The options also rename, tag and skip columns:

```rust
use nominal::core::{CsvIngest, TimeUnit, Timestamp};

let ingest = CsvIngest::new(Timestamp::epoch("time", TimeUnit::Seconds))
    .channel_prefix("bench.")                // bench.voltage, bench.current
    .tag_column("unit", "serial_number")     // tag each point with its row's serial number
    .additional_file_tag("stand", "stand-2") // tag every point in the file
    .exclude_column("notes");
let (job, _) = client.ingest().upload_csv("bench.csv", "<ri.catalog.instance.dataset.identifier>", ingest).await?;
```

`ParquetIngest` has the same options. With `is_archive(true)`, it ingests each Parquet file in a `.tar`, `.tar.gz` or `.zip` file.

## MCAP

`upload_mcap` ingests an MCAP file's protobuf time series. Choose topics with `include_topic` or `exclude_topic`, not both:

```rust
use nominal::core::McapIngest;

let ingest = McapIngest::new().exclude_topic("/diagnostics").ignore_invalid_topics(true);
let (job, _) = client.ingest().upload_mcap("recording.mcap", "<ri.catalog.instance.dataset.identifier>", ingest).await?;
```

With `ignore_invalid_topics(true)`, topics Nominal can't read are skipped instead of failing the ingest. To ingest an MCAP file's video, use [`upload_video`](#video).

## Logs, Avro streams and DataFlash

```rust
use nominal::core::{AvroStreamIngest, DataflashIngest, JournalJsonIngest};

let dataset = "<ri.catalog.instance.dataset.identifier>";
client.ingest().upload_journal_json("system.jsonl", dataset, JournalJsonIngest::new().channel("logs")).await?;
client.ingest().upload_avro_stream("fallback.avro", dataset, AvroStreamIngest::new()).await?;
client.ingest().upload_ardupilot_dataflash("flight.bin", dataset, DataflashIngest::new()).await?;
```

Journal JSON lines go to one log channel, `logs` unless you name another. Avro stream files are what a [stream's fallback file](stream.md#keep-a-copy-on-disk) writes.

## Video

Videos go into a video, not a dataset. Give the time of the first frame for a video file, or the topic for a video stream in an MCAP file:

```rust
use chrono::{TimeZone, Utc};
use nominal::core::{VideoCreate, VideoIngest};

let start = Utc.with_ymd_and_hms(2026, 9, 10, 14, 38, 19).unwrap();
let (job, video_rid) = client
    .ingest()
    .upload_video("nose_camera.mp4", VideoCreate::new("Nose camera"), VideoIngest::starting_at(start))
    .await?;

let (job, _) = client
    .ingest()
    .upload_video("recording.mcap", VideoCreate::new("Front camera"), VideoIngest::mcap_topic("/camera/front"))
    .await?;
```

Video files can be `.mp4`, `.mkv`, `.avi` or `.ts`. Each MCAP upload takes one video topic. To show a video with an asset or run, add it with `add_video`, as in [Assets](assets.md#add-data-with-data-scopes).

## Ingest jobs and progress

To check a job's status, wait at another interval, or follow a large file's upload, see [Ingest jobs](ingest-jobs.md).
