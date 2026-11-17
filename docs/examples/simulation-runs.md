# Simulation runs

{.lead}
Upload every execution of a simulation model to one dataset, tag each execution's points with its ID, and create a [run](https://dev.nominal.io/platform/data-model/#runs) for each execution that selects its points, as in [Simulations](https://dev.nominal.io/platform/data-model/organization/simulation/). Every execution starts at the same time zero, so the runs overlay.

Replace the CSV rows with your simulator's output. The program creates the runs through the [REST API](../guides/rest-api.md), which can filter a run's dataset by tag.

```shell
cargo add nominal chrono
cargo add tokio --features full
```

```rust
use std::sync::Arc;

use nominal::core::{CsvIngest, DatasetCreate, TimeUnit, Timestamp};
use nominal::streaming::api::clients::scout::{AsyncRunService, AsyncRunServiceClient};
use nominal::streaming::api::objects::api::rids::{DatasetRid, WorkspaceRid};
use nominal::streaming::api::objects::api::{TagName, TagValue};
use nominal::streaming::api::objects::scout::api::DataSourceRefName;
use nominal::streaming::api::objects::scout::run::api::{CreateRunDataSource, CreateRunRequest, DataSource, UtcTimestamp};
use nominal::streaming::client::async_conjure_client;
use nominal::streaming::client::conjure::http::client::{AsyncService, ConjureRuntime};
use nominal::streaming::client::conjure::object::SafeLong;
use nominal::streaming::prelude::{BearerToken, ResourceIdentifier};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load the API key and base URL that API Keys Setup saved
    let config = nominal::Config::load()?;
    let profile = config.get_profile("default").ok_or("no profile named default")?;
    let client = nominal::NominalClient::from_profile_config(profile)?;

    // The REST API's run service. Its errors convert to nominal::Error.
    let token = BearerToken::new(profile.token())?;
    let api = async_conjure_client("runs", profile.base_url().parse()?).map_err(nominal::Error::from)?;
    let runs = AsyncRunServiceClient::new(api, &Arc::new(ConjureRuntime::default()));

    // Each run must be in the dataset's workspace, the profile's
    let workspace = WorkspaceRid(ResourceIdentifier::new(profile.workspace_rid().ok_or("no workspace in profile")?)?);

    // One dataset for the model, and one time zero for every execution
    let dataset = client.catalog().create_dataset(DatasetCreate::new("Model A")).await?;
    let t0 = chrono::DateTime::UNIX_EPOCH;

    // Stands in for your simulator: each execution's altitude in meters, by seconds since it started
    let outputs = [
        ("1", ["t,altitude", "0,0", "30,620", "60,860"]),
        ("2", ["t,altitude", "0,0", "30,860", "60,980"]),
        ("3", ["t,altitude", "0,0", "30,950", "60,1000"]),
    ];

    for (execution, output) in outputs {
        let path = format!("execution-{execution}.csv");
        std::fs::write(&path, output.join("\n"))?;

        // Upload the output from time zero, with every point tagged with the execution's ID
        let ingest = CsvIngest::new(Timestamp::relative("t", TimeUnit::Seconds).with_offset(t0))
            .additional_file_tag("execution-id", execution);
        let (job, _) = client.ingest().upload_csv(&path, dataset.rid(), ingest).await?;
        client.ingest().wait_for_ingest_job(job.rid()).await?;

        // A run for the execution, with the dataset under the ref name "default", filtered to the execution's tag
        let source = CreateRunDataSource::builder()
            .data_source(DataSource::Dataset(DatasetRid(ResourceIdentifier::new(dataset.rid())?)))
            .insert_series_tags(TagName("execution-id".into()), TagValue(execution.into()))
            .build();
        let request = CreateRunRequest::builder()
            .title(format!("Model A, execution {execution}"))
            .description("")
            .start_time(UtcTimestamp::new(SafeLong::new(t0.timestamp())?))
            .end_time(UtcTimestamp::new(SafeLong::new(t0.timestamp() + 60)?)) // 60 seconds after time zero
            .insert_data_sources(DataSourceRefName("default".into()), source)
            .workspace(workspace.clone())
            .build();
        runs.create_run(&token, &request).await.map_err(nominal::Error::from)?;
    }
    Ok(())
}
```

To compare the executions, open one run in a workbook and add the others, as in [Compare executions](https://dev.nominal.io/platform/data-model/paradigms/stories/simulation-first/#compare-executions).
