use std::sync::Arc;

use conjure_http::client::{AsyncService, ConjureRuntime};
use conjure_object::BearerToken;
use conjure_runtime::Client;
use nominal_api::clients::security::api::workspace::{
    AsyncWorkspaceService, AsyncWorkspaceServiceClient,
};
use nominal_api::objects::api::rids::WorkspaceRid;

use crate::core::rid::parse_rid;
use crate::{Error, Result};

/// Thin wrapper around the workspace API, used during profile
/// validation to confirm that credentials can reach a workspace.
pub struct WorkspacesClient {
    service: AsyncWorkspaceServiceClient<Client>,
    token: BearerToken,
}

impl WorkspacesClient {
    pub(crate) fn new(client: Client, runtime: &Arc<ConjureRuntime>, token: BearerToken) -> Self {
        Self {
            service: AsyncWorkspaceServiceClient::new(client, runtime),
            token,
        }
    }

    /// Verify that the token can reach a workspace.
    /// Only needs to check workspace *exists* and credentials are valid.
    /// The profile config already stores the RID when one is provided.
    pub async fn resolve_workspace(&self, workspace_rid: Option<&str>) -> Result<()> {
        if let Some(rid) = workspace_rid {
            // Explicit RID: confirm it resolves (404 → validation error).
            let workspace_rid = parse_rid::<WorkspaceRid>(rid).map_err(Error::from)?;
            self.service
                .get_workspace(&self.token, &workspace_rid)
                .await
                .map_err(Error::from)?;
            return Ok(());
        }

        // No explicit RID: ensure the tenant has a default workspace.
        let workspaces = self
            .service
            .get_default_workspace(&self.token)
            .await
            .map_err(Error::from)?;

        if workspaces.is_none() {
            return Err(Error::NoDefaultWorkspace);
        }

        Ok(())
    }
}
