# Checklists

{.lead}
A [checklist](https://dev.nominal.io/platform/workbooks/#checklists) is a set of checks, such as a limit on a channel, that each pass or fail. Checklists allow users to perform [automated pass/fail checking](https://dev.nominal.io/platform/data-model/paradigms/stories/automated-checks/) on their test data.

Make checklists in Nominal, or [create them through the REST API](rest-api.md#create-a-checklist). To execute a checklist on a run after a test, use the [REST API](rest-api.md#execute-a-checklist-on-a-run). The [Check a test against its limits](../examples/automated-checks.md) example does both.

To use this page, first make sure you can [Connect to Nominal](connect.md) and obtain a `client`.

## Fetch a checklist

If the RID of a checklist is known, you can fetch that checklist directly without needing to search.

```rust
let checklist = client.checklists().get("<ri.scout.instance.check-collection.identifier>").await?;
```

## Find checklists

{rust:method}`search <ChecklistsClient::search>` finds checklists by text, label, property or whether they're published. Combine filters with {rust:method}`and <ChecklistQuery::and>` and {rust:method}`or <ChecklistQuery::or>`:

```rust
use nominal::core::ChecklistQuery;

// Every checklist in the workspace using an empty query
let checklists = client.checklists().search(ChecklistQuery::and([])).await?;

// Titles or descriptions that match "temperature limits"
let checklists = client.checklists().search(ChecklistQuery::search_text("temperature limits")).await?;

// The same, leaving out unpublished checklists
let checklists = client
    .checklists()
    .search(ChecklistQuery::and([
        ChecklistQuery::search_text("temperature limits"),
        ChecklistQuery::is_published(true),
    ]))
    .await?;
```

## Attach a checklist to assets

{rust:method}`execute_streaming <ChecklistsClient::execute_streaming>` attaches a checklist to one or more assets, so it checks all the data they stream from then on. If the checklist is already attached to one of those assets, the options you pass replace its current ones.

```rust
use std::time::Duration;

use nominal::core::ExecuteStreamingChecklist;

let checklist_rid = "<ri.scout.instance.check-collection.identifier>";
let asset_rid = "<ri.scout.instance.asset.identifier>";

client
    .checklists()
    .execute_streaming(
        checklist_rid,
        [asset_rid],
        ExecuteStreamingChecklist::new()
            .evaluation_delay(Duration::from_secs(5)) // wait for data that arrives late
            .auto_create_events(true),                // create events when a check fails or recovers
    )
    .await?;
```

{rust:struct}`ExecuteStreamingChecklist` also sets how long a failed check waits before it can recover, and integrations to notify when a check fails. {rust:method}`list_for_asset <ChecklistsClient::list_for_asset>` lists the checklists attached to an asset.

:::{warning}
Some checklist features can't run on streamed data. A checklist that uses them still attaches, but each check that uses one is never evaluated: its [live status](#read-the-live-status) stays `CheckStatus::Skipped`, while the checklist's other checks run as usual. To evaluate those checks, [execute the checklist on a run](rest-api.md#execute-a-checklist-on-a-run) after the test.
:::

## Read the live status

```rust
use nominal::core::CheckStatus;

let status = client.checklists().live_status(checklist_rid, asset_rid).await?;
// The check results are empty until the checklist has checked some data
for check in status.check_results() {
    match check.status() {
        CheckStatus::Pass => {}
        CheckStatus::Fail => println!("{} failed", check.check_rid()),
        // Skipped or Invalid: the check wasn't evaluated, such as when its channels can't be found
        other => println!("{} not evaluated: {other:?}", check.check_rid()),
    }
}
```

{rust:method}`live_status <ChecklistsClient::live_status>` returns an error if the checklist isn't attached to the asset.

## Detach a checklist from assets

{rust:method}`stop_streaming_for_assets <ChecklistsClient::stop_streaming_for_assets>` detaches a checklist from the assets you name, and {rust:method}`stop_streaming <ChecklistsClient::stop_streaming>` detaches it from every asset. The checklist itself is unchanged, so you can still execute it on runs.

```rust
// for a specific asset
client.checklists().stop_streaming_for_assets(checklist_rid, [asset_rid]).await?;
// for all assets
client.checklists().stop_streaming(checklist_rid).await?;
```
