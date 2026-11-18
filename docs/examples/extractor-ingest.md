# Ingest with an extractor

{.lead}
Upload a file that a [containerized extractor](https://dev.nominal.io/extractors/) reads, and ingest it into a new dataset, through the REST API. Like every file, it goes through two steps: an [upload](https://dev.nominal.io/api/rest/upload-service/#upload-file), then an [ingest](https://dev.nominal.io/api/rest/ingest-service/#ingest).

Replace the file name, the extractor's RID and its input's environment variable with your own. The extractor must be registered in the dataset's workspace, which here is your profile's.

```shell
cargo add nominal serde_json
cargo add reqwest --features json
cargo add tokio --features full
```

```rust
use nominal::core::DatasetCreate;
use serde_json::{Value, json};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load the API key, base URL and workspace that API Keys Setup saved
    let config = nominal::Config::load()?;
    let profile = config.get_profile("default").ok_or("no profile named default")?;
    let client = nominal::NominalClient::from_profile_config(profile)?;
    let workspace = profile.workspace_rid().ok_or("no workspace in the profile")?;
    let http = reqwest::Client::new();

    // Step 1: upload the file to Nominal. This endpoint suits smaller files.
    let s3_path: String = http
        .post(format!("{}/upload/v1/upload-file", profile.base_url()))
        .bearer_auth(profile.token())
        .query(&[("fileName", "flight-12.tdms"), ("workspace", workspace)])
        .header("Content-Type", "application/octet-stream")
        .body(std::fs::read("flight-12.tdms")?)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    // Step 2: ingest the uploaded file into a new dataset, with the extractor.
    // File types Nominal ingests itself, such as CSV and MCAP, take their own items and options:
    // see https://dev.nominal.io/file-ingestion/
    let dataset = client.catalog().create_dataset(DatasetCreate::new("Flight 12")).await?;
    let response: Value = http
        .post(format!("{}/ingest/v2", profile.base_url()))
        .bearer_auth(profile.token())
        .json(&json!({
            "datasetRid": dataset.rid(),
            "items": [{
                "containerized": {
                    "extractorRid": "<ri.scout.instance.containerized-extractor.identifier>",
                    // The uploaded file, under the environment variable of the extractor's input
                    "sources": { "INPUT_FILE": { "s3": { "path": s3_path } } }
                }
            }]
        }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    // Wait for the extractor to run and its output to ingest
    let job_rid = response["ingestJobRid"].as_str().ok_or("no ingest job")?;
    client.ingest().wait_for_ingest_job(job_rid).await?;
    println!("{}", dataset.nominal_url());
    Ok(())
}
```

For the extractor's arguments, tags and timestamp overrides, see [Run an ingest](https://dev.nominal.io/extractors/ingest/#rest-api).
