# Call the REST API directly

{.lead}
The `nominal` crate comes with generated clients for Nominal's REST API, in `nominal::streaming::api`. Use them for what `NominalClient` doesn't cover, such as events, creating checklists, executing a checklist on a run, and [runs that select tagged data](../examples/simulation-runs.md).

To use this page, first make sure you can [Connect to Nominal](connect.md) and obtain a `client`.

## Set up the REST API clients

```rust
use std::sync::Arc;

use nominal::streaming::api::objects::api::rids::WorkspaceRid;
use nominal::streaming::client::async_conjure_client;
use nominal::streaming::client::conjure::http::client::{AsyncService, ConjureRuntime};
use nominal::streaming::prelude::{BearerToken, ProductAgent, ResourceIdentifier};

// Load the API key, base URL and workspace that API Keys Setup saved
let config = nominal::Config::load()?;
let profile = config.get_profile("default").ok_or("no profile named default")?;
let token = BearerToken::new(profile.token())?;
let workspace = WorkspaceRid(ResourceIdentifier::new(profile.workspace_rid().ok_or("no workspace in the profile")?)?);

// One HTTP client for every service, named "nominal" in its logs
let api = async_conjure_client("nominal", profile.base_url().parse()?, &ProductAgent::default()).map_err(nominal::Error::from)?;
let runtime = Arc::new(ConjureRuntime::default());
```

Each service's client is in `nominal::streaming::api::clients`, and its requests and responses are in `nominal::streaming::api::objects`. Each client method is an endpoint of the same name in the [REST API reference](https://dev.nominal.io/api/), such as `create_event` for [Create event](https://dev.nominal.io/api/legacy/event-service/#create-event). The reference writes fields in camelCase, such as `assetRids` for `asset_rids`.

## Create an event

An [event](https://dev.nominal.io/platform/data-model/#events) marks a moment or a span of time on assets, such as a fault:

```rust
use nominal::streaming::api::clients::event::{AsyncEventService, AsyncEventServiceClient};
use nominal::streaming::api::objects::api::Timestamp;
use nominal::streaming::api::objects::event::{CreateEvent, EventType};
use nominal::streaming::api::objects::scout::{rids::api::AssetRid, run::api::Duration};
use nominal::streaming::client::conjure::object::SafeLong;

// Times are in seconds and nanoseconds
let event = CreateEvent::builder()
    .timestamp(Timestamp::new(SafeLong::new(1_767_268_800)?, SafeLong::new(0)?)) // 2026-01-01T12:00:00Z
    .duration(Duration::new(SafeLong::new(5)?, SafeLong::new(0)?)) // 0 for a moment
    .name("Overtemperature")
    .type_(EventType::Error)
    .insert_asset_rids(AssetRid(ResourceIdentifier::new("<ri.scout.instance.asset.identifier>")?))
    .build();
let events = AsyncEventServiceClient::new(api.clone(), &runtime);
events.create_event(&token, &event).await.map_err(nominal::Error::from)?; // convert to nominal::Error so ? works
```

## Create a checklist

A check's condition finds channels through variables. This check fails where the channel `temperature`, under the [ref name](https://dev.nominal.io/platform/data-model/#data-scopes-and-ref-names) `default`, is above 80:

```rust
use std::collections::BTreeMap;

use nominal::streaming::api::clients::scout::checks::api::{AsyncChecklistService, AsyncChecklistServiceClient};
use nominal::streaming::api::objects::api::Channel;
use nominal::streaming::api::objects::scout::api::{ChannelLocator, DataSourceRefName, Priority};
use nominal::streaming::api::objects::scout::checks::api::{
    CreateCheckRequest, CreateChecklistEntryRequest, CreateChecklistRequest, UnresolvedCheckCondition,
    UnresolvedNumRangesConditionV3, UnresolvedVariableLocator,
};
use nominal::streaming::api::objects::scout::compute::api::{
    DoubleConstant, NumericSeries, RangeSeries, Reference, ThresholdOperator, ThresholdingRanges, VariableName,
};
use nominal::streaming::api::objects::scout::rids::api::UserRid;

let condition = UnresolvedNumRangesConditionV3::builder()
    .ranges(RangeSeries::Threshold(ThresholdingRanges::new(
        NumericSeries::Raw(Reference::new(VariableName("temperature".into()))),
        DoubleConstant::Literal(80.0),
        ThresholdOperator::GreaterThan,
    )))
    .function_spec(BTreeMap::<String, String>::new())
    .threshold(0) // fail if there are more than 0 ranges above 80
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

let me = client.users().who_am_i().await?;
let request = CreateChecklistRequest::builder()
    .commit_message("Create checklist")
    .assignee_rid(UserRid(ResourceIdentifier::new(me.rid())?))
    .title("Temperature limits")
    .description("")
    .push_checks(CreateChecklistEntryRequest::CreateCheck(check))
    .workspace(workspace.clone())
    .build();
let checklist = AsyncChecklistServiceClient::new(api.clone(), &runtime)
    .create(&token, &request)
    .await
    .map_err(nominal::Error::from)?;
println!("{}", checklist.rid());
```

[`CreateCheckRequest`](https://dev.nominal.io/api/rest/checklist-service/#createcheckrequest) lists a check's fields, and [`UnresolvedCheckCondition`](https://dev.nominal.io/api/rest/checklist-service/#unresolvedcheckcondition) the other kinds of condition.

## Execute a checklist on a run

To check a test after it ends, execute a checklist on its run and wait for the checks to finish. Each check passes or fails:

```rust
use nominal::streaming::api::clients::scout::datareview::api::{AsyncDataReviewService, AsyncDataReviewServiceClient};
use nominal::streaming::api::objects::scout::datareview::api::{
    AutomaticCheckEvaluationState as State, BatchInitiateDataReviewRequest, CreateDataReviewRequest,
};
use nominal::streaming::api::objects::scout::{rids::api::ChecklistRid, run::api::RunRid};

let reviews = AsyncDataReviewServiceClient::new(api.clone(), &runtime);
let request = BatchInitiateDataReviewRequest::builder()
    .push_requests(CreateDataReviewRequest::new(
        RunRid(ResourceIdentifier::new("<ri.scout.instance.run.identifier>")?),
        ChecklistRid(ResourceIdentifier::new("<ri.scout.instance.check-collection.identifier>")?),
    ))
    .build();
let review_rid = reviews.batch_initiate(&token, &request).await.map_err(nominal::Error::from)?.rids()[0].clone();

// Wait until no check is pending or executing
let review = loop {
    let review = reviews.get(&token, &review_rid).await.map_err(nominal::Error::from)?;
    let executing = review
        .check_evaluations()
        .iter()
        .any(|check| matches!(check.state(), State::PendingExecution(_) | State::Executing(_)));
    if !executing {
        break review;
    }
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
};

// Checks that generated alerts failed
let failed = review
    .check_evaluations()
    .iter()
    .filter(|check| matches!(check.state(), State::GeneratedAlerts(_) | State::TooManyAlerts(_)))
    .count();
println!("{failed} of {} checks failed", review.check_evaluations().len());
```

[`AutomaticCheckEvaluationState`](https://dev.nominal.io/api/rest/data-review-service/#automaticcheckevaluationstate) lists the other states a check can be in.
