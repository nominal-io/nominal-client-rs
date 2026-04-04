use anyhow::Context;
use clap::Subcommand;
use nominal_client::NominalClient;

#[derive(Subcommand)]
pub enum UserCommands {
    /// Get your profile information
    GetProfile,
}

pub async fn handle(cmd: UserCommands, client: NominalClient) -> anyhow::Result<()> {
    match cmd {
        UserCommands::GetProfile => {
            let user = client
                .get_my_profile()
                .await
                .context("Failed to get profile")?;

            println!("RID: {}", user.rid());
            println!("Org RID: {}", user.org_rid());
            println!("Email: {}", user.email());
            println!("Display Name: {}", user.display_name());
        }
    }

    Ok(())
}
