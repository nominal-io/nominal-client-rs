# Upload files and organize them

{.lead}
Upload files into datasets, then group them: an asset for the hardware you test, and a run for each test.

## Upload a file

`upload_csv` uploads a CSV file into a dataset, and returns the ingest job and the dataset's RID. Give it an existing dataset's RID, or a `DatasetCreate` to create the dataset with the upload:

```rust
use nominal::core::{CsvIngest, DatasetCreate, Timestamp};

let client = nominal::NominalClient::from_profile("default")?;
let (job, dataset_rid) = client
    .ingest()
    .upload_csv("test-1.csv", DatasetCreate::new("Bench test 1"), CsvIngest::new(Timestamp::iso8601("timestamp")))
    .await?;
// Returns an error if the ingest fails or is cancelled
client.ingest().wait_for_ingest_job(job.rid()).await?;
```

`CsvIngest` also sets how the file's columns become channels:

```rust
let ingest = CsvIngest::new(Timestamp::iso8601("timestamp"))
    .channel_prefix("bench.")                   // bench.voltage, bench.current
    .tag_column("unit", "serial_number")        // tag each point with its row's serial number
    .additional_file_tag("stand", "stand-2")    // tag every point in the file
    .exclude_column("notes");
```

The ingest client has an upload function for each file type it reads: `upload_csv`, `upload_parquet`, `upload_mcap`, `upload_journal_json`, `upload_avro_stream`, `upload_ardupilot_dataflash` and `upload_video`.

## Add the dataset to an asset and a run

An asset is the hardware you test, and a run is the time range of one test. Add the dataset to both, so you can find its data from either:

```rust
use nominal::core::{AssetCreate, RunCreate};

let asset = client.assets().create(AssetCreate::new("Bench unit 7")).await?;
client.assets().add_dataset(asset.rid(), "bench", &dataset_rid).await?;

let start: chrono::DateTime<chrono::Utc> = "2026-01-01T14:00:00Z".parse()?;
let end: chrono::DateTime<chrono::Utc> = "2026-01-01T14:30:00Z".parse()?;
let run = client
    .runs()
    .create(RunCreate::new("Bench test 1", start).end(end).assets([asset.rid()]))
    .await?;
client.runs().add_dataset(run.rid(), "bench", &dataset_rid).await?;
```

The second argument of `add_dataset` names the dataset within the asset or run. The run's start and end come from the [chrono](https://docs.rs/chrono) crate.
