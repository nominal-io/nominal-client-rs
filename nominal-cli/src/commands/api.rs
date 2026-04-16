use anyhow::{Context, bail};
use clap::Args;
use prost_reflect::DynamicMessage;

pub(crate) mod generated {
    include!(concat!(env!("OUT_DIR"), "/conjure_endpoints.rs"));
    include!(concat!(env!("OUT_DIR"), "/grpc_http_endpoints.rs"));
}

pub(crate) use generated::{
    CONJURE_ENDPOINTS, ConjureEndpoint, GRPC_HTTP_ENDPOINTS, GrpcHttpEndpoint,
};

#[derive(Args)]
pub struct ApiArgs {
    /// REST path (/scout/v1/run/rid) or gRPC method (pkg.Service/Method)
    pub target: String,

    /// JSON request body
    pub body: Option<String>,

    /// Override HTTP method (GET, POST, PUT, DELETE, PATCH)
    #[arg(short = 'X', long, alias = "request")]
    pub method: Option<String>,

    /// Force Conjure (REST) routing
    #[arg(long, conflicts_with_all = ["grpc_http", "grpc"])]
    pub conjure: bool,

    /// Force gRPC-over-HTTP routing
    #[arg(long, conflicts_with_all = ["conjure", "grpc"])]
    pub grpc_http: bool,

    /// Force native gRPC routing
    #[arg(long, conflicts_with_all = ["conjure", "grpc_http"])]
    pub grpc: bool,

    /// Print the matched endpoint and exit without sending the request
    #[arg(long)]
    pub dry_run: bool,
}

pub async fn handle(args: ApiArgs, profile: nominal::Profile) -> anyhow::Result<()> {
    let base_url = profile.base_url();
    let token = profile.token();
    let body = args.body.as_deref();
    let method_override = args.method.as_deref().map(|m| m.to_uppercase());

    // Detect protocol from input shape or force flags
    if args.grpc || (!args.conjure && !args.grpc_http && is_grpc_method(&args.target)) {
        return super::grpc::call_grpc_method(
            &args.target,
            body.unwrap_or("{}"),
            base_url,
            token,
            args.dry_run,
        )
        .await;
    }

    let normalized = normalize_path(&args.target, base_url);
    let path = strip_query(&normalized);

    let has_body = body.is_some();

    if args.conjure {
        let ep = find_conjure(path, method_override.as_deref(), has_body)?;
        return call_conjure(ep, &args.target, base_url, body, args.dry_run, token).await;
    }

    if args.grpc_http {
        let ep = find_grpc_http(path, method_override.as_deref(), has_body)?;
        return call_grpc_http(ep, &args.target, base_url, body, args.dry_run, token).await;
    }

    // Auto: gRPC-HTTP first, conjure fallback
    match find_grpc_http(path, method_override.as_deref(), has_body) {
        Ok(ep) => call_grpc_http(ep, &args.target, base_url, body, args.dry_run, token).await,
        Err(grpc_http_err) => match find_conjure(path, method_override.as_deref(), has_body) {
            Ok(ep) => call_conjure(ep, &args.target, base_url, body, args.dry_run, token).await,
            Err(conjure_err) => bail!(
                "no matching endpoint for `{path}`\n\
                 gRPC-HTTP: {grpc_http_err}\n\
                 Conjure:   {conjure_err}"
            ),
        },
    }
}

/// Returns true if the input looks like a gRPC method path: `pkg.Service/Method`
/// (contains a dot, then a slash, and does not start with `/`).
pub(crate) fn is_grpc_method(input: &str) -> bool {
    !input.starts_with('/') && input.contains('.') && input.contains('/')
}

// ── Path utilities ────────────────────────────────────────────────────────────

