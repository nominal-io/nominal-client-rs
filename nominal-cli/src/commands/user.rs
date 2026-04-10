use anyhow::Context;
use clap::Subcommand;
use nominal::NominalClient;

#[derive(Subcommand)]
pub enum UserCommands {
    /// Get your user information
    WhoAmI,
}

pub async fn handle(cmd: UserCommands, client: NominalClient) -> anyhow::Result<()> {
    match cmd {
        UserCommands::WhoAmI => {
            let user = client
                .users()
                .who_am_i()
                .await
                .context("Failed to get user information")?;

            println!("RID: {}", user.rid());
            println!("Org RID: {}", user.org_rid());
            println!("Email: {}", user.email());
            println!("Display Name: {}", user.display_name());
        }
    }

    Ok(())
}
