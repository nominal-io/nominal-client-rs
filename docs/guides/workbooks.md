# Workbooks

{.lead}
A workbook is a saved view of an asset's or a run's data, with panels, charts and tables. Create workbooks from a template, so every asset or run gets the same views.

## Create a workbook from a template

Make the template in Nominal, then create a workbook from it for an asset or a run. The template's inputs refer to data by data scope name or ref name, so they match on every asset or run that uses the same names, as in [Assets](assets.md#add-data-with-data-scopes) and [Runs](runs.md#create-a-run-from-datasets).

```rust
use nominal::core::{WorkbookCreate, WorkbookDataScope};

let template = client.templates().get("<ri.scout.instance.template.identifier>").await?;
let workbook = client
    .workbooks()
    .create_from_template(
        &template,
        WorkbookDataScope::runs(["<ri.scout.instance.run.identifier>"]),
        WorkbookCreate::new().title("Flight acceptance review").labels(["acceptance"]),
    )
    .await?;
println!("{}", workbook.nominal_url());
```

`WorkbookDataScope::assets` creates the workbook for assets instead. `templates().get` returns the template's latest version.

## Find workbooks

`search` takes a `WorkbookQuery`, which can filter by the asset or run a workbook is for:

```rust
use nominal::core::WorkbookQuery;

let workbooks = client
    .workbooks()
    .search(WorkbookQuery::and([
        WorkbookQuery::run_rid("<ri.scout.instance.run.identifier>"),
        WorkbookQuery::label("acceptance"),
    ]))
    .await?;
```

`get` returns one workbook by RID.

## Archive a workbook

`archive` hides the workbook from views and searches. `unarchive` restores it.

```rust
client.workbooks().archive(workbook.rid()).await?;
```
