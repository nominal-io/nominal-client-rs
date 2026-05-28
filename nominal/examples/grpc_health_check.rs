use std::sync::Arc;

use bytes::{BufMut, BytesMut};
use nominal::Config;
use nominal::smartcard::SmartcardCertResolver;
use prost::Message;
use rustls_platform_verifier::BuilderVerifierExt;

// MeshService is known to be registered as a native gRPC service on this host
// (other services return nginx 404; MeshService returned grpc-status:12 for an
// unregistered method, which proves the gRPC handler is live).
// Mesh is the core binary-data operation with no HTTP-only fallback.
const GRPC_METHOD: &str = "nominal.mesh.v1.MeshService/Mesh";

// oneof request { DataStreamRequest = 1; FileIngestRequest = 2; }
// Sending with no variant set is valid protobuf; server will reject it with
// INVALID_ARGUMENT, but that is a proper gRPC response and proves the transport.
#[derive(Message)]
struct MeshRequest {}

#[derive(Message)]
struct MeshResponse {}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        if !matches!(e, nominal::Error::Tls { .. }) {
            eprintln!("error: {e}");
        }
        std::process::exit(1);
    }
}

async fn run() -> nominal::Result<()> {
    let config = Config::load()?;
    let profile = config
        .get_profile("cac_staging")
        .expect("profile 'cac_staging' not found");

    // NOMINAL_PKCS11_MODULE env var is picked up automatically
    let resolver = Arc::new(SmartcardCertResolver::new()?);

    let mut tls = rustls::ClientConfig::builder_with_provider(
        conjure_runtime::crypto::ring_crypto_provider().clone(),
    )
    .with_safe_default_protocol_versions()
    .map_err(|e| nominal::Error::Tls {
        details: format!("TLS protocol-version config: {e}"),
    })?
    .with_platform_verifier()
    .map_err(|e| nominal::Error::Tls {
        details: format!("platform verifier: {e}"),
    })?
    .with_client_cert_resolver(resolver);

    // gRPC requires HTTP/2; set ALPN so it is negotiated during the TLS handshake.
    tls.alpn_protocols = vec![b"h2".to_vec()];

    let http = reqwest::Client::builder()
        .use_preconfigured_tls(tls)
        .build()
        .map_err(|e| nominal::Error::Tls {
            details: format!("build HTTP client: {e}"),
        })?;

    let host_root = host_root(profile.base_url());
    let url = format!("{host_root}/{GRPC_METHOD}");
    println!("POST {url}");

    let request_bytes = MeshRequest {}.encode_to_vec();
    let mut frame = BytesMut::with_capacity(5 + request_bytes.len());
    frame.put_u8(0); // compressed-flag = 0
    frame.put_u32(request_bytes.len() as u32);
    frame.put_slice(&request_bytes);

    let response = http
        .post(&url)
        .bearer_auth(profile.token())
        .header("content-type", "application/grpc+proto")
        .header("te", "trailers")
        .body(frame.freeze())
        .send()
        .await
        .map_err(|e| nominal::Error::Tls {
            details: e.to_string(),
        })?;

    let http_status = response.status();
    let grpc_status = response
        .headers()
        .get("grpc-status")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("0")
        .to_owned();
    let grpc_message = response
        .headers()
        .get("grpc-message")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();

    let body = response.bytes().await.map_err(|e| nominal::Error::Tls {
        details: e.to_string(),
    })?;

    println!("HTTP {http_status}  grpc-status: {grpc_status}");

    if !http_status.is_success() {
        eprintln!("{}", String::from_utf8_lossy(&body));
        std::process::exit(1);
    }

    if body.len() >= 5 {
        let msg_len = u32::from_be_bytes([body[1], body[2], body[3], body[4]]) as usize;
        if body.len() >= 5 + msg_len && grpc_status == "0" {
            let payload = &body[5..5 + msg_len];
            MeshResponse::decode(payload).map_err(|e| nominal::Error::Tls {
                details: format!("proto decode: {e}"),
            })?;
            println!("gRPC round-trip OK ({msg_len} response bytes)");
        }
    }

    if grpc_status != "0" {
        // A gRPC-level error (e.g. INVALID_ARGUMENT=3) still proves the transport works.
        println!("gRPC transport OK — server returned status {grpc_status}: {grpc_message}");
    }

    Ok(())
}

/// Strip the path from a base URL to get the scheme+host root.
/// e.g. "https://api.gov.nominal.io/api" → "https://api.gov.nominal.io"
fn host_root(base_url: &str) -> String {
    if let Some(after_scheme) = base_url.find("://").map(|i| i + 3) {
        let rest = &base_url[after_scheme..];
        let host_end = rest.find('/').unwrap_or(rest.len());
        let scheme = &base_url[..after_scheme];
        format!("{scheme}{}", &rest[..host_end])
    } else {
        base_url.trim_end_matches('/').to_owned()
    }
}
