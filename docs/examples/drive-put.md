# Put a file in a drive

{.lead}
Upload a file to Nominal and place it at a path in a [drive](../guides/drives.md), through the REST API. The file goes through two steps: an upload into Nominal's file storage, then a change that places it in the drive.

Replace the drive's RID, the local file and its path in the drive with your own. The drive must be in your profile's workspace.

```shell
cargo add nominal serde_json
cargo add reqwest --features json
cargo add tokio --features full
```

```rust
use serde_json::{Value, json};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load the API key, base URL and workspace that API Keys Setup saved
    let config = nominal::Config::load()?;
    let profile = config.get_profile("default").ok_or("no profile named default")?;
    let workspace = profile.workspace_rid().ok_or("no workspace in the profile")?;
    let api = profile.base_url();
    let http = reqwest::Client::new();

    let drive_rid = "<ri.file-store.instance.drive.identifier>";
    let path = "reports/run-12.pdf"; // where the file goes in the drive

    // Start an upload into the file storage that drives use, in the drive's workspace
    let upload: Value = http
        .post(format!("{api}/upload/v1/multipart-upload"))
        .bearer_auth(profile.token())
        .json(&json!({
            "filename": "run-12.pdf",
            "filetype": "application/pdf",
            "workspace": workspace,
            "destination": "FILE_STORE",
        }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let upload_id = upload["uploadId"].as_str().ok_or("no uploadId")?;
    let key = upload["key"].as_str().ok_or("no key")?;
    let bucket = upload["bucket"].as_str().ok_or("no bucket")?;

    // Get a signed URL for the file's one part. One part holds up to 5 GB.
    let signed: Value = http
        .post(format!("{api}/upload/v1/multipart-upload/{upload_id}"))
        .bearer_auth(profile.token())
        .query(&[("key", key), ("partNumber", "1"), ("bucket", bucket)])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    // Send the file to the signed URL, with the headers the signature needs
    let mut request = http.put(signed["url"].as_str().ok_or("no url")?).body(std::fs::read("run-12.pdf")?);
    for (name, value) in signed["headers"].as_object().into_iter().flatten() {
        request = request.header(name, value.as_str().unwrap_or_default());
    }
    let part = request.send().await?.error_for_status()?;
    let etag = part.headers()["etag"].to_str()?.to_string();

    // Finish the upload
    http.post(format!("{api}/upload/v1/multipart-upload/{upload_id}/complete"))
        .bearer_auth(profile.token())
        .query(&[("key", key), ("bucket", bucket)])
        .json(&json!([{ "partNumber": 1, "etag": etag }]))
        .send()
        .await?
        .error_for_status()?;

    // Place the uploaded file at its path in the drive. This fails if a file is already there.
    let response: Value = http
        .post(format!("{api}/file-store/v1/files/apply-changes"))
        .bearer_auth(profile.token())
        .json(&json!({
            "driveRid": drive_rid,
            "changes": [{ "put": {
                "object": { "objectKey": key },
                "destination": { "path": { "path": { "path": path } } },
            } }],
        }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let result = &response["results"][0];
    if let Some(failure) = result.get("failure") {
        return Err(format!("couldn't place the file: {failure}").into());
    }
    println!("{}", result["success"]["file"]["path"]["path"].as_str().unwrap_or_default());
    Ok(())
}
```

To move, remove, list or download the file afterwards, see [Drives and files](../guides/drives.md).
