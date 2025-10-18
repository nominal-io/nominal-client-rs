use crate::config::Profile;
use conjure_object::BearerToken;
use conjure_runtime::{Agent, Client, UserAgent};

#[derive(Clone)]
pub struct NominalClient {
    pub client: Client,
    pub token: BearerToken,
    pub workspace_rid: Option<String>,
}

impl NominalClient {
    pub fn new(
        base_url: String,
        token: String,
        workspace_rid: Option<String>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let bearer_token = BearerToken::new(&token).unwrap();
        let client = create_client(&base_url).unwrap();
        Ok(NominalClient {
            client,
            token: bearer_token,
            workspace_rid,
        })
    }

    pub fn from_profile(profile: &Profile) -> Result<Self, Box<dyn std::error::Error>> {
        NominalClient::new(
            profile.base_url.clone(),
            profile.token.clone(),
            profile.workspace_rid.clone(),
        )
    }
}

fn create_client(url: &str) -> Result<Client, conjure_error::Error> {
    Client::builder()
        .service("nom-cli-rs")
        .user_agent(UserAgent::new(Agent::new("nom-cli-rs", "0.0")))
        .uri(url.try_into().unwrap())
        .build()
}
