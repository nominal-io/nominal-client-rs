use std::{env, fmt::Write as FmtWrite, fs, path::PathBuf};

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    let meta = cargo_metadata::MetadataCommand::new()
        .exec()
        .expect("failed to run `cargo metadata`");

    let nominal_api = meta
        .packages
        .iter()
        .find(|p| p.name == "nominal-api")
        .expect("nominal-api not found in dependency tree");

    let crate_root = nominal_api
        .manifest_path
        .parent()
        .expect("manifest_path has no parent");

    let conjure_json = crate_root.join("definitions/conjure/scout-service-api.conjure.json");
    let protos_dir = crate_root.join("definitions/protos");
    let includes_dir = crate_root.join("definitions/proto-includes");

    println!("cargo:rerun-if-changed={}", conjure_json);
    println!("cargo:rerun-if-changed={}", protos_dir);

    generate_conjure_endpoints(conjure_json.as_std_path(), &out_dir);
    generate_grpc_http_endpoints(protos_dir.as_std_path(), &out_dir);
    generate_proto_descriptor(
        protos_dir.as_std_path(),
        includes_dir.as_std_path(),
        &out_dir,
    );
}

fn generate_conjure_endpoints(json_path: &std::path::Path, out_dir: &PathBuf) {
    let raw = fs::read_to_string(json_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", json_path.display()));
    let def: serde_json::Value = serde_json::from_str(&raw).expect("failed to parse conjure JSON");

    let mut entries = String::new();

    for svc in def["services"].as_array().expect("services array") {
        let svc_name = svc["serviceName"]["name"].as_str().unwrap();

        for ep in svc["endpoints"].as_array().expect("endpoints array") {
            let ep_name = ep["endpointName"].as_str().unwrap();
            let method = ep["httpMethod"].as_str().unwrap();
            let path = ep["httpPath"].as_str().unwrap();

            let body_arg = ep["args"]
                .as_array()
                .and_then(|args| args.iter().find(|a| a["paramType"]["type"] == "body"));

            let validate_body = match body_arg {
                None => "None".to_owned(),
                Some(arg) => match conjure_type_to_rust(&arg["type"]) {
                    Ok(rust_ty) => format!(
                        "Some(|s: &str| -> ::std::result::Result<(), ::std::string::String> {{ \
                            ::serde_json::from_str::<{rust_ty}>(s)\
                                .map(|_| ())\
                                .map_err(|e| e.to_string()) \
                        }})"
                    ),
                    // Unknown type – skip validation rather than failing the build
                    Err(_) => "None".to_owned(),
                },
            };

            writeln!(
                entries,
                "    ConjureEndpoint {{ service: {svc_name:?}, name: {ep_name:?}, \
                method: {method:?}, path_template: {path:?}, validate_body: {validate_body} }},",
            )
            .unwrap();
        }
    }

    let src = format!(
        "pub struct ConjureEndpoint {{\n\
            pub service: &'static str,\n\
            pub name: &'static str,\n\
            pub method: &'static str,\n\
            pub path_template: &'static str,\n\
            pub validate_body: Option<fn(&str) -> ::std::result::Result<(), ::std::string::String>>,\n\
        }}\n\n\
        pub static CONJURE_ENDPOINTS: &[ConjureEndpoint] = &[\n{entries}];\n"
    );

    fs::write(out_dir.join("conjure_endpoints.rs"), src)
        .expect("failed to write conjure_endpoints.rs");
}

/// Maps a conjure type JSON node to the fully-qualified Rust type string.
fn conjure_type_to_rust(ty: &serde_json::Value) -> Result<String, String> {
    match ty["type"].as_str().unwrap_or("") {
        "reference" => {
            let pkg = ty["reference"]["package"]
                .as_str()
                .ok_or("missing package")?;
            let name = ty["reference"]["name"].as_str().ok_or("missing name")?;
            // strip_prefix("io.nominal.") – matches what conjure_codegen does with
            // Config::new().strip_prefix("io.nominal")
            let module = pkg
                .strip_prefix("io.nominal.")
                .unwrap_or(pkg)
                .replace('.', "::");
            Ok(format!("::nominal_api::objects::{module}::{name}"))
        }
        "set" => Ok(format!(
            "::std::collections::BTreeSet<{}>",
            conjure_type_to_rust(&ty["set"]["itemType"])?
        )),
        "list" => Ok(format!(
            "::std::vec::Vec<{}>",
            conjure_type_to_rust(&ty["list"]["itemType"])?
        )),
        "map" => Ok(format!(
            "::std::collections::BTreeMap<{}, {}>",
            conjure_type_to_rust(&ty["map"]["keyType"])?,
            conjure_type_to_rust(&ty["map"]["valueType"])?
        )),
        "optional" => Ok(format!(
            "::std::option::Option<{}>",
            conjure_type_to_rust(&ty["optional"]["itemType"])?
        )),
        "primitive" => match ty["primitive"].as_str().unwrap_or("") {
            "STRING" => Ok("::std::string::String".into()),
            "INTEGER" => Ok("i32".into()),
            "DOUBLE" => Ok("f64".into()),
            "BOOLEAN" => Ok("bool".into()),
            "RID" => Ok("::conjure_object::ResourceIdentifier".into()),
            "SAFELONG" => Ok("::conjure_object::SafeLong".into()),
            "DATETIME" => Ok("::conjure_object::DateTime<::conjure_object::Utc>".into()),
            "UUID" => Ok("::conjure_object::Uuid".into()),
            "BEARERTOKEN" => Ok("::conjure_object::BearerToken".into()),
            "BINARY" | "ANY" => Ok("::serde_json::Value".into()),
            other => Err(format!("unknown primitive: {other}")),
        },
        other => Err(format!("unknown conjure type kind: {other}")),
    }
}

fn generate_grpc_http_endpoints(protos_dir: &std::path::Path, out_dir: &PathBuf) {
    let mut entries = String::new();

    // Walk every .proto file and parse google.api.http annotations
    let proto_files = collect_proto_files(protos_dir);
    for proto_file in &proto_files {
        let content = match fs::read_to_string(proto_file) {
            Ok(c) => c,
            Err(_) => continue,
        };
        parse_grpc_http_endpoints(&content, &mut entries);
    }

    let src = format!(
        "pub struct GrpcHttpEndpoint {{\n\
            pub service: &'static str,\n\
            pub rpc: &'static str,\n\
            pub method: &'static str,\n\
            pub path_template: &'static str,\n\
            /// \"*\" = whole message is body, a field name = only that field, None = no body\n\
            pub body: Option<&'static str>,\n\
            /// If set, unwrap this field from the response JSON\n\
            pub response_body: Option<&'static str>,\n\
        }}\n\n\
        pub static GRPC_HTTP_ENDPOINTS: &[GrpcHttpEndpoint] = &[\n{entries}];\n"
    );

    fs::write(out_dir.join("grpc_http_endpoints.rs"), src)
        .expect("failed to write grpc_http_endpoints.rs");
}

fn collect_proto_files(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(dir) {
        for entry in rd.flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(collect_proto_files(&path));
            } else if path.extension().map_or(false, |e| e == "proto") {
                out.push(path);
            }
        }
    }
    out
}

