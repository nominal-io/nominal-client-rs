use clap::Subcommand;

use super::api::{CONJURE_ENDPOINTS, GRPC_HTTP_ENDPOINTS};
use super::descriptor_pool;

#[derive(Subcommand)]
pub enum EndpointCommands {
    /// List all available endpoints
    List {
        /// Filter by substring (matches service, path, or method name)
        #[arg(long)]
        filter: Option<String>,
    },
}

pub fn handle(command: EndpointCommands) -> anyhow::Result<()> {
    match command {
        EndpointCommands::List { filter } => cmd_list(filter.as_deref()),
    }
}

fn cmd_list(filter: Option<&str>) -> anyhow::Result<()> {
    let filter = filter.unwrap_or("").to_lowercase();

    // Collect all rows: (http_method, path, service_method, protocol)
    // gRPC-HTTP first so they shadow conjure entries in display (still show both)
    let mut rows: Vec<(String, String, String, &str)> = Vec::new();

    for ep in GRPC_HTTP_ENDPOINTS {
        let service_method = format!("{}/{}", ep.service, ep.rpc);
        rows.push((
            ep.method.to_owned(),
            ep.path_template.to_owned(),
            service_method,
            "grpc-http",
        ));
    }

    for ep in CONJURE_ENDPOINTS {
        let service_method = format!("{}.{}", ep.service, ep.name);
        rows.push((
            ep.method.to_owned(),
            ep.path_template.to_owned(),
            service_method,
            "conjure",
        ));
    }

    let pool = descriptor_pool();
    for svc in pool.services() {
        for method in svc.methods() {
            let service_method = format!("{}/{}", svc.full_name(), method.name());
            rows.push((
                String::new(),
                service_method.clone(),
                service_method,
                "grpc",
            ));
        }
    }

    // Filter
    let rows: Vec<_> = if filter.is_empty() {
        rows
    } else {
        rows.into_iter()
            .filter(|(method, path, svc_method, proto)| {
                let haystack = format!("{method} {path} {svc_method} {proto}").to_lowercase();
                haystack.contains(&filter)
            })
            .collect()
    };

    if rows.is_empty() {
        if filter.is_empty() {
            println!("no endpoints found");
        } else {
            println!("no endpoints matching `{filter}`");
        }
        return Ok(());
    }

    // Column widths
    let method_w = rows.iter().map(|(m, ..)| m.len()).max().unwrap_or(0);
    let path_w = rows.iter().map(|(_, p, ..)| p.len()).max().unwrap_or(0);
    let proto_w = rows
        .iter()
        .map(|(.., proto)| proto.len())
        .max()
        .unwrap_or(0);

    for (method, path, svc_method, proto) in &rows {
        println!(
            "{:<method_w$}  {:<path_w$}  {:<proto_w$}  {}",
            method, path, proto, svc_method,
        );
    }

    Ok(())
}
