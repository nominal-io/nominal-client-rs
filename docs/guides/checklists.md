# Checklists

{.lead}
A [checklist](https://dev.nominal.io/platform/workbooks/#checklists) is a set of checks, such as a limit on a channel, that each pass or fail. Execute a checklist on the data streaming to an asset to see failed checks live.

Make checklists in Nominal, or [create them through the REST API](rest-api.md#create-a-checklist). To execute a checklist on a run after a test, use the [REST API](rest-api.md#execute-a-checklist-on-a-run) as well.

[Automated pass/fail checks](https://dev.nominal.io/platform/data-model/paradigms/stories/automated-checks/) shows how checklists fit into testing.

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

## Execute a checklist on streamed data

{rust:method}`execute_streaming <ChecklistsClient::execute_streaming>` executes a checklist on the data streaming to one or more assets. If the checklist already executes on one of them, these options replace the old ones there.

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

{rust:struct}`ExecuteStreamingChecklist` also sets how long a failed check waits before it can recover, and integrations to notify when a check fails. {rust:method}`list_for_asset <ChecklistsClient::list_for_asset>` lists the checklists executing on an asset.

## Read the live status

```rust
use nominal::core::CheckStatus;

let status = client.checklists().live_status(checklist_rid, asset_rid).await?;
// The check results are empty until the checklist is running
for check in status.check_results() {
    if check.status() == CheckStatus::Fail {
        println!("{} failed", check.check_rid());
    }
}
```

{rust:method}`live_status <ChecklistsClient::live_status>` returns an error if the checklist isn't executing on the asset.

## Stop a checklist

```rust
client.checklists().stop_streaming_for_assets(checklist_rid, [asset_rid]).await?;
client.checklists().stop_streaming(checklist_rid).await?; // on every asset
```
