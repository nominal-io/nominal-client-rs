# Check a test against its limits

{.lead}
Upload a test's data, create a run over it, create a checklist with a limit check, and run the checklist on the run. The program prints how many violations the check found. [Automated pass/fail checks](https://dev.nominal.io/platform/data-model/paradigms/stories/automated-checks/) describes when to check data this way.

```shell
cargo add nominal anyhow chrono
cargo add tokio --features full
```

The `nominal` crate creates the dataset and the run. Creating and running the checklist goes through the REST API, as in [Call the REST API directly](../guides/rest-api.md). The REST API reference describes the requests' fields on the [checklist service](https://dev.nominal.io/api/rest/checklist-service/) and [data review service](https://dev.nominal.io/api/rest/data-review-service/) pages.

```rust
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use chrono::Utc;
use nominal::core::{CsvIngest, DatasetCreate, RunCreate, Timestamp};
use nominal::streaming::api::clients::event::{AsyncEventService, AsyncEventServiceClient};
use nominal::streaming::api::clients::scout::checks::api::{AsyncChecklistService, AsyncChecklistServiceClient};
use nominal::streaming::api::clients::scout::datareview::api::{AsyncDataReviewService, AsyncDataReviewServiceClient};
use nominal::streaming::api::objects::api::Channel;
use nominal::streaming::api::objects::event::EventType;
use nominal::streaming::api::objects::scout::api::{ChannelLocator, DataSourceRefName, Priority};
use nominal::streaming::api::objects::scout::checks::api::{
    CreateCheckRequest, CreateChecklistEntryRequest, CreateChecklistRequest, UnresolvedCheckCondition,
    UnresolvedNumRangesConditionV3, UnresolvedVariableLocator,
};
use nominal::streaming::api::objects::scout::compute::api::{
    DoubleConstant, NumericSeries, RangeSeries, Reference, ThresholdOperator, ThresholdingRanges, VariableName,
};
use nominal::streaming::api::objects::scout::datareview::api::{
    AutomaticCheckEvaluationState as State, BatchInitiateDataReviewRequest, CreateDataReviewRequest,
};
use nominal::streaming::api::objects::scout::rids::api::UserRid;
use nominal::streaming::api::objects::scout::run::api::RunRid;
use nominal::streaming::client::async_conjure_client;
use nominal::streaming::client::conjure::http::client::{AsyncService, ConjureRuntime};
use nominal::streaming::prelude::{BearerToken, ResourceIdentifier};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = nominal::NominalClient::from_profile("default")?;
    let config = nominal::Config::load()?;
    let profile = config.get_profile("default").context("no profile named default")?;
    let token = BearerToken::new(profile.token())?;
    let api = async_conjure_client("checks", profile.base_url().parse()?).map_err(nominal::Error::from)?;
    let runtime = Arc::new(ConjureRuntime::default());

    // 1. A minute of temperature data with one excursion above 80, in a new dataset
    let start = Utc::now() - chrono::Duration::minutes(1);
    let mut csv = String::from("timestamp,temperature\n");
    for i in 0..60 {
        let temperature = if (30..35).contains(&i) { 85.0 } else { 70.0 };
        csv += &format!("{},{temperature}\n", (start + chrono::Duration::seconds(i)).to_rfc3339());
    }
    std::fs::write("bench.csv", csv)?;
    let (job, dataset_rid) = client
        .ingest()
        .upload_csv("bench.csv", DatasetCreate::new("Bench test"), CsvIngest::new(Timestamp::iso8601("timestamp")))
        .await?;
    client.ingest().wait_for_ingest_job(job.rid()).await?;

    // 2. A run over the test, with the dataset under the ref name "default"
    let run = client.runs().create(RunCreate::new("Bench test 1", start).end(Utc::now())).await?;
    client.runs().add_dataset(run.rid(), "default", &dataset_rid).await?;

    // 3. A checklist with one check: temperature, under the ref name "default", above 80
    let condition = UnresolvedNumRangesConditionV3::builder()
        .ranges(RangeSeries::Threshold(ThresholdingRanges::new(
            NumericSeries::Raw(Reference::new(VariableName("temperature".into()))),
            DoubleConstant::Literal(80.0),
            ThresholdOperator::GreaterThan,
        )))
        .function_spec(BTreeMap::<String, String>::new())
        .threshold(0) // violated if there are more than 0 ranges
        .operator(ThresholdOperator::GreaterThan)
        .insert_variables(
            VariableName("temperature".into()),
            UnresolvedVariableLocator::Series(ChannelLocator::new(
                DataSourceRefName("default".into()),
                Channel("temperature".into()),
            )),
        )
        .build();
    let me = client.users().who_am_i().await?;
    let request = CreateChecklistRequest::builder()
        .commit_message("Create checklist")
        .assignee_rid(UserRid(ResourceIdentifier::new(me.rid())?))
        .title("Bench temperature limits")
        .description("")
        .push_checks(CreateChecklistEntryRequest::CreateCheck(
            CreateCheckRequest::builder()
                .title("Temperature above 80")
                .description("")
                .priority(Priority::P2)
                .condition(UnresolvedCheckCondition::NumRangesV3(condition))
                .build(),
        ))
        .build();
    let checklist = AsyncChecklistServiceClient::new(api.clone(), &runtime)
        .create(&token, &request)
        .await
        .map_err(nominal::Error::from)?;

    // 4. Run the checklist on the run, and wait until no check is pending or executing
    let reviews = AsyncDataReviewServiceClient::new(api.clone(), &runtime);
    let request = BatchInitiateDataReviewRequest::builder()
        .push_requests(CreateDataReviewRequest::new(
            RunRid(ResourceIdentifier::new(run.rid())?),
            checklist.rid().clone(),
        ))
        .build();
    let review_rid = reviews.batch_initiate(&token, &request).await.map_err(nominal::Error::from)?.rids()[0].clone();
    let review = loop {
        let review = reviews.get(&token, &review_rid).await.map_err(nominal::Error::from)?;
        let running = review
            .check_evaluations()
            .iter()
            .any(|c| matches!(c.state(), State::PendingExecution(_) | State::Executing(_)));
        if !running {
            break review;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    };

    // 5. Count the events the check created
    let event_rids: BTreeSet<_> = review
        .check_evaluations()
        .iter()
        .filter_map(|c| match c.state() {
            State::GeneratedAlerts(s) => Some(s.event_rids().clone()),
            _ => None,
        })
        .flatten()
        .collect();
    let events = AsyncEventServiceClient::new(api, &runtime)
        .batch_get_events(&token, &event_rids)
        .await
        .map_err(nominal::Error::from)?;
    let violations = events.iter().filter(|e| *e.type_() == EventType::Error).count();
    println!("{violations} violations: {}", run.nominal_url());
    Ok(())
}
```

Open the printed run in Nominal to see the check's events over the excursion.
