use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use conjure_http::client::{AsyncService, ConjureRuntime};
use conjure_object::BearerToken;
use conjure_runtime::Client;
use futures::{TryStreamExt, stream};
use nominal_api::clients::upload::api::{AsyncUploadService, AsyncUploadServiceClient};
use nominal_api::objects::api::rids::WorkspaceRid;
use nominal_api::objects::ingest::api::{InitiateMultipartUploadRequest, Part};
use tokio::io::{AsyncRead, AsyncReadExt};

use crate::core::ingest::options::UploadOptions;
use crate::core::ingest::progress::{ProgressCallback, UploadEvent};
use crate::core::rid::parse_rid;
use crate::{Error, Result};

/// Upload a local file to Nominal-backed object storage using multipart upload.
///
/// The file is streamed in chunks of `options.chunk_size` bytes. Each chunk is
/// signed via Nominal's upload service and PUT to the returned presigned URL,
/// with up to `options.max_concurrency` parts in flight. Failed PUTs are
/// retried up to `options.max_retries` times with exponential backoff and the
/// part is re-signed on each attempt. On any unrecoverable error the upload is
/// aborted server-side.
///
/// Returns the storage location (S3 path) of the completed upload.
pub(crate) async fn upload_file(
    conjure_client: Client,
    runtime: &Arc<ConjureRuntime>,
    token: BearerToken,
    workspace_rid: Option<String>,
    path: impl AsRef<Path>,
    filename: String,
    mimetype: String,
    options: UploadOptions,
) -> Result<String> {
    let file = tokio::fs::File::open(path.as_ref()).await?;
    let total_bytes = file.metadata().await?.len();
    upload_reader(
        conjure_client,
        runtime,
        token,
        workspace_rid,
        file,
        total_bytes,
        filename,
        mimetype,
        options,
    )
    .await
}

/// Upload an arbitrary async reader using Nominal's multipart upload.
///
/// `total_bytes` is used for progress reporting and to reject empty streams
/// (the multipart API requires at least one part).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn upload_reader<R>(
    conjure_client: Client,
    runtime: &Arc<ConjureRuntime>,
    token: BearerToken,
    workspace_rid: Option<String>,
    reader: R,
    total_bytes: u64,
    filename: String,
    mimetype: String,
    options: UploadOptions,
) -> Result<String>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    if total_bytes == 0 {
        return Err(Error::Upload {
            details: "cannot upload empty stream".into(),
        });
    }
    if options.chunk_size == 0 {
        return Err(Error::Upload {
            details: "chunk_size must be > 0".into(),
        });
    }
    if options.max_concurrency == 0 {
        return Err(Error::Upload {
            details: "max_concurrency must be > 0".into(),
        });
    }

    let upload_service = AsyncUploadServiceClient::new(conjure_client, runtime);
    let workspace = workspace_rid
        .as_deref()
        .map(parse_rid::<WorkspaceRid>)
        .transpose()?;

    let init_req = InitiateMultipartUploadRequest::builder()
        .filename(filename)
        .filetype(mimetype)
        .workspace(workspace)
        .build();
    let init_resp = upload_service
        .initiate_multipart_upload(&token, &init_req)
        .await?;
    let key = init_resp.key().to_string();
    let upload_id = init_resp.upload_id().to_string();

    let total_parts = total_bytes.div_ceil(options.chunk_size as u64) as u32;
    emit(&options.progress, || UploadEvent::Started {
        total_bytes,
        total_parts,
    });

    let http = reqwest::Client::builder()
        .pool_max_idle_per_host(options.max_concurrency)
        .build()
        .map_err(|e| Error::Upload {
            details: format!("failed to build HTTP client: {e}"),
        })?;

    let ctx = PartCtx {
        upload_service: &upload_service,
        token: &token,
        http: &http,
        key: &key,
        upload_id: &upload_id,
        options: &options,
    };
    let result = upload_all_parts(&ctx, reader).await;

    match result {
        Ok(mut parts) => {
            // S3 CompleteMultipartUpload requires parts sorted by part number.
            parts.sort_by_key(|(n, _)| *n);
            let parts: Vec<Part> = parts
                .into_iter()
                .map(|(n, etag)| Part::new(n, etag))
                .collect();
            let complete_resp = upload_service
                .complete_multipart_upload(&token, &upload_id, &key, &parts)
                .await?;
            let location = complete_resp
                .location()
                .ok_or_else(|| Error::Upload {
                    details: "multipart upload completion returned no location".into(),
                })?
                .to_string();
            emit(&options.progress, || UploadEvent::Completed {
                s3_path: location.clone(),
            });
            Ok(location)
        }
        Err(e) => {
            // Best-effort abort; surface the original error regardless.
            let _ = upload_service
                .abort_multipart_upload(&token, &upload_id, &key)
                .await;
            Err(e)
        }
    }
}

