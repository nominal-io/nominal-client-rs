# Ingest jobs

{.lead}
Uploading a file starts an ingest job, which reads the file into its dataset. The data is in the dataset once the job completes.

## Wait for a job

```rust
use std::time::Duration;

use nominal::core::{CsvIngest, Timestamp};

let client = nominal::NominalClient::from_profile("default")?;
let (job, _) = client
    .ingest()
    .upload_csv("bench.csv", "<ri.catalog.instance.dataset.identifier>", CsvIngest::new(Timestamp::iso8601("timestamp")))
    .await?;

// Check the job every 2 seconds until it completes. This returns an error if the job fails or is cancelled.
let job = client.ingest().wait_for_ingest_job(job.rid()).await?;

// Or check every 10 seconds, such as for a large file
let job = client.ingest().wait_for_ingest_job_with_interval(job.rid(), Duration::from_secs(10)).await?;
```

## Check a job's status

{rust:method}`get_ingest_job <IngestClient::get_ingest_job>` checks a job once, without waiting.

```rust
use nominal::core::IngestJobStatus;

let job = client.ingest().get_ingest_job(job.rid()).await?;
match job.status() {
    IngestJobStatus::Completed => println!("ingested"),
    IngestJobStatus::Submitted | IngestJobStatus::Queued | IngestJobStatus::InProgress => println!("still ingesting"),
    other => eprintln!("not ingested: {other:?}"), // Failed, Cancelled, or a status this client doesn't know
}
```

## Upload options and progress

Each file type's options take {rust:struct}`UploadOptions`, which set how the file uploads and report its progress.

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

The progress callback runs inside the upload, so keep it quick. Hand slower work to another task.