/// Very lightweight proto parser – only extracts google.api.http annotations.
/// No full proto grammar; relies on the consistent formatting in nominal-api protos.
fn parse_grpc_http_endpoints(content: &str, out: &mut String) {
    // Grab the service name from `package nominal.xxx.v1;` – we'll use it as the
    // gRPC service prefix so we know which package each RPC belongs to.
    let package = content
        .lines()
        .find_map(|l| {
            let l = l.trim();
            if let Some(rest) = l.strip_prefix("package ") {
                Some(rest.trim_end_matches(';').trim().to_owned())
            } else {
                None
            }
        })
        .unwrap_or_default();

    // Find the service name(s) declared in this file
    let service_name = content
        .lines()
        .find_map(|l| {
            let l = l.trim();
            if let Some(rest) = l.strip_prefix("service ") {
                Some(rest.split('{').next().unwrap_or("").trim().to_owned())
            } else {
                None
            }
        })
        .unwrap_or_default();

    let full_service = if package.is_empty() {
        service_name.clone()
    } else {
        format!("{package}.{service_name}")
    };

    // State machine: scan for `rpc Name(Req) returns (Resp)` then
    // look for the http option block inside its body.
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i].trim();

        if let Some(rpc_name) = parse_rpc_name(l) {
            // Scan ahead for the http option block (within ~20 lines)
            let window = &lines[i..std::cmp::min(i + 30, lines.len())];
            if let Some(binding) = parse_http_binding(window) {
                writeln!(
                    out,
                    "    GrpcHttpEndpoint {{ service: {full_service:?}, rpc: {rpc_name:?}, \
                    method: {:?}, path_template: {:?}, body: {:?}, response_body: {:?} }},",
                    binding.method,
                    binding.path,
                    binding.body.as_deref(),
                    binding.response_body.as_deref(),
                )
                .unwrap();
            }
        }

        i += 1;
    }
}