/// Extract just the path (no scheme/host/query) and strip the base URL's path prefix.
pub(crate) fn normalize_path(input: &str, base_url: &str) -> String {
    let path: String = if input.contains("://") {
        url::Url::parse(input)
            .map(|u| u.path().to_owned())
            .unwrap_or_else(|_| input.to_owned())
    } else {
        input.to_owned()
    };

    if let Ok(base) = url::Url::parse(base_url) {
        let base_path = base.path().trim_end_matches('/');
        if !base_path.is_empty() {
            if let Some(stripped) = path.strip_prefix(base_path) {
                return stripped.to_owned();
            }
        }
    }
    path
}

fn strip_query(path: &str) -> &str {
    path.split('?').next().unwrap_or(path)
}

/// Match a concrete path against a `{param}`-style template.
pub(crate) fn path_matches(template: &str, path: &str) -> bool {
    let t: Vec<&str> = template.split('/').filter(|s| !s.is_empty()).collect();
    let p: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if t.len() != p.len() {
        return false;
    }
    t.iter()
        .zip(p.iter())
        .all(|(t_seg, p_seg)| (t_seg.starts_with('{') && t_seg.ends_with('}')) || t_seg == p_seg)
}

// ── Endpoint lookup ───────────────────────────────────────────────────────────

fn find_conjure<'a>(
    path: &str,
    method_override: Option<&str>,
    has_body: bool,
) -> anyhow::Result<&'a ConjureEndpoint> {
    let candidates: Vec<&ConjureEndpoint> = CONJURE_ENDPOINTS
        .iter()
        .filter(|e| path_matches(e.path_template, path))
        .collect();

    pick_endpoint_by_method(candidates, method_override, has_body, path)
}

fn find_grpc_http<'a>(
    path: &str,
    method_override: Option<&str>,
    has_body: bool,
) -> anyhow::Result<&'a GrpcHttpEndpoint> {
    let candidates: Vec<&GrpcHttpEndpoint> = GRPC_HTTP_ENDPOINTS
        .iter()
        .filter(|e| path_matches(e.path_template, path))
        .collect();

    pick_endpoint_by_method(candidates, method_override, has_body, path)
}

trait HasMethod {
    fn http_method(&self) -> &str;
}
impl HasMethod for ConjureEndpoint {
    fn http_method(&self) -> &str {
        self.method
    }
}
impl HasMethod for GrpcHttpEndpoint {
    fn http_method(&self) -> &str {
        self.method
    }
}

fn pick_endpoint_by_method<'a, E: HasMethod>(
    candidates: Vec<&'a E>,
    method_override: Option<&str>,
    has_body: bool,
    path: &str,
) -> anyhow::Result<&'a E> {
    if candidates.is_empty() {
        bail!("no endpoint matches path `{path}`");
    }
    if candidates.len() == 1 {
        return Ok(candidates[0]);
    }

    let Some(method) = method_override.or(if !has_body { Some("GET") } else { None }) else {
        let methods: Vec<&str> = candidates.iter().map(|e| e.http_method()).collect();
        bail!(
            "path `{path}` matches multiple methods ({}); use -X to specify",
            methods.join(", ")
        )
    };

    let filtered: Vec<&&'a E> = candidates
        .iter()
        .filter(|e| e.http_method().eq_ignore_ascii_case(method))
        .collect();

    match filtered.len() {
        1 => Ok(filtered[0]),
        0 => {
            let methods: Vec<&str> = candidates.iter().map(|e| e.http_method()).collect();
            bail!(
                "path `{path}` matches {} but not `{method}`",
                methods.join(", ")
            )
        }
        _ => bail!("ambiguous: multiple `{method}` endpoints match `{path}`"),
    }
}

// ── Conjure call ──────────────────────────────────────────────────────────────

