# Cheat sheet

{.lead}
{rust:struct}`NominalClient` has a client for each area of Nominal, such as `client.assets()` for assets and `client.catalog()` for datasets and channels.

The calls below use this client and these imports:

```rust
use chrono::Utc; // cargo add chrono
use nominal::core::*; // AssetCreate, RunQuery and the other types below

let client = nominal::NominalClient::from_profile("default")?;
```

## Assets

| Action | Rust |
| --- | --- |
| Create an asset | `client.assets().create(AssetCreate::new("Engine 1").labels(["prototype"])).await?` |
| Get an asset by RID | `client.assets().get("<rid>").await?` |
| Search by label | `client.assets().search(AssetQuery::label("prototype")).await?` |
| Add a dataset under a [ref name](https://dev.nominal.io/platform/data-model/#data-scopes-and-ref-names) | `client.assets().add_dataset(asset.rid(), "telemetry", dataset.rid()).await?` |
| Replace labels | `client.assets().update(asset.rid(), AssetUpdate::new().labels(["prototype", "flight"])).await?` |
| Archive | `client.assets().archive(asset.rid()).await?` |

## Runs

| Action | Rust |
| --- | --- |
| Start a run on an asset now | `client.runs().create(RunCreate::new("Engine test 1", Utc::now()).assets([asset.rid()])).await?` |
| End an open run | `client.runs().update(run.rid(), RunUpdate::new().end(Utc::now())).await?` |
| Get a run by RID | `client.runs().get("<rid>").await?` |
| Search by label | `client.runs().search(RunQuery::label("acceptance")).await?` |
| Add a dataset to a run with no assets | `client.runs().add_dataset(run.rid(), "telemetry", dataset.rid()).await?` |
| Archive | `client.runs().archive(run.rid()).await?` |

## Datasets and channels

| Action | Rust |
| --- | --- |
| Create a dataset | `client.catalog().create_dataset(DatasetCreate::new("Engine test 1")).await?` |
| Get a dataset by RID | `client.catalog().get_dataset("<rid>").await?` |
| Search by part of the name | `client.catalog().search_datasets(DatasetQuery::substring_match("Engine")).await?` |
| List a dataset's channels | `client.catalog().list_channels(dataset.rid()).await?` |
| Archive | `client.catalog().archive_dataset(dataset.rid()).await?` |

## Files and drives

| Action | Rust |
| --- | --- |
| Upload a CSV into a new dataset | `client.ingest().upload_csv("test.csv", DatasetCreate::new("Engine test 1"), CsvIngest::new(Timestamp::iso8601("timestamp"))).await?` |
| Wait for the upload's ingest job | `client.ingest().wait_for_ingest_job(job.rid()).await?` |
| Get a drive by name | `client.drives().get_by_id("test-reports").await?` |
| List a drive folder | `client.files(drive.rid()).list("reports", false).await?` |

## Workbooks

| Action | Rust |
| --- | --- |
| Get a template by RID | `client.templates().get("<rid>").await?` |
| Create a workbook for a run | `client.workbooks().create_from_template(&template, WorkbookDataScope::runs([run.rid()]), WorkbookCreate::new()).await?` |

## Beyond `NominalClient`

- Stream points as your program produces them: [Stream data](stream.md)
- Query your data with SQL: [Get data out with SQL](sql.md)
- Create events and checklists, and execute a checklist on a run: [Call the REST API directly](rest-api.md)
- Create runs that select tagged data: [Simulation runs](../examples/simulation-runs.md)
