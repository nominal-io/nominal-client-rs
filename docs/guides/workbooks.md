# Workbooks

{.lead}
A [workbook](https://dev.nominal.io/platform/workbooks/) is where you plot and review an asset's or a run's data in Nominal. Create workbooks from a template, so every asset or run gets the same views.

## Create a workbook from a template

Save a workbook as a template in Nominal, then create a workbook from the template for each run or asset. The template finds channels by [ref name](https://dev.nominal.io/platform/data-model/#data-scopes-and-ref-names), so it works on every run or asset that adds its datasets under the same ref names, as in [Runs](runs.md) and [Assets](assets.md).

```rust
use nominal::core::{WorkbookCreate, WorkbookDataScope};

let client = nominal::NominalClient::from_profile("default")?;

let template = client.templates().get("<ri.scout.instance.template.identifier>").await?;
let workbook = client
    .workbooks()
    .create_from_template(
        &template,
        WorkbookDataScope::runs(["<ri.scout.instance.run.identifier>"]), // or WorkbookDataScope::assets
        WorkbookCreate::new().title("Flight acceptance review").labels(["acceptance"]),
    )
    .await?;
println!("{}", workbook.nominal_url());
```

## Find workbooks

```rust
use nominal::core::WorkbookQuery;

// Workbooks for one run, labeled "acceptance"
let workbooks = client
    .workbooks()
    .search(WorkbookQuery::and([
        WorkbookQuery::run_rid("<ri.scout.instance.run.identifier>"),
        WorkbookQuery::label("acceptance"),
    ]))
    .await?;
```

{rust:enum}`WorkbookQuery` also searches by text, property and asset. {rust:method}`get <WorkbooksClient::get>` returns one workbook by RID.

## Archive a workbook

Archiving hides a workbook without deleting it. {rust:method}`unarchive <WorkbooksClient::unarchive>` restores it.

```rust
client.workbooks().archive(workbook.rid()).await?;
```
