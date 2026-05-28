use nominal::NominalClient;

#[tokio::main]
async fn main() -> nominal::Result<()> {
    let nm = NominalClient::from_profile("cac_staging")?;
    let asset = nm
        .assets()
        .get("ri.scout.main.asset.24e2c5f4-3653-44c0-87a5-47d5479d63bb")
        .await?;
    println!("{} {}", asset.name(), asset.rid());
    Ok(())
}
