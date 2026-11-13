# Cheat sheet

{.lead}
Everything starts from a `NominalClient`, and each area of Nominal has a client of its own: `client.assets()`, `client.runs()` and so on. Calls that reach Nominal are async and return a `nominal::Result`, so they end in `.await?`.

## Client

| Action | Rust |
| --- | --- |
| Connect with the `default` profile | `NominalClient::from_profile("default")?` |
| Connect with the profile named by `NOMINAL_PROFILE` | `NominalClient::from_profile_env()?` |
| Check who you're signed in as | `client.users().who_am_i().await?.email()` |

[Connect to Nominal](connect.md) covers connecting with an API key.

## Assets

| Action | Rust |
| --- | --- |
| Create an asset | `client.assets().create(AssetCreate::new("UUT1").properties([("serial", "SN-001")])).await?` |
| Get an asset by RID | `client.assets().get("<rid>").await?` |
| Search by label | `client.assets().search(AssetQuery::label("prototype")).await?` |
| Add a dataset under a data scope | `client.assets().add_dataset(asset.rid(), "default", "<dataset rid>").await?` |
| Set labels | `client.assets().update(asset.rid(), AssetUpdate::new().labels(["prototype"])).await?` |
| Archive | `client.assets().archive(asset.rid()).await?` |

## Datasets and channels

| Action | Rust |
| --- | --- |
| Create a dataset | `client.catalog().create_dataset(DatasetCreate::new("Bench test")).await?` |
| Get a dataset by RID | `client.catalog().get_dataset("<rid>").await?` |
| Search by name | `client.catalog().search_datasets(DatasetQuery::substring_match("Bench")).await?` |
| List a dataset's channels | `client.catalog().list_channels(dataset.rid()).await?` |
| Archive | `client.catalog().archive_dataset(dataset.rid()).await?` |

## Runs

| Action | Rust |
| --- | --- |
| Create a run on an asset | `client.runs().create(RunCreate::new("Test 1", start).end(end).assets([asset.rid()])).await?` |
| Get a run by RID | `client.runs().get("<rid>").await?` |
| Search by label | `client.runs().search(RunQuery::label("acceptance")).await?` |
| Add a dataset under a ref name | `client.runs().add_dataset(run.rid(), "default", dataset.rid()).await?` |
| End an open run | `client.runs().update(run.rid(), RunUpdate::new().end(Utc::now())).await?` |
| Archive | `client.runs().archive(run.rid()).await?` |

## Files and drives

| Action | Rust |
| --- | --- |
| Upload a CSV into a new dataset | `client.ingest().upload_csv("test.csv", DatasetCreate::new("Test 1"), CsvIngest::new(Timestamp::iso8601("timestamp"))).await?` |
| Wait for the ingest | `client.ingest().wait_for_ingest_job(job.rid()).await?` |
| Get a drive by its ID | `client.drives().get_by_id("test-reports").await?` |
| Put a file in a drive | `client.files(drive.rid()).put("report.pdf", "reports/report.pdf", UploadOptions::new()).await?` |
| List a drive folder | `client.files(drive.rid()).list("reports", false).await?` |

## Workbooks

| Action | Rust |
| --- | --- |
| Get a template | `client.templates().get("<rid>").await?` |
| Create a workbook for a run | `client.workbooks().create_from_template(&template, WorkbookDataScope::runs([run.rid()]), WorkbookCreate::new()).await?` |

## Not in the crate

| Action | Where |
| --- | --- |
| Stream values | `nominal::streaming`: [Stream data](stream.md) |
| Events, checklist authoring, runs over tagged data | The REST API: [Call the REST API directly](rest-api.md) |
| Read data back | SQL: [Get data out with SQL](sql.md) |

Types such as `AssetCreate` and `RunQuery` are in `nominal::core`.