/// Everything needed to sign and PUT an individual part.
struct PartCtx<'a> {
    upload_service: &'a AsyncUploadServiceClient<Client>,
    token: &'a BearerToken,
    http: &'a reqwest::Client,
    key: &'a str,
    upload_id: &'a str,
    options: &'a UploadOptions,
}

/// Owned version of [`PartCtx`] used inside per-part async tasks.
#[derive(Clone)]
struct OwnedPartCtx {
    upload_service: AsyncUploadServiceClient<Client>,
    token: BearerToken,
    http: reqwest::Client,
    key: String,
    upload_id: String,
    options: UploadOptions,
}

impl PartCtx<'_> {
    fn to_owned(&self) -> OwnedPartCtx {
        OwnedPartCtx {
            upload_service: self.upload_service.clone(),
            token: self.token.clone(),
            http: self.http.clone(),
            key: self.key.to_string(),
            upload_id: self.upload_id.to_string(),
            options: self.options.clone(),
        }
    }
}

async fn upload_all_parts<R>(ctx: &PartCtx<'_>, reader: R) -> Result<Vec<(i32, String)>>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    let chunks = stream::unfold(
        (reader, 1i32, ctx.options.chunk_size),
        |(mut reader, part, chunk_size)| async move {
            let mut buf = vec![0u8; chunk_size];
            let mut total = 0;
            while total < chunk_size {
                match reader.read(&mut buf[total..]).await {
                    Ok(0) => break,
                    Ok(n) => total += n,
                    Err(e) => {
                        return Some((Err(Error::from(e)), (reader, part, chunk_size)));
                    }
                }
            }
            if total == 0 {
                return None;
            }
            buf.truncate(total);
            Some((
                Ok((part, Bytes::from(buf))),
                (reader, part + 1, chunk_size),
            ))
        },
    );

    let max_concurrency = ctx.options.max_concurrency;
    chunks
        .map_ok(|(part_number, bytes)| {
            let ctx = ctx.to_owned();
            async move {
                let etag = sign_and_put(&ctx, part_number, bytes).await?;
                Ok::<_, Error>((part_number, etag))
            }
        })
        .try_buffer_unordered(max_concurrency)
        .try_collect()
        .await
}

async fn sign_and_put(ctx: &OwnedPartCtx, part_number: i32, bytes: Bytes) -> Result<String> {
    let part_size = bytes.len() as u64;
    let mut last_err: Option<Error> = None;
    for attempt in 0..=ctx.options.max_retries {
        match put_once(ctx, part_number, bytes.clone()).await {
            Ok(etag) => {
                emit(&ctx.options.progress, || UploadEvent::PartCompleted {
                    part_number: part_number as u32,
                    bytes: part_size,
                });
                return Ok(etag);
            }
            Err(e) => {
                last_err = Some(e);
                if attempt < ctx.options.max_retries {
                    // 100ms, 200ms, 400ms, ...
                    let backoff = Duration::from_millis(100u64 << attempt);
                    tokio::time::sleep(backoff).await;
                }
            }
        }
    }
    Err(last_err.unwrap_or_else(|| Error::Upload {
        details: format!("failed to upload part {part_number}"),
    }))
}

async fn put_once(ctx: &OwnedPartCtx, part_number: i32, bytes: Bytes) -> Result<String> {
    // Re-sign on every attempt: presigned URLs can expire between retries.
    let sign_resp = ctx
        .upload_service
        .sign_part(&ctx.token, &ctx.upload_id, &ctx.key, part_number)
        .await?;
    let mut req = ctx.http.put(sign_resp.url()).body(bytes);
    for (k, v) in sign_resp.headers() {
        req = req.header(k, v);
    }
    let resp = req.send().await.map_err(|e| Error::Upload {
        details: format!("PUT part {part_number} failed: {e}"),
    })?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(Error::Upload {
            details: format!("PUT part {part_number} returned {status}: {body}"),
        });
    }
    // S3 returns the etag wrapped in double quotes per RFC; CompleteMultipartUpload
    // normalizes but list_parts returns the bare value, so strip to match.
    let etag = resp
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| Error::Upload {
            details: format!("PUT part {part_number} response missing ETag header"),
        })?
        .trim_matches('"')
        .to_string();
    Ok(etag)
}

fn emit(callback: &Option<ProgressCallback>, event: impl FnOnce() -> UploadEvent) {
    if let Some(cb) = callback {
        cb(event());
    }
}
