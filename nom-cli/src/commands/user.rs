use nominal_client::NominalClient;
use clap::Subcommand;
use conjure_http::client::AsyncService;
use nominal_api::authentication::api::AuthenticationServiceV2AsyncClient;

#[derive(Subcommand)]
pub enum UserCommands {
    /// Get your profile information
    GetProfile,
}

pub async fn handle(cmd: UserCommands, client: NominalClient) {
    match cmd {
        UserCommands::GetProfile => {
            let service = AuthenticationServiceV2AsyncClient::new(client.client);
            let response = service.get_my_profile(&client.token).await;
            println!("{:#?}\n", response);
        }
    }
}
