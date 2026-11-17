# Get data out with SQL

{.lead}
Read your data back from Nominal with SQL, through the [SQL REST API](https://dev.nominal.io/sql-reference/rest-api/). [Writing queries](https://dev.nominal.io/sql-guides/writing-queries/) covers the tables you can query and the SQL they accept.

To connect a Postgres driver such as tokio-postgres instead, see [Postgres clients](https://dev.nominal.io/sql-guides/postgres-clients/).

## Run a query

Add [reqwest](https://docs.rs/reqwest), with its `json` feature, and [serde_json](https://docs.rs/serde_json):

```shell
cargo add reqwest --features json
cargo add serde_json
```

```rust
// Load the API key, base URL and workspace that API Keys Setup saved
let config = nominal::Config::load()?;
let profile = config.get_profile("default").ok_or("no profile named default")?;

let csv = reqwest::Client::new()
    .post(format!("{}/sql/v1/query", profile.base_url()))
    .bearer_auth(profile.token())
    .json(&serde_json::json!({
        "query": "SELECT dataset_rid, name FROM datasets LIMIT 20",
        "workspace_rid": profile.workspace_rid(),
        "result_format": "SQL_SERVICE_QUERY_RESULT_FORMAT_CSV",
    }))
    .send()
    .await?
    .error_for_status()?
    .text()
    .await?;
println!("{csv}");
```

[Query](https://dev.nominal.io/sql-reference/rest-api/query/) has the endpoint's limits and its other result formats.

## Export a large result

`/sql/v1/query/export` writes a query's whole result to a file, with no row or size limit, and returns a download URL:

```rust
let export: serde_json::Value = reqwest::Client::new()
    .post(format!("{}/sql/v1/query/export", profile.base_url()))
    .bearer_auth(profile.token())
    .json(&serde_json::json!({
        "query": "SELECT ts, channel, value FROM points_double WHERE dataset_rid = '<ri.catalog.instance.dataset.identifier>'",
        "workspace_rid": profile.workspace_rid(),
    }))
    .send()
    .await?
    .error_for_status()?
    .json()
    .await?;

// The download URL carries its own credentials, so keep it secret
let url = export["presignedUrl"].as_str().ok_or("no presignedUrl in the response")?;
std::fs::write("export.csv", reqwest::get(url).await?.error_for_status()?.bytes().await?)?;
```

[Export](https://dev.nominal.io/sql-reference/rest-api/export/) covers Parquet files.
