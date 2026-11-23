# Workbooks

{.lead}
A [workbook](https://dev.nominal.io/platform/workbooks/) is where you plot and review an asset's or a run's data in Nominal. Use rust to create workbooks from a template, streamlining your team's [post-test analysis](https://dev.nominal.io/platform/data-model/paradigms/stories/post-test/).

To use this page, first make sure you can [Connect to Nominal](connect.md) and obtain a `client`.

## Create a workbook from a template

Save a hand-made workbook as a template in Nominal, then create a workbook from the template for each run or asset. The template finds channels by [ref name](https://dev.nominal.io/platform/data-model/#data-scopes-and-ref-names), so it works on every run or asset that adds its datasets under the same ref names.

```rust
use nominal::core::{WorkbookCreate, WorkbookDataScope};

let template = client.templates().get("<ri.scout.instance.template.identifier>").await?;
let workbook = client
    .workbooks()
    .create_from_template(
        &template,
        // use WorkbookDataScope::runs or WorkbookDataScope::assets with a matching set of RIDs
        WorkbookDataScope::runs(["<ri.scout.instance.run.identifier>"]), 
        WorkbookCreate::new().title("Flight acceptance review").labels(["acceptance"]),
    )
    .await?;
println!("{}", workbook.nominal_url());
```

## Fetch a workbook

If the RID of a workbook is known, you can fetch that workbook directly without needing to search.

```rust
let workbook = client.workbooks().get("<ri.scout.instance.notebook.identifier>").await?;
```

## Find workbooks

{rust:method}`search <WorkbooksClient::search>` finds workbooks by text, label, property, asset or run. Combine filters with {rust:method}`and <WorkbookQuery::and>` and {rust:method}`or <WorkbookQuery::or>`:

```rust
use nominal::core::WorkbookQuery;

// Every workbook in the workspace using an empty query
let workbooks = client.workbooks().search(WorkbookQuery::and([])).await?;

// Titles or descriptions that match "acceptance review"
let workbooks = client.workbooks().search(WorkbookQuery::search_text("acceptance review")).await?;

// Workbooks for one run, labeled "acceptance"
let workbooks = client
    .workbooks()
    .search(WorkbookQuery::and([
        WorkbookQuery::run_rid("<ri.scout.instance.run.identifier>"),
        WorkbookQuery::label("acceptance"),
    ]))
    .await?;
```

## Archive a workbook

Archiving hides a workbook without deleting it. {rust:method}`unarchive <WorkbooksClient::unarchive>` restores it.

```rust
client.workbooks().archive(workbook.rid()).await?;
```
