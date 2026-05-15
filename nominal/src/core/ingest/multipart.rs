use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use conjure_http::client::{AsyncService, ConjureRuntime};
use conjure_object::BearerToken;
use conjure_runtime::Client;
use conjure_runtime::crypto::ring_crypto_provider;
use futures::{Stream, StreamExt, TryStreamExt, stream};
use nominal_api::clients::upload::api::{AsyncUploadService, AsyncUploadServiceClient};
use nominal_api::objects::api::rids::WorkspaceRid;
use nominal_api::objects::ingest::api::{InitiateMultipartUploadRequest, Part};
use rustls::client::ResolvesClientCert;
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
#[allow(clippy::too_many_arguments)]
pub(crate) async fn upload_file(
    conjure_client: Client,
    runtime: &Arc<ConjureRuntime>,
    token: BearerToken,
    workspace_rid: Option<String>,
    path: impl AsRef<Path>,
    filename: String,
    mimetype: String,
    options: UploadOptions,
    tls_resolver: Option<Arc<dyn ResolvesClientCert>>,
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
        tls_resolver,
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
    tls_resolver: Option<Arc<dyn ResolvesClientCert>>,
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

    let http = build_http_client(&options, tls_resolver)?;

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

fn build_http_client(
    options: &UploadOptions,
    tls_resolver: Option<Arc<dyn ResolvesClientCert>>,
) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder().pool_max_idle_per_host(options.max_concurrency);
    if let Some(resolver) = tls_resolver {
        let mut root_store = rustls::RootCertStore::empty();
        root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let tls = rustls::ClientConfig::builder_with_provider(ring_crypto_provider().clone())
            .with_safe_default_protocol_versions()
            .map_err(|e| Error::Tls {
                details: format!("TLS protocol-version config: {e}"),
            })?
            .with_root_certificates(root_store)
            .with_client_cert_resolver(resolver);
        builder = builder.use_preconfigured_tls(tls);
    }
    builder.build().map_err(|e| Error::Upload {
        details: format!("failed to build HTTP client: {e}"),
    })
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
    let max_concurrency = ctx.options.max_concurrency;
    chunk_stream(reader, ctx.options.chunk_size)
        .enumerate()
        .map(|(i, res)| res.map(|bytes| (i as i32 + 1, bytes)))
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

/// Stream an `AsyncRead` as `chunk_size`-sized `Bytes`. The final chunk may be
/// shorter; EOF ends the stream. A read error ends the stream after yielding
/// the error (via `try_unfold`, which drops the reader on failure).
fn chunk_stream<R>(reader: R, chunk_size: usize) -> impl Stream<Item = Result<Bytes>>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    stream::try_unfold(reader, move |mut reader| async move {
        let mut buf = BytesMut::with_capacity(chunk_size);
        while buf.len() < chunk_size {
            if reader.read_buf(&mut buf).await? == 0 {
                break;
            }
        }
        Ok((!buf.is_empty()).then(|| (buf.freeze(), reader)))
    })
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
    let etag = resp
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| Error::Upload {
            details: format!("PUT part {part_number} response missing ETag header"),
        })?
        .to_string();
    Ok(etag)
}

fn emit(callback: &Option<ProgressCallback>, event: impl FnOnce() -> UploadEvent) {
    if let Some(cb) = callback {
        cb(event());
    }
}
