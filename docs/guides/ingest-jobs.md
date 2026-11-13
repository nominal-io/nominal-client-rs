# Ingest jobs

{.lead}
Uploading a file starts an ingest job, which reads the file into Nominal. Wait for the job before you use the data, and check its status when an ingest fails.

## What an upload returns

Each `upload_*` function returns the ingest job, and the RID of the dataset the data goes into. `upload_video` returns the video's RID instead.

```rust
use nominal::core::{CsvIngest, DatasetCreate, Timestamp};

let (job, dataset_rid) = client
    .ingest()
    .upload_csv("bench.csv", DatasetCreate::new("Bench test"), CsvIngest::new(Timestamp::iso8601("time")))
    .await?;
```

The upload returns once the file is uploaded and the job is created. The data isn't in the dataset until the job completes.

## Wait for a job

`wait_for_ingest_job` checks the job every 2 seconds, and returns the job once it completes:

```rust
let job = client.ingest().wait_for_ingest_job(job.rid()).await?;
```

It returns an error if the job fails or is cancelled, or reports a status this client doesn't recognize. To check at another interval, such as for a large file:

```rust
let job = client
    .ingest()
    .wait_for_ingest_job_with_interval(job.rid(), std::time::Duration::from_secs(10))
    .await?;
```

## Check a job's status

`get_ingest_job` checks a job once:

```rust
use nominal::core::IngestJobStatus;

let job = client.ingest().get_ingest_job(job.rid()).await?;
match job.status() {
    IngestJobStatus::Completed => println!("ingested"),
    IngestJobStatus::Failed | IngestJobStatus::Cancelled => eprintln!("not ingested: {:?}", job.origin_files()),
    status if !status.is_terminal() => println!("still running: {status:?}"),
    other => eprintln!("unrecognized status: {other:?}"),
}
```

A job moves through `Submitted`, `Queued` and `InProgress`, and ends as `Completed`, `Failed` or `Cancelled`. `Unknown` holds a status this client doesn't recognize. `origin_files` lists the files the job is ingesting, when the server reports them, and `ingest_type` the kind of data, such as `Tabular` or `Mcap`.

## Upload options and progress

Files upload in parts of 64 MiB, eight at a time, and each part is retried up to 3 times. `UploadOptions` changes these and reports progress. Every file type's options take it, through `upload_options`:

```rust
use nominal::core::{CsvIngest, Timestamp, UploadEvent, UploadOptions};

let options = UploadOptions::new()
    .chunk_size(16 * 1024 * 1024) // at least 5 MiB
    .max_concurrency(4)
    .max_retries(5)
    .on_progress(|event| match event {
        UploadEvent::Started { total_bytes, total_parts } => println!("{total_bytes} bytes in {total_parts} parts"),
        UploadEvent::PartCompleted { part_number, .. } => println!("part {part_number} uploaded"),
        UploadEvent::Completed { .. } => println!("upload complete"),
    });
let ingest = CsvIngest::new(Timestamp::iso8601("time")).upload_options(options);
```

The callback runs on the upload's tasks, so keep it short: send events to a channel for anything slower.
