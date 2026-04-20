use std::sync::Arc;

use conjure_http::client::{AsyncService, ConjureRuntime};
use conjure_object::BearerToken;
use conjure_runtime::Client;
use nominal_api::clients::authentication::api::{
    AsyncAuthenticationServiceV2, AsyncAuthenticationServiceV2Client,
};

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

    pub(crate) fn from_conjure(user: nominal_api::objects::authentication::api::UserV2) -> Self {
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
    service: AsyncAuthenticationServiceV2Client<Client>,
    token: BearerToken,
}

impl UsersClient {
    pub(crate) fn new(
        client: Client,
        runtime: &Arc<ConjureRuntime>,
        token: BearerToken,
    ) -> Self {
        Self {
            service: AsyncAuthenticationServiceV2Client::new(client, runtime),
            token,
        }
    }

    /// Get the authenticated user.
    pub async fn who_am_i(&self) -> Result<User> {
        let response = self
            .service
            .get_my_profile(&self.token)
            .await
            .map_err(Error::from)?;
        Ok(User::from_conjure(response))
    }
}
