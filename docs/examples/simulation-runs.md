# Simulation runs

{.lead}
Upload each execution of a simulation to one dataset for the model, with every execution starting at the same time zero and tagged with its execution ID. Then create a run for each execution that selects its tagged points, so runs overlay with no time shifting. The [simulations approach](https://dev.nominal.io/platform/data-model/organization/simulation/) describes this way of organizing data.

```shell
cargo add nominal anyhow
cargo add tokio --features full
```

```rust
use std::sync::Arc;

use anyhow::Context;
use nominal::core::{CsvIngest, DatasetCreate, TimeUnit, Timestamp};
use nominal::streaming::api::clients::scout::{AsyncRunService, AsyncRunServiceClient};
use nominal::streaming::api::objects::api::{TagName, TagValue, rids::DatasetRid};
use nominal::streaming::api::objects::scout::api::DataSourceRefName;
use nominal::streaming::api::objects::scout::run::api::{CreateRunDataSource, CreateRunRequest, DataSource, UtcTimestamp};
use nominal::streaming::client::async_conjure_client;
use nominal::streaming::client::conjure::http::client::{AsyncService, ConjureRuntime};
use nominal::streaming::client::conjure::object::SafeLong;
use nominal::streaming::prelude::{BearerToken, ResourceIdentifier};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = nominal::NominalClient::from_profile("default")?;
    let config = nominal::Config::load()?;
    let profile = config.get_profile("default").context("no profile named default")?;
    let token = BearerToken::new(profile.token())?;
    let api = async_conjure_client("runs", profile.base_url().parse()?).map_err(nominal::Error::from)?;
    let runs = AsyncRunServiceClient::new(api, &Arc::new(ConjureRuntime::default()));

    // One dataset for the model; every execution starts at the same time zero
    let dataset = client.catalog().create_dataset(DatasetCreate::new("Model A")).await?;
    let t0 = chrono::DateTime::UNIX_EPOCH;

    for (execution, gain) in [(1, 0.5), (2, 1.0), (3, 2.0)] {
        // 60 seconds of simulated output, in seconds since the execution started
        let mut csv = String::from("t,altitude\n");
        for i in 0..=600 {
            let t = i as f64 / 10.0;
            csv += &format!("{t:.1},{:.2}\n", 1000.0 * (1.0 - (-gain * t / 30.0).exp()));
        }
        let path = format!("execution-{execution}.csv");
        std::fs::write(&path, csv)?;

        // Tag each execution's points, so a run can select them
        let ingest = CsvIngest::new(Timestamp::relative("t", TimeUnit::Seconds).with_offset(t0))
            .additional_file_tag("model", "A")
            .additional_file_tag("execution-id", execution.to_string());
        let (job, _) = client.ingest().upload_csv(&path, dataset.rid(), ingest).await?;
        client.ingest().wait_for_ingest_job(job.rid()).await?;

        // A run for the execution: the dataset, filtered to the execution's tags
        let source = CreateRunDataSource::builder()
            .data_source(DataSource::Dataset(DatasetRid(ResourceIdentifier::new(dataset.rid())?)))
            .insert_series_tags(TagName("model".into()), TagValue("A".into()))
            .insert_series_tags(TagName("execution-id".into()), TagValue(execution.to_string()))
            .build();
        let request = CreateRunRequest::builder()
            .title(format!("Model A, execution {execution}"))
            .description(format!("gain {gain}"))
            .start_time(UtcTimestamp::new(SafeLong::new(t0.timestamp())?))
            .end_time(UtcTimestamp::new(SafeLong::new(t0.timestamp() + 60)?))
            .insert_data_sources(DataSourceRefName("default".into()), source)
            .build();
        runs.create_run(&token, &request).await.map_err(nominal::Error::from)?;
    }
    Ok(())
}
```

The `nominal` crate's `RunCreate` can't filter a dataset by tags, so the runs are created through the REST API, as in [Call the REST API directly](../guides/rest-api.md). Each run's `altitude` channel covers the same 60 seconds from 1970-01-01T00:00:00Z.
