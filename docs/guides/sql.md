# Query data with SQL

{.lead}
Read your data back from Nominal with SQL, through the [SQL REST API](https://dev.nominal.io/sql-reference/rest-api/). [Writing queries](https://dev.nominal.io/sql-guides/writing-queries/) covers the tables you can query and the SQL they accept.

To connect a Postgres-compatible driver such as tokio-postgres instead, see [Postgres clients](https://dev.nominal.io/sql-guides/postgres-clients/).

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

// Send the query, and fail on an error status
let response = reqwest::Client::new()
    .post(format!("{}/sql/v1/query", profile.base_url()))
    .bearer_auth(profile.token())
    .json(&serde_json::json!({
        "query": "SELECT dataset_rid, name FROM datasets LIMIT 20",
        "workspace_rid": profile.workspace_rid(),
        "result_format": "SQL_SERVICE_QUERY_RESULT_FORMAT_CSV",
    }))
    .send()
    .await?
    .error_for_status()?;

// Read the result: CSV with a header row
let csv = response.text().await?;
println!("{csv}");
```

[Query](https://dev.nominal.io/sql-reference/rest-api/query/) has the endpoint's limits and its other return formats.

## Export a large result

`/sql/v1/query/export` writes a query's whole result to a file, with no row or size limit, and returns a download URL:

```rust
// Send the export request, and fail on an error status
let response = reqwest::Client::new()
    .post(format!("{}/sql/v1/query/export", profile.base_url()))
    .bearer_auth(profile.token())
    .json(&serde_json::json!({
        "query": "SELECT ts, channel, value FROM points_double WHERE dataset_rid = '<ri.catalog.instance.dataset.identifier>'",
        "workspace_rid": profile.workspace_rid(),
        "format": "SQL_SERVICE_EXPORT_FORMAT_CSV", // or SQL_SERVICE_EXPORT_FORMAT_PARQUET
    }))
    .send()
    .await?
    .error_for_status()?;

// Read the download URL. It carries its own credentials, so keep it secret.
let export: serde_json::Value = response.json().await?;
let url = export["presignedUrl"].as_str().ok_or("no presignedUrl in the response")?;

// Download the file
let download = reqwest::get(url).await?.error_for_status()?;
std::fs::write("export.csv", download.bytes().await?)?;
```

The export runs the whole query before it returns the URL, so a large query can take a while. The URL works for a limited time. When it expires, http code 403 Forbidden is returned and you will have to perform the export again.

Export can also generate a Parquet file. To enable, change `format` in the request to `SQL_SERVICE_EXPORT_FORMAT_PARQUET`, and save the file as `export.parquet`. [Export](https://dev.nominal.io/sql-reference/rest-api/export/) covers Parquet's compression and row group options.
