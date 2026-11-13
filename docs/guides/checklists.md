# Checklists

{.lead}
A checklist is a set of checks, such as limits on a channel, that Nominal evaluates against assets or runs. Run one on streamed data to watch an asset live. Make and publish checklists in Nominal, then find and run them from Rust.

To create a checklist from code, or run one on a finished run, use the REST API, as in [Call the REST API directly](rest-api.md#create-a-checklist).

## Find a checklist

`search` takes a `ChecklistQuery`. To leave out drafts and archived checklists, filter on both:

```rust
use nominal::core::ChecklistQuery;

let checklists = client
    .checklists()
    .search(ChecklistQuery::and([
        ChecklistQuery::search_text("thermal limits"),
        ChecklistQuery::is_published(true),
        ChecklistQuery::is_archived(false),
    ]))
    .await?;
```

`get` returns one checklist by RID, at its latest version.

## Run a checklist on streamed data

`execute_streaming` runs a checklist on the data streamed to one or more assets. If the checklist already runs on one of them, these options replace the old ones there.

```rust
use std::time::Duration;

use nominal::core::ExecuteStreamingChecklist;

client
    .checklists()
    .execute_streaming(
        "<ri.scout.instance.checklist.identifier>",
        ["<ri.scout.instance.asset.identifier>"],
        ExecuteStreamingChecklist::new()
            .evaluation_delay(Duration::from_secs(5)) // when data arrives late
            .auto_create_events(true),                // an event each time a check fails and recovers
    )
    .await?;
```

A failed check can't recover until `recovery_delay` has passed: 15 seconds by default, and at least 15 seconds. `integration_rids` sends notifications to integrations set up in Nominal.

To see which checklists run on an asset, use `list_for_asset`.

## Read the live status

```rust
use nominal::core::{CheckStatus, ChecklistState};

let status = client
    .checklists()
    .live_status("<ri.scout.instance.checklist.identifier>", "<ri.scout.instance.asset.identifier>")
    .await?;
if status.state() == ChecklistState::Running {
    for check in status.check_results() {
        if check.status() == CheckStatus::Fail {
            println!("{} failed", check.check_rid());
        }
    }
}
```

Check results are only filled in while the checklist is running. `live_status` returns an error if the checklist doesn't run on the asset.

## Stop a checklist

```rust
let checklist_rid = "<ri.scout.instance.checklist.identifier>";
client.checklists().stop_streaming_for_assets(checklist_rid, ["<ri.scout.instance.asset.identifier>"]).await?;
client.checklists().stop_streaming(checklist_rid).await?; // on every asset
```
