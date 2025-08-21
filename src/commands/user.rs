use crate::client::NominalClient;
use clap::Subcommand;
use conjure_http::client::AsyncService;
use nominal_api::authentication::api::AuthenticationServiceV2AsyncClient;

#[derive(Subcommand)]
pub enum UserCommands {
    /// Get your profile information
    GetProfile,
}

pub async fn handle(user_command: UserCommands, client: NominalClient) {
    match user_command {
        UserCommands::GetProfile => {
            let auth_service = AuthenticationServiceV2AsyncClient::new(client.client);
            let response = auth_service.get_my_profile(&client.token).await;
            println!("User profile: {:?}", response);
        }
    }
}
