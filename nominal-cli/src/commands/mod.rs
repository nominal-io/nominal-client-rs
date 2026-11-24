pub mod api;
pub mod asset;
pub mod channel;
pub mod config;
pub mod connection;
pub mod dataset;
pub mod endpoint;
pub mod fs;
pub mod grpc;
pub mod ingest;
pub mod run;
pub mod user;
pub mod video;

use anyhow::Context;
use nominal::core::{NominalClient, NominalClientBuilder};
use nominal::{Config, Error, Profile};
use once_cell::sync::OnceCell;
use prost_reflect::DescriptorPool;

use crate::context::resolve_profile;

static DESCRIPTOR_POOL: OnceCell<DescriptorPool> = OnceCell::new();

pub(crate) fn descriptor_pool() -> &'static DescriptorPool {
    DESCRIPTOR_POOL.get_or_init(|| {
        let bytes = include_bytes!(concat!(env!("OUT_DIR"), "/nominal_descriptor.bin"));
        DescriptorPool::decode(bytes.as_ref()).expect("failed to decode proto descriptor")
    })
}

pub(crate) fn load_profile(flag: Option<&str>) -> anyhow::Result<Profile> {
    let profile_name = resolve_profile(flag).map_err(|err| match err {
        Error::EnvVarNotSet { .. } => {
            anyhow::anyhow!("no profile specified: pass --profile or set NOMINAL_PROFILE")
        }
        other => anyhow::Error::new(other),
    })?;
    let config = Config::load().map_err(anyhow::Error::new)?;
    config
        .get_profile(&profile_name)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Profile '{profile_name}' not found"))
}

/// The CLI's product name in the `User-Agent` header.
const USER_AGENT_NAME: &str = "nominal-cli";

pub(crate) fn load_client(flag: Option<&str>) -> anyhow::Result<NominalClient> {
    let profile = load_profile(flag)?;
    NominalClientBuilder::from_profile_config(&profile)
        .user_agent(USER_AGENT_NAME, env!("CARGO_PKG_VERSION"))
        .build()
        .context("Failed to create client")
}

/// An HTTP client for raw API calls; requests send `nominal-cli/<version> ai-agent/<0|1>` as
/// their user agent.
pub(crate) fn http_client() -> anyhow::Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(nominal::user_agent::with_ai_agent(&format!(
            "{USER_AGENT_NAME}/{}",
            env!("CARGO_PKG_VERSION")
        )))
        .build()
        .context("failed to create HTTP client")
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::any;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// The `User-Agent` of each request `server` received.
    async fn user_agents(server: &MockServer) -> Vec<String> {
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .map(|request| {
                request
                    .headers
                    .get("user-agent")
                    .map(|value| value.to_str().unwrap().to_string())
                    .unwrap_or_default()
            })
            .collect()
    }

    fn expected_user_agent() -> String {
        format!(
            "nominal-cli/{} ai-agent/{}",
            env!("CARGO_PKG_VERSION"),
            nominal::user_agent::ai_agent_version()
        )
    }

    #[tokio::test]
    async fn raw_api_requests_send_the_cli_and_ai_agent_as_user_agent() {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
            .mount(&server)
            .await;

        super::api::send_request("GET", &format!("{}/api/x", server.uri()), None, "token")
            .await
            .unwrap();

        assert_eq!(user_agents(&server).await, [expected_user_agent()]);
    }
}
