# Read channel data

{.lead}
Upload a test's data, then read two of its channels back into memory as CSV, through the REST API's data export service. [Query data with SQL](../guides/sql.md) reads the same data with a SQL query.

Replace the CSV rows, the channel names and the time range with your own. The program uses the [REST API](../guides/rest-api.md) clients that come with the `nominal` crate.

```shell
cargo add nominal futures
cargo add tokio --features full
```

```rust
use std::sync::Arc;

use futures::TryStreamExt;
use nominal::core::{CsvIngest, DatasetCreate, Timestamp};
use nominal::streaming::api::clients::scout::dataexport::api::{AsyncDataExportService, AsyncDataExportServiceClient};
use nominal::streaming::api::objects::api::Timestamp as ExportTimestamp;
use nominal::streaming::api::objects::scout::compute::api::{
    Context, Dataset, NumericSeries, SavedDataset, SelectSeries, Series, StringConstant,
};
use nominal::streaming::api::objects::scout::dataexport::api::{
    Csv, ExportChannels, ExportDataRequest, ExportFormat, ExportTimeDomainChannels, Iso8601TimestampFormat,
    MergeTimestampStrategy, NoneStrategy, ResolutionOption, TimeDomainChannel, TimestampFormat, UndecimatedResolution,
};
use nominal::streaming::client::async_conjure_client;
use nominal::streaming::client::conjure::http::client::{AsyncService, ConjureRuntime};
use nominal::streaming::client::conjure::object::SafeLong;
use nominal::streaming::prelude::{BearerToken, ProductAgent};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load the API key and base URL that API Keys Setup saved
    let config = nominal::Config::load()?;
    let profile = config.get_profile("default").ok_or("no profile named default")?;
    let client = nominal::NominalClient::from_profile_config(profile)?;

    // 1. Generate sample data -- if data already exists in nominal, this can be skipped
    // two channels, one reading a second
    let csv = [
        "timestamp,temperature,pressure",
        "2026-01-01T12:00:00Z,70.2,101.3",
        "2026-01-01T12:00:01Z,70.4,101.2",
        "2026-01-01T12:00:02Z,70.9,101.2",
    ];
    std::fs::write("bench-test.csv", csv.join("\n"))?;
    let (job, dataset_rid) = client
        .ingest()
        .upload_csv("bench-test.csv", DatasetCreate::new("Bench test"), CsvIngest::new(Timestamp::iso8601("timestamp")))
        .await?;
    client.ingest().wait_for_ingest_job(job.rid()).await?;



    // 2. Query that data
    // Generate a client for the REST API's data export service
    let token = BearerToken::new(profile.token())?;
    let api = async_conjure_client("export", profile.base_url().parse()?, &ProductAgent::default())
        .map_err(nominal::Error::from)?;
    let export = AsyncDataExportServiceClient::new(api, &Arc::new(ConjureRuntime::default()));

    // this closure takes a channel name and creates an object which combines the dataset RID and name
    // it is called below to define the desired data channels
    let column = |channel: &str| {
        TimeDomainChannel::new(
            channel,
            Series::Numeric(Box::new(NumericSeries::SelectNumeric(
                SelectSeries::builder()
                    .name(StringConstant::Literal(channel.into()))
                    .dataset(Dataset::Saved(SavedDataset::new(StringConstant::Literal(dataset_rid.clone()))))
                    .build(),
            ))),
        )
    };

    // Define the data to query:
    // the timestamp in ISO 8601
    // then column object for each channel in our data
    let columns = ExportTimeDomainChannels::builder()
        .output_timestamp_format(TimestampFormat::Iso8601(Iso8601TimestampFormat::new()))
        // A channel's cell is empty at a timestamp where it has no point
        .merge_timestamp_strategy(MergeTimestampStrategy::None(NoneStrategy::new()))
        .push_channels(column("temperature"))
        .push_channels(column("pressure"))
        .build();

    // Create a request asking for csv data
    // from the first timestamp of the entered data to one minute later,
    // with the original time resolution,
    // for the columns defined above
    let request = ExportDataRequest::builder()
        .format(ExportFormat::Csv(Csv::new()))
        .start_time(ExportTimestamp::new(SafeLong::new(1_767_268_800)?, SafeLong::new(0)?)) // seconds since the Unix epoch
        .end_time(ExportTimestamp::new(SafeLong::new(1_767_268_860)?, SafeLong::new(0)?))
        .resolution(ResolutionOption::Undecimated(UndecimatedResolution::new()))
        .channels(ExportChannels::TimeDomain(columns))
        .context(Context::builder().build())
        .build();

    // Read the response into memory as it arrives
    let body = export.export_channel_data(&token, &request).await.map_err(nominal::Error::from)?;
    let csv = body.map_ok(|chunk| chunk.to_vec()).try_concat().await.map_err(nominal::Error::from)?;
    print!("{}", String::from_utf8(csv)?);
    Ok(())
}
```

The program prints:

```text
timestamp,temperature,pressure
2026-01-01T12:00:00Z,70.2,101.3
2026-01-01T12:00:01Z,70.4,101.2
2026-01-01T12:00:02Z,70.9,101.2
```
