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

/// Client for workspace operations.
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

    /// Resolve the configured workspace, or the tenant default when none is configured.
    pub async fn resolve_workspace(&self, workspace_rid: Option<&str>) -> Result<()> {
        if let Some(rid) = workspace_rid {
            let workspace_rid = parse_rid::<WorkspaceRid>(rid).map_err(Error::from)?;
            self.service
                .get_workspace(&self.token, &workspace_rid)
                .await
                .map_err(Error::from)?;
            return Ok(());
        }

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