fn parse_rpc_name(line: &str) -> Option<String> {
    let line = line.trim();
    if !line.starts_with("rpc ") {
        return None;
    }
    // `rpc FooBar(FooBarRequest) returns (FooBarResponse) {`
    let after = line.strip_prefix("rpc ")?.trim();
    let name = after.split('(').next()?.trim().to_owned();
    if name.is_empty() { None } else { Some(name) }
}

struct HttpBinding {
    method: String,
    path: String,
    body: Option<String>,
    response_body: Option<String>,
}

fn parse_http_binding(lines: &[&str]) -> Option<HttpBinding> {
    // Find `option (google.api.http) = {` or `option (google.api.http) = {get: "..."};`
    let start = lines.iter().position(|l| l.contains("google.api.http"))?;

    // Collect only lines belonging to this option block, stopping at the closing `};`
    // which ends both the block form and the single-line form.
    let mut block_lines: Vec<&str> = Vec::new();
    for line in &lines[start..] {
        block_lines.push(line);
        if line.contains("};") {
            break;
        }
    }
    let snippet = block_lines.join(" ");

    let method_path = ["get", "post", "put", "delete", "patch"]
        .iter()
        .find_map(|&m| {
            let pattern = format!("{m}:");
            if let Some(pos) = snippet.find(&pattern) {
                let after = snippet[pos + pattern.len()..].trim_start();
                let path = after
                    .trim_start_matches('"')
                    .split('"')
                    .next()
                    .unwrap_or("")
                    .to_owned();
                if !path.is_empty() {
                    return Some((m.to_uppercase(), path));
                }
            }
            None
        })?;

    let body = extract_field_value(&snippet, "body");
    let response_body = extract_field_value(&snippet, "response_body");

    Some(HttpBinding {
        method: method_path.0,
        path: method_path.1,
        body,
        response_body,
    })
}

/// Extract `field: "value"` from a snippet, returning `Some("value")`.
/// Requires the match to be at a word boundary (preceded by whitespace or `{`).
fn extract_field_value(snippet: &str, field: &str) -> Option<String> {
    let pattern = format!("{field}:");
    let mut search = snippet;
    loop {
        let pos = search.find(&pattern)?;
        // Verify word boundary: char before must be whitespace, '{', or start-of-string
        let boundary_ok = pos == 0
            || search[..pos]
                .chars()
                .last()
                .map_or(true, |c| c.is_whitespace() || c == '{');
        let after = &search[pos + pattern.len()..];
        if boundary_ok {
            let after = after.trim_start();
            if after.starts_with('"') {
                let val = after.trim_start_matches('"').split('"').next()?;
                if !val.is_empty() {
                    return Some(val.to_owned());
                }
            }
        }
        // Advance past this occurrence and keep searching
        search = after;
    }
}

fn generate_proto_descriptor(
    protos_dir: &std::path::Path,
    includes_dir: &std::path::Path,
    out_dir: &PathBuf,
) {
    let descriptor_path = out_dir.join("nominal_descriptor.bin");

    let proto_files = collect_proto_files(protos_dir);
    let proto_file_paths: Vec<&std::path::Path> = proto_files.iter().map(|p| p.as_path()).collect();

    tonic_build::configure()
        .build_server(false)
        .build_client(false)
        .file_descriptor_set_path(&descriptor_path)
        .compile_protos(&proto_file_paths, &[protos_dir, includes_dir])
        .expect("failed to compile proto descriptors");
}
