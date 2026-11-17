# Check a test against its limits

{.lead}
Upload a test's data, create a [run](https://dev.nominal.io/platform/data-model/#runs) over it, and execute a checklist with one temperature check on the run, as in [Automated pass/fail checks](https://dev.nominal.io/platform/data-model/paradigms/stories/automated-checks/). The program prints how many violations the check found.

Replace the CSV rows and the limit with your own. The program creates and executes the checklist through the [REST API](../guides/rest-api.md).

```shell
cargo add nominal chrono
cargo add tokio --features full
```

```rust
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use nominal::core::{CsvIngest, DatasetCreate, RunCreate, Timestamp};
use nominal::streaming::api::clients::scout::checks::api::{AsyncChecklistService, AsyncChecklistServiceClient};
use nominal::streaming::api::clients::scout::datareview::api::{AsyncDataReviewService, AsyncDataReviewServiceClient};
use nominal::streaming::api::objects::api::{Channel, rids::WorkspaceRid};
use nominal::streaming::api::objects::scout::api::{ChannelLocator, DataSourceRefName, Priority};
use nominal::streaming::api::objects::scout::checks::api::{
    CreateCheckRequest, CreateChecklistEntryRequest, CreateChecklistRequest, UnresolvedCheckCondition,
    UnresolvedNumRangesConditionV3, UnresolvedVariableLocator,
};
use nominal::streaming::api::objects::scout::compute::api::{
    DoubleConstant, NumericSeries, RangeSeries, Reference, ThresholdOperator, ThresholdingRanges, VariableName,
};
use nominal::streaming::api::objects::scout::datareview::api::{
    AutomaticCheckEvaluationState as CheckState, BatchInitiateDataReviewRequest, CreateDataReviewRequest,
};
use nominal::streaming::api::objects::scout::{rids::api::UserRid, run::api::RunRid};
use nominal::streaming::client::async_conjure_client;
use nominal::streaming::client::conjure::http::client::{AsyncService, ConjureRuntime};
use nominal::streaming::prelude::{BearerToken, ResourceIdentifier};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load the API key and base URL that API Keys Setup saved
    let config = nominal::Config::load()?;
    let profile = config.get_profile("default").ok_or("no profile named default")?;
    let client = nominal::NominalClient::from_profile_config(profile)?;

    // The REST API's services for checklists and their executions, which it calls data reviews
    let token = BearerToken::new(profile.token())?;
    let api = async_conjure_client("checks", profile.base_url().parse()?).map_err(nominal::Error::from)?;
    let runtime = Arc::new(ConjureRuntime::default());
    let checklists = AsyncChecklistServiceClient::new(api.clone(), &runtime);
    let executions = AsyncDataReviewServiceClient::new(api, &runtime);

    // The checklist must be in the run's workspace, the profile's
    let workspace = WorkspaceRid(ResourceIdentifier::new(profile.workspace_rid().ok_or("no workspace in profile")?)?);

    // Stands in for your test's data file: temperature in °C, with one reading above 80
    let csv = [
        "timestamp,temperature",
        "2026-01-01T12:00:00Z,70.2",
        "2026-01-01T12:00:10Z,71.5",
        "2026-01-01T12:00:20Z,85.3",
        "2026-01-01T12:00:30Z,72.0",
    ];
    std::fs::write("bench-test.csv", csv.join("\n"))?;

    // Upload the test's data to a new dataset
    let (job, dataset_rid) = client
        .ingest()
        .upload_csv("bench-test.csv", DatasetCreate::new("Bench test"), CsvIngest::new(Timestamp::iso8601("timestamp")))
        .await?;
    client.ingest().wait_for_ingest_job(job.rid()).await?;

    // A run over the test, with the dataset under the ref name "default"
    let start: DateTime<Utc> = "2026-01-01T12:00:00Z".parse()?;
    let end: DateTime<Utc> = "2026-01-01T12:00:30Z".parse()?;
    let run = client.runs().create(RunCreate::new("Bench test 1", start).end(end)).await?;
    client.runs().add_dataset(run.rid(), "default", &dataset_rid).await?;

    // A check that flags where the temperature channel is above 80
    let condition = UnresolvedNumRangesConditionV3::builder()
        .ranges(RangeSeries::Threshold(ThresholdingRanges::new(
            NumericSeries::Raw(Reference::new(VariableName("temperature".into()))),
            DoubleConstant::Literal(80.0),
            ThresholdOperator::GreaterThan,
        )))
        .function_spec(BTreeMap::<String, String>::new()) // required, and empty for this check
        .threshold(0) // a violation when there are more than 0 of those ranges
        .operator(ThresholdOperator::GreaterThan)
        .insert_variables(
            VariableName("temperature".into()),
            UnresolvedVariableLocator::Series(ChannelLocator::new(
                DataSourceRefName("default".into()),
                Channel("temperature".into()),
            )),
        )
        .build();
    let check = CreateCheckRequest::builder()
        .title("Temperature above 80")
        .description("")
        .priority(Priority::P2)
        .condition(UnresolvedCheckCondition::NumRangesV3(condition))
        .build();

    // A checklist with the check, assigned to you
    let me = client.users().who_am_i().await?;
    let request = CreateChecklistRequest::builder()
        .commit_message("Create checklist")
        .assignee_rid(UserRid(ResourceIdentifier::new(me.rid())?))
        .title("Bench temperature limits")
        .description("")
        .push_checks(CreateChecklistEntryRequest::CreateCheck(check))
        .workspace(workspace)
        .build();
    let checklist = checklists.create(&token, &request).await.map_err(nominal::Error::from)?;

    // Execute the checklist on the run, and wait until no check is pending or executing
    let request = BatchInitiateDataReviewRequest::builder()
        .push_requests(CreateDataReviewRequest::new(RunRid(ResourceIdentifier::new(run.rid())?), checklist.rid().clone()))
        .build();
    let execution_rid = executions.batch_initiate(&token, &request).await.map_err(nominal::Error::from)?.rids()[0].clone();
    let execution = loop {
        let execution = executions.get(&token, &execution_rid).await.map_err(nominal::Error::from)?;
        let running = execution
            .check_evaluations()
            .iter()
            .any(|evaluation| matches!(evaluation.state(), CheckState::PendingExecution(_) | CheckState::Executing(_)));
        if !running {
            break execution;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    };

    // Each violation is an event the check created
    for evaluation in execution.check_evaluations() {
        match evaluation.state() {
            CheckState::Passing(_) => println!("Passed"),
            CheckState::GeneratedAlerts(alerts) => println!("Violations: {}", alerts.event_rids().len()),
            state => println!("{state:?}"), // any other outcome, such as FailedToExecute
        }
    }
    println!("{}", run.nominal_url());
    Ok(())
}
```
