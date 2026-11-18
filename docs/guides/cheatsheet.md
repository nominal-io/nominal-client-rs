# Cheat sheet

{.lead}
{rust:struct}`NominalClient` has a client for each area of Nominal, such as `client.assets()` for assets and `client.catalog()` for datasets and channels.

This project setup command and associated imports will prepare you to call any of the functions listed on this page.

```shell
cargo new nominal-cheatsheet
cd nominal-cheatsheet
cargo add nominal chrono
cargo add tokio --features full
```

```rust
use chrono::Utc; // chrono is the time library used by the nominal crate
use nominal::core::*; // AssetCreate, RunQuery and the other types below

let client = nominal::NominalClient::from_profile("default")?;
```

## Assets

| Action | Ref | Rust |
| --- | :---: | --- |
| Create an asset | {rust:method}`📄 <AssetsClient::create>` | `client.assets().create(AssetCreate::new("Engine 1").labels(["prototype"])).await?` |
| Get an asset by RID | {rust:method}`📄 <AssetsClient::get>` | `client.assets().get("<rid>").await?` |
| Search by label | {rust:method}`📄 <AssetsClient::search>` | `client.assets().search(AssetQuery::label("prototype")).await?` |
| Add a dataset under a [ref name](https://dev.nominal.io/platform/data-model/#data-scopes-and-ref-names) | {rust:method}`📄 <AssetsClient::add_dataset>` | `client.assets().add_dataset(asset.rid(), "telemetry", dataset.rid()).await?` |
| Replace labels | {rust:method}`📄 <AssetsClient::update>` | `client.assets().update(asset.rid(), AssetUpdate::new().labels(["prototype", "flight"])).await?` |
| Archive | {rust:method}`📄 <AssetsClient::archive>` | `client.assets().archive(asset.rid()).await?` |

## Runs

| Action | Ref | Rust |
| --- | :---: | --- |
| Start a run on an asset now | {rust:method}`📄 <RunsClient::create>` | `client.runs().create(RunCreate::new("Engine test 1", Utc::now()).assets([asset.rid()])).await?` |
| End an open run | {rust:method}`📄 <RunsClient::update>` | `client.runs().update(run.rid(), RunUpdate::new().end(Utc::now())).await?` |
| Get a run by RID | {rust:method}`📄 <RunsClient::get>` | `client.runs().get("<rid>").await?` |
| Search by label | {rust:method}`📄 <RunsClient::search>` | `client.runs().search(RunQuery::label("acceptance")).await?` |
| Add a dataset to a run with no assets | {rust:method}`📄 <RunsClient::add_dataset>` | `client.runs().add_dataset(run.rid(), "telemetry", dataset.rid()).await?` |
| Archive | {rust:method}`📄 <RunsClient::archive>` | `client.runs().archive(run.rid()).await?` |

## Datasets and channels

| Action | Ref | Rust |
| --- | :---: | --- |
| Create a dataset | {rust:method}`📄 <CatalogClient::create_dataset>` | `client.catalog().create_dataset(DatasetCreate::new("Engine test 1")).await?` |
| Get a dataset by RID | {rust:method}`📄 <CatalogClient::get_dataset>` | `client.catalog().get_dataset("<rid>").await?` |
| Search by part of the name | {rust:method}`📄 <CatalogClient::search_datasets>` | `client.catalog().search_datasets(DatasetQuery::substring_match("Engine")).await?` |
| List a dataset's channels | {rust:method}`📄 <CatalogClient::list_channels>` | `client.catalog().list_channels(dataset.rid()).await?` |
| Archive | {rust:method}`📄 <CatalogClient::archive_dataset>` | `client.catalog().archive_dataset(dataset.rid()).await?` |

## Files and drives

| Action | Ref | Rust |
| --- | :---: | --- |
| Upload a CSV into a new dataset | {rust:method}`📄 <IngestClient::upload_csv>` | `client.ingest().upload_csv("test.csv", DatasetCreate::new("Engine test 1"), CsvIngest::new(Timestamp::iso8601("timestamp"))).await?` |
| Wait for the upload's ingest job | {rust:method}`📄 <IngestClient::wait_for_ingest_job>` | `client.ingest().wait_for_ingest_job(job.rid()).await?` |
| Get a drive by name | {rust:method}`📄 <DrivesClient::get_by_id>` | `client.drives().get_by_id("test-reports").await?` |
| List a drive folder | {rust:method}`📄 <DriveFilesClient::list>` | `client.files(drive.rid()).list("reports", false).await?` |

## Workbooks

| Action | Ref | Rust |
| --- | :---: | --- |
| Get a template by RID | {rust:method}`📄 <TemplatesClient::get>` | `client.templates().get("<rid>").await?` |
| Create a workbook for a run | {rust:method}`📄 <WorkbooksClient::create_from_template>` | `client.workbooks().create_from_template(&template, WorkbookDataScope::runs([run.rid()]), WorkbookCreate::new()).await?` |

## Beyond `NominalClient`

- Stream points as your program produces them: [Stream data](stream.md)
- Query your data with SQL: [Query data with SQL](sql.md)
- Create events and checklists, and execute a checklist on a run: [Call the REST API directly](rest-api.md)
- Create runs that select tagged data: [Simulation runs](../examples/simulation-runs.md)