async fn call_conjure(
    ep: &ConjureEndpoint,
    original_input: &str,
    base_url: &str,
    body: Option<&str>,
    dry_run: bool,
    token: &str,
) -> anyhow::Result<()> {
    let url = resolve_url(original_input, base_url);

    if let (Some(validate), Some(json)) = (ep.validate_body, body) {
        validate(json).map_err(|e| anyhow::anyhow!("request body validation failed: {e}"))?;
    }

    if dry_run {
        println!("DRY RUN");
        println!("  protocol : conjure");
        println!("  endpoint : {}.{}", ep.service, ep.name);
        println!("  method   : {}", ep.method);
        println!("  url      : {url}");
        if let Some(b) = body {
            println!("  body     : {b}");
        }
        return Ok(());
    }

    send_request(ep.method, &url, body, token).await
}

// ── gRPC-HTTP validation ──────────────────────────────────────────────────────

fn validate_grpc_http_body(ep: &GrpcHttpEndpoint, json: &str) -> anyhow::Result<()> {
    let pool = super::descriptor_pool();

    let svc = pool
        .get_service_by_name(ep.service)
        .with_context(|| format!("proto service `{}` not found in descriptor", ep.service))?;

    let method = svc
        .methods()
        .find(|m| m.name() == ep.rpc)
        .with_context(|| format!("proto method `{}` not found in `{}`", ep.rpc, ep.service))?;

    let mut deser = serde_json::Deserializer::from_str(json);
    DynamicMessage::deserialize(method.input(), &mut deser).with_context(|| {
        format!(
            "request body does not match `{}`",
            method.input().full_name()
        )
    })?;

    Ok(())
}

// ── gRPC-HTTP call ────────────────────────────────────────────────────────────

async fn call_grpc_http(
    ep: &GrpcHttpEndpoint,
    original_input: &str,
    base_url: &str,
    body: Option<&str>,
    dry_run: bool,
    token: &str,
) -> anyhow::Result<()> {
    let url = resolve_url(original_input, base_url);

    if let Some(json) = body {
        validate_grpc_http_body(ep, json)?;
    }

    if dry_run {
        println!("DRY RUN");
        println!("  protocol    : grpc-http");
        println!("  endpoint    : {}.{}", ep.service, ep.rpc);
        println!("  method      : {}", ep.method);
        println!("  url         : {url}");
        if let Some(b) = ep.body {
            println!("  body field  : {b}");
        }
        if let Some(rb) = ep.response_body {
            println!("  resp field  : {rb}");
        }
        if let Some(b) = body {
            println!("  body        : {b}");
        }
        return Ok(());
    }

    send_request(ep.method, &url, body, token).await
}

// ── Shared HTTP logic ─────────────────────────────────────────────────────────

pub(crate) fn resolve_url(input: &str, base_url: &str) -> String {
    if input.contains("://") {
        input.to_owned()
    } else {
        let base = base_url.trim_end_matches('/');
        let path = input.trim_start_matches('/');
        format!("{base}/{path}")
    }
}

pub(crate) async fn send_request(
    method: &str,
    url: &str,
    body: Option<&str>,
    token: &str,
) -> anyhow::Result<()> {
    let client = reqwest::Client::new();

    let mut builder = match method.to_uppercase().as_str() {
        "GET" => client.get(url),
        "POST" => client.post(url),
        "PUT" => client.put(url),
        "DELETE" => client.delete(url),
        "PATCH" => client.patch(url),
        other => bail!("unsupported HTTP method: {other}"),
    };

    builder = builder.bearer_auth(token);

    if let Some(json) = body {
        builder = builder
            .header("Content-Type", "application/json")
            .body(json.to_owned());
    }

    let response = builder.send().await.context("failed to send request")?;
    let status = response.status();
    let text = response
        .text()
        .await
        .context("failed to read response body")?;

    if !status.is_success() {
        bail!("HTTP {status}\n{text}");
    }

    if text.is_empty() {
        return Ok(());
    }

    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(v) => println!("{}", serde_json::to_string_pretty(&v)?),
        Err(_) => println!("{text}"),
    }

    Ok(())
}
