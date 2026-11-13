# Get data out with SQL

{.lead}
Read your data back from Nominal with SQL, through the SQL REST API. The `nominal` crate has no SQL client, so these examples send the requests with [reqwest](https://docs.rs/reqwest), using the API key, base URL and workspace from your profile.

Add reqwest, with its `json` feature, and serde_json:

```shell
cargo add reqwest --features json
cargo add serde_json anyhow
```

[Writing queries](https://dev.nominal.io/sql-guides/writing-queries/) covers the tables you can query and the SQL they accept.

## Run a query

`/sql/v1/query` returns a query's results in the response. With `result_format` set to CSV, the response is CSV text:

```rust
use anyhow::Context;

let config = nominal::Config::load()?;
let profile = config.get_profile("default").context("no profile named default")?;
let workspace_rid = profile.workspace_rid().context("the profile has no workspace RID")?;

let csv = reqwest::Client::new()
    .post(format!("{}/sql/v1/query", profile.base_url()))
    .bearer_auth(profile.token())
    .json(&serde_json::json!({
        "query": "SELECT dataset_rid, name FROM datasets LIMIT 20",
        "workspace_rid": workspace_rid,
        "result_format": "SQL_SERVICE_QUERY_RESULT_FORMAT_CSV",
    }))
    .send()
    .await?
    .error_for_status()?
    .text()
    .await?;
println!("{csv}");
```

The [query endpoint's reference](https://dev.nominal.io/sql-reference/rest-api/query/) has its limits and other result formats.

## Export a large result

`/sql/v1/query/export` writes a query's full result to a file, with no row limit, and returns a download URL:

```rust
use anyhow::Context;

let config = nominal::Config::load()?;
let profile = config.get_profile("default").context("no profile named default")?;
let workspace_rid = profile.workspace_rid().context("the profile has no workspace RID")?;

let response: serde_json::Value = reqwest::Client::new()
    .post(format!("{}/sql/v1/query/export", profile.base_url()))
    .bearer_auth(profile.token())
    .json(&serde_json::json!({
        "query": "SELECT ts, channel, value FROM points_double WHERE dataset_rid = '<ri.catalog.instance.dataset.identifier>'",
        "workspace_rid": workspace_rid,
    }))
    .send()
    .await?
    .error_for_status()?
    .json()
    .await?;
let url = response["presignedUrl"].as_str().context("no presignedUrl in the response")?;
std::fs::write("export.csv", reqwest::get(url).await?.error_for_status()?.bytes().await?)?;
```

The download URL carries its own credentials, so treat it as a secret. The [export endpoint's reference](https://dev.nominal.io/sql-reference/rest-api/export/) covers Parquet files.
