use std::sync::Arc;

use conjure_http::client::{AsyncService, ConjureRuntime};
use conjure_object::BearerToken;
use conjure_runtime::Client;
use nominal_api::clients::security::api::workspace::{
    AsyncWorkspaceService, AsyncWorkspaceServiceClient,
};
use nominal_api::objects::api::rids::WorkspaceRid;

use crate::core::rid::{parse_rid, rid_to_string};
use crate::{Error, Result};

/// A workspace the authenticated user can access.
#[derive(Debug, Clone)]
pub struct Workspace {
    rid: String,
    display_name: Option<String>,
}

impl Workspace {
    pub fn rid(&self) -> &str {
        &self.rid
    }

    pub fn display_name(&self) -> Option<&str> {
        self.display_name.as_deref()
    }

    pub(crate) fn from_conjure(
        workspace: nominal_api::objects::security::api::workspace::Workspace,
    ) -> Self {
        Self {
            rid: rid_to_string(workspace.rid()),
            display_name: workspace.display_name().map(ToString::to_string),
        }
    }
}

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

    /// List the workspaces the authenticated user can access,
    /// sorted by display name (then RID).
    pub async fn list_workspaces(&self) -> Result<Vec<Workspace>> {
        let workspaces = self
            .service
            .get_workspaces(&self.token)
            .await
            .map_err(Error::from)?;

        let mut workspaces: Vec<Workspace> = workspaces
            .into_iter()
            .map(Workspace::from_conjure)
            .collect();
        workspaces.sort_by(|a, b| {
            a.display_name
                .cmp(&b.display_name)
                .then_with(|| a.rid.cmp(&b.rid))
        });
        Ok(workspaces)
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
