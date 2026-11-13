# Call the REST API directly

{.lead}
The `nominal` crate covers the common tasks. For the rest of Nominal's REST API, such as events and creating checklists, use the generated REST API clients that come with it, as `nominal::streaming::api`. They take your API key and base URL from the same profile.

## Set up a client

Each REST API service has a client in `nominal::streaming::api::clients`, with its requests and responses in `nominal::streaming::api::objects`. Create the HTTP client once, and the service clients from it:

```rust
use std::sync::Arc;

use anyhow::Context;
use nominal::streaming::client::async_conjure_client;
use nominal::streaming::client::conjure::http::client::{AsyncService, ConjureRuntime};
use nominal::streaming::prelude::BearerToken;

let config = nominal::Config::load()?;
let profile = config.get_profile("default").context("no profile named default")?;
let token = BearerToken::new(profile.token())?;
let api = async_conjure_client("events", profile.base_url().parse()?).map_err(nominal::Error::from)?;
let runtime = Arc::new(ConjureRuntime::default());
```

The first argument of `async_conjure_client` is a name for the client. Requests go to the base URL. Every request takes the token, and its errors convert to `nominal::Error`.

The examples on this page use [anyhow](https://docs.rs/anyhow) for errors. The [REST API reference](https://dev.nominal.io/api/) lists every service and its fields.

## Create an event

An event marks a moment or a span of time on assets, such as a fault:

```rust
use nominal::streaming::api::clients::event::{AsyncEventService, AsyncEventServiceClient};
use nominal::streaming::api::objects::scout::{rids::api::AssetRid, run::api::Duration};
use nominal::streaming::api::objects::{api::Timestamp, event::{CreateEvent, EventType}};
use nominal::streaming::client::conjure::object::SafeLong;
use nominal::streaming::prelude::ResourceIdentifier;

let events = AsyncEventServiceClient::new(api.clone(), &runtime);
let event = CreateEvent::builder()
    .timestamp(Timestamp::new(SafeLong::new(1_767_276_720)?, SafeLong::new(0)?)) // 2026-01-01T14:12:00Z
    .duration(Duration::new(SafeLong::new(5)?, SafeLong::new(0)?)) // zero for a point in time
    .name("Overtemperature")
    .type_(EventType::Error)
    .insert_asset_rids(AssetRid(ResourceIdentifier::new("<ri.scout.instance.asset.identifier>")?))
    .build();
events.create_event(&token, &event).await.map_err(nominal::Error::from)?;
```

Timestamps and durations are seconds and nanoseconds.

## Create a checklist

A checklist's checks each have a condition on channels. The condition refers to a channel through a variable, bound to a channel name under a ref name. This check is violated where `temperature`, under the ref name `default`, is above 80:

```rust
use std::collections::BTreeMap;

use nominal::streaming::api::clients::scout::checks::api::{AsyncChecklistService, AsyncChecklistServiceClient};
use nominal::streaming::api::objects::scout::api::{ChannelLocator, DataSourceRefName, Priority};
use nominal::streaming::api::objects::scout::checks::api::{
    CreateCheckRequest, CreateChecklistEntryRequest, CreateChecklistRequest, UnresolvedCheckCondition,
    UnresolvedNumRangesConditionV3, UnresolvedVariableLocator,
};
use nominal::streaming::api::objects::scout::compute::api::{
    DoubleConstant, NumericSeries, RangeSeries, Reference, ThresholdOperator, ThresholdingRanges, VariableName,
};
use nominal::streaming::api::objects::{api::Channel, scout::rids::api::UserRid};
use nominal::streaming::prelude::ResourceIdentifier;

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
let check = CreateCheckRequest::builder()
    .title("Temperature above 80")
    .description("")
    .priority(Priority::P2)
    .condition(UnresolvedCheckCondition::NumRangesV3(condition))
    .build();

let me = nominal::NominalClient::from_profile("default")?.users().who_am_i().await?;
let request = CreateChecklistRequest::builder()
    .commit_message("Create checklist")
    .assignee_rid(UserRid(ResourceIdentifier::new(me.rid())?))
    .title("Temperature limits")
    .description("")
    .push_checks(CreateChecklistEntryRequest::CreateCheck(check))
    .build();
let checklist = AsyncChecklistServiceClient::new(api.clone(), &runtime)
    .create(&token, &request)
    .await
    .map_err(nominal::Error::from)?;
println!("{}", checklist.rid());
```

## Run a checklist on a run

To check a finished test, run a checklist on its run, then wait until no check is pending or executing. Each check that found violations lists the events it created:

```rust
use std::collections::BTreeSet;

use nominal::streaming::api::clients::event::{AsyncEventService, AsyncEventServiceClient};
use nominal::streaming::api::clients::scout::datareview::api::{AsyncDataReviewService, AsyncDataReviewServiceClient};
use nominal::streaming::api::objects::scout::datareview::api::{
    AutomaticCheckEvaluationState as State, BatchInitiateDataReviewRequest, CreateDataReviewRequest,
};
use nominal::streaming::api::objects::{event::EventType, scout::{rids::api::ChecklistRid, run::api::RunRid}};
use nominal::streaming::prelude::ResourceIdentifier;

let reviews = AsyncDataReviewServiceClient::new(api.clone(), &runtime);

// Run the checklist's latest version on the run
let request = BatchInitiateDataReviewRequest::builder()
    .push_requests(CreateDataReviewRequest::new(
        RunRid(ResourceIdentifier::new("<ri.scout.instance.run.identifier>")?),
        ChecklistRid(ResourceIdentifier::new("<ri.scout.instance.check-collection.identifier>")?),
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
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
};

let event_rids: BTreeSet<_> = review
    .check_evaluations()
    .iter()
    .filter_map(|c| match c.state() {
        State::GeneratedAlerts(s) => Some(s.event_rids().clone()),
        _ => None,
    })
    .flatten()
    .collect();
let events = AsyncEventServiceClient::new(api.clone(), &runtime)
    .batch_get_events(&token, &event_rids)
    .await
    .map_err(nominal::Error::from)?;
let violations = events.iter().filter(|e| *e.type_() == EventType::Error).count();
println!("{violations} violations");
```

To check data as it streams in, use the `nominal` crate's [checklists](checklists.md) client instead.

## Create a run that selects tagged data

A run can select only the points of a dataset with given tag values, such as one execution of a simulation:

```rust
use nominal::streaming::api::clients::scout::{AsyncRunService, AsyncRunServiceClient};
use nominal::streaming::api::objects::api::{TagName, TagValue, rids::DatasetRid};
use nominal::streaming::api::objects::scout::api::DataSourceRefName;
use nominal::streaming::api::objects::scout::run::api::{CreateRunDataSource, CreateRunRequest, DataSource, UtcTimestamp};
use nominal::streaming::client::conjure::object::SafeLong;
use nominal::streaming::prelude::ResourceIdentifier;

let runs = AsyncRunServiceClient::new(api.clone(), &runtime);
let t0 = chrono::DateTime::UNIX_EPOCH;
let source = CreateRunDataSource::builder()
    .data_source(DataSource::Dataset(DatasetRid(ResourceIdentifier::new("<ri.catalog.instance.dataset.identifier>")?)))
    .insert_series_tags(TagName("model".into()), TagValue("A".into()))
    .insert_series_tags(TagName("execution-id".into()), TagValue("7".into()))
    .build();
let request = CreateRunRequest::builder()
    .title("Model A, execution 7")
    .description("")
    .start_time(UtcTimestamp::new(SafeLong::new(t0.timestamp())?))
    .end_time(UtcTimestamp::new(SafeLong::new(t0.timestamp() + 60)?))
    .insert_data_sources(DataSourceRefName("default".into()), source)
    .build();
runs.create_run(&token, &request).await.map_err(nominal::Error::from)?;
```

See [Simulation runs](../examples/simulation-runs.md) for a full program.
