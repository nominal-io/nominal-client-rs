use conjure_http::client::AsyncService;
use conjure_object::BearerToken;
use conjure_runtime::Client;
use nominal_api::authentication::api::AuthenticationServiceV2AsyncClient;

use crate::core::rid::rid_to_string;
use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct User {
    rid: String,
    org_rid: String,
    email: String,
    display_name: String,
}

impl User {
    pub fn rid(&self) -> &str {
        &self.rid
    }

    pub fn org_rid(&self) -> &str {
        &self.org_rid
    }

    pub fn email(&self) -> &str {
        &self.email
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    pub(crate) fn from_conjure(user: nominal_api::authentication::api::UserV2) -> Self {
        Self {
            rid: rid_to_string(user.rid()),
            org_rid: rid_to_string(user.org_rid()),
            email: user.email().to_string(),
            display_name: user.display_name().to_string(),
        }
    }
}

/// Client for user operations.
pub struct UsersClient {
    service: AuthenticationServiceV2AsyncClient<Client>,
    token: BearerToken,
}

impl UsersClient {
    pub(crate) fn new(client: Client, token: BearerToken) -> Self {
        Self {
            service: AuthenticationServiceV2AsyncClient::new(client),
            token,
        }
    }

    /// Get the profile of the authenticated user.
    pub async fn get_my_profile(&self) -> Result<User> {
        let response = self
            .service
            .get_my_profile(&self.token)
            .await
            .map_err(Error::from)?;
        Ok(User::from_conjure(response))
    }
}
