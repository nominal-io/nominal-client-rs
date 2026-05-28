use nominal::smartcard::SmartcardCertResolver;
use nominal::{Config, NominalClientBuilder};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

const ASSET_RID: &str = "ri.scout.main.asset.24e2c5f4-3653-44c0-87a5-47d5479d63bb";
const NUM_THREADS: usize = 4;
const POLL_INTERVAL_MS: u64 = 500;

#[tokio::main]
async fn main() {
    println!(
        "Starting {} concurrent pollers ({}ms interval). Press Ctrl+C to exit.",
        NUM_THREADS, POLL_INTERVAL_MS
    );

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
    let resolver = SmartcardCertResolver::new()?;
    let nm = Arc::new(
        NominalClientBuilder::from_profile_config(profile)
            .client_cert_resolver(Arc::new(resolver))
            .build()?,
    );

    let handles: Vec<_> = (0..NUM_THREADS)
        .map(|i| {
            let nm = Arc::clone(&nm);
            tokio::spawn(async move {
                let mut count = 0u64;
                loop {
                    match nm.assets().get(ASSET_RID).await {
                        Ok(asset) => {
                            count += 1;
                            println!(
                                "[thread {i}] poll {count}: {} {}",
                                asset.name(),
                                asset.rid()
                            );
                        }
                        Err(e) => eprintln!("[thread {i}] error: {e}"),
                    }
                    sleep(Duration::from_millis(POLL_INTERVAL_MS)).await;
                }
            })
        })
        .collect();

    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen for ctrl+c");
    println!("\nShutting down...");

    for handle in &handles {
        handle.abort();
    }

    Ok(())
}
