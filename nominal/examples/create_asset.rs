use nominal::NominalClient;
use nominal::core::AssetCreate;

#[tokio::main]
async fn main() -> nominal::Result<()> {
    let nm = NominalClient::from_profile("cac_staging")?;
    let asset = nm.assets().create(AssetCreate::new("Test!")).await?;
    println!("{}", asset.rid());
    Ok(())
}
