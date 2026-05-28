use nominal::smartcard::SmartcardCertResolver;
use nominal::{Config, NominalClientBuilder};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("error: {e}");
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

    let nm = NominalClientBuilder::from_profile_config(profile)
        .client_cert_resolver(Arc::new(resolver))
        .build()?;

    let asset = nm
        .assets()
        .get("ri.scout.main.asset.24e2c5f4-3653-44c0-87a5-47d5479d63bb")
        .await?;
    println!("{} {}", asset.name(), asset.rid());
    Ok(())
}
