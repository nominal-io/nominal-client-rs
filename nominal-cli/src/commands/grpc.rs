use anyhow::{Context, bail};
use bytes::{BufMut, BytesMut};
use prost::Message;
use prost_reflect::{DescriptorPool, DynamicMessage, MethodDescriptor, ServiceDescriptor};

fn pool() -> &'static DescriptorPool {
    super::descriptor_pool()
}

/// Call a gRPC method by its full path (`pkg.ServiceName/MethodName`).
/// Exposed for use by `nomctl api` auto-routing.
pub async fn call_grpc_method(
    method_path: &str,
    body_json: &str,
    base_url: &str,
    token: &str,
    dry_run: bool,
) -> anyhow::Result<()> {
    let method_desc = resolve_method(pool(), method_path)
        .with_context(|| format!("unknown gRPC method `{method_path}`"))?;

    let grpc_path = format!(
        "/{}/{}",
        method_desc.parent_service().full_name(),
        method_desc.name()
    );

    if dry_run {
        let host_root = host_root(base_url);
        println!("DRY RUN");
        println!("  protocol : grpc");
        println!("  method   : {method_path}");
        println!("  url      : {host_root}{grpc_path}");
        println!("  body     : {body_json}");
        return Ok(());
    }

    let mut deser = serde_json::Deserializer::from_str(body_json);
    let request_msg = DynamicMessage::deserialize(method_desc.input(), &mut deser)
        .context("failed to parse request JSON into proto message")?;
    let request_bytes = request_msg.encode_to_vec();

    let response_bytes = grpc_unary(base_url, &grpc_path, request_bytes, token).await?;

    let response_msg = DynamicMessage::decode(method_desc.output(), response_bytes.as_ref())
        .context("failed to decode gRPC response")?;
    let json =
        serde_json::to_string_pretty(&response_msg).context("failed to serialize response")?;
    println!("{json}");

    Ok(())
}

async fn grpc_unary(
    base_url: &str,
    grpc_path: &str,
    request_bytes: Vec<u8>,
    token: &str,
) -> anyhow::Result<bytes::Bytes> {
    let mut framed = BytesMut::with_capacity(5 + request_bytes.len());
    framed.put_u8(0);
    framed.put_u32(request_bytes.len() as u32);
    framed.put_slice(&request_bytes);

    let url = format!(
        "{}/{}",
        host_root(base_url),
        grpc_path.trim_start_matches('/')
    );

    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .bearer_auth(token)
        .header("content-type", "application/grpc+proto")
        .header("te", "trailers")
        .body(framed.freeze())
        .send()
        .await
        .context("gRPC request failed")?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        bail!("HTTP {status}: {body}");
    }

    let body = response
        .bytes()
        .await
        .context("failed to read gRPC response")?;

    if body.len() < 5 {
        bail!("gRPC response too short ({} bytes)", body.len());
    }
    let msg_len = u32::from_be_bytes([body[1], body[2], body[3], body[4]]) as usize;
    if body.len() < 5 + msg_len {
        bail!(
            "gRPC response frame truncated: expected {} bytes, got {}",
            5 + msg_len,
            body.len()
        );
    }

    Ok(body.slice(5..5 + msg_len))
}

pub(crate) fn find_service(pool: &DescriptorPool, name: &str) -> Option<ServiceDescriptor> {
    pool.get_service_by_name(name)
}

fn resolve_method(pool: &DescriptorPool, path: &str) -> Option<MethodDescriptor> {
    let (svc_part, method_name) = path.split_once('/')?;
    let svc = find_service(pool, svc_part)?;
    svc.methods().find(|m| m.name() == method_name)
}

fn host_root(base_url: &str) -> String {
    url::Url::parse(base_url)
        .ok()
        .map(|u| format!("{}://{}", u.scheme(), u.host_str().unwrap_or_default()))
        .unwrap_or_else(|| base_url.trim_end_matches('/').to_owned())
}
