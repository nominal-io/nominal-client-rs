use super::*;
use crate::core::{
    UploadOptions, WaitOptions, WorkspacesClient,
    grpc::{GrpcConnection, GrpcMutationTransport, GrpcTransport},
    rid::parse_rid,
};
use crate::{Error, Result};
use conjure_http::client::ConjureRuntime;
use conjure_object::BearerToken;
use conjure_runtime::Client;
use nominal_api::objects::api::rids::WorkspaceRid;
use proto::registry_service_client::RegistryServiceClient;
use std::{path::Path, sync::Arc};

/// Image registration and readiness operations.
#[derive(Clone)]
pub struct ContainerImagesClient {
    read: RegistryServiceClient<GrpcTransport>,
    write: RegistryServiceClient<GrpcMutationTransport>,
    client: Client,
    runtime: Arc<ConjureRuntime>,
    token: BearerToken,
    workspace: Option<String>,
}
impl ContainerImagesClient {
    pub(crate) fn new(
        grpc: GrpcConnection,
        client: Client,
        runtime: Arc<ConjureRuntime>,
        token: BearerToken,
        workspace_rid: Option<WorkspaceRid>,
    ) -> Self {
        Self {
            read: RegistryServiceClient::with_interceptor(grpc.channel(), grpc.interceptor()),
            write: RegistryServiceClient::with_interceptor(
                grpc.mutation_channel(),
                grpc.interceptor(),
            ),
            client,
            runtime,
            token,
            workspace: workspace_rid.map(|r| r.to_string()),
        }
    }
    pub fn in_workspace(mut self, rid: impl Into<String>) -> Self {
        self.workspace = Some(rid.into());
        self
    }
    pub(crate) async fn workspace(&self) -> Result<String> {
        match &self.workspace {
            Some(rid) => Ok(rid.clone()),
            None => {
                Ok(
                    WorkspacesClient::new(self.client.clone(), &self.runtime, self.token.clone())
                        .get_default_workspace()
                        .await?
                        .rid()
                        .into(),
                )
            }
        }
    }
    pub async fn register(
        &self,
        extractor: &ContainerizedExtractor,
        tarball: &Path,
        registration: ImageRegistration,
    ) -> Result<ContainerImage> {
        registration.validate()?;
        let workspace = extractor.workspace_rid().to_owned();
        let filename = tarball
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| {
                ExtractorError::InvalidRegistration("tarball filename is not UTF-8".into())
            })?
            .to_owned();
        let object_path = crate::core::ingest::multipart::upload_file(
            self.client.clone(),
            &self.runtime,
            self.token.clone(),
            Some(parse_rid::<WorkspaceRid>(&workspace)?),
            tarball,
            filename,
            "application/x-tar".into(),
            UploadOptions::default(),
        )
        .await?;
        let response = self
            .write
            .clone()
            .create_image(registration.into_request(extractor, object_path.clone()))
            .await
            .map_err(|source| ExtractorError::Registration {
                object_path,
                source: Box::new(source.into()),
            })?
            .into_inner();
        Self::convert(response.image, workspace)
    }
    pub async fn get(&self, rid: &str) -> Result<ContainerImage> {
        self.get_scoped(rid, self.workspace().await?).await
    }
    async fn get_scoped(&self, rid: &str, workspace: String) -> Result<ContainerImage> {
        let response = self
            .read
            .clone()
            .get_image(proto::GetImageRequest {
                rid: rid.into(),
                workspace_rid: workspace.clone(),
            })
            .await?
            .into_inner();
        Self::convert(response.image, workspace)
    }
    pub async fn refresh(&self, image: &ContainerImage) -> Result<ContainerImage> {
        self.get_scoped(image.rid(), image.workspace_rid().into())
            .await
    }
    pub async fn search(&self, query: ContainerImageQuery) -> Result<Vec<ContainerImage>> {
        let workspace = self.workspace().await?;
        let filter = query.into_proto();
        let mut token = None;
        let mut results = Vec::new();
        loop {
            let response = self
                .read
                .clone()
                .search_images(proto::SearchImagesRequest {
                    workspace_rid: workspace.clone(),
                    filter: filter.clone(),
                    page_size: Some(100),
                    next_page_token: token,
                })
                .await?
                .into_inner();
            results.extend(
                response
                    .images
                    .into_iter()
                    .map(|i| ContainerImage::from_proto(i, workspace.clone()))
                    .collect::<Result<Vec<_>>>()?,
            );
            token = response.next_page_token;
            if token.is_none() {
                break;
            }
        }
        Ok(results)
    }
    pub async fn delete(&self, image: &ContainerImage) -> Result<()> {
        self.write
            .clone()
            .delete_image(proto::DeleteImageRequest {
                rid: image.rid().into(),
                workspace_rid: image.workspace_rid().into(),
            })
            .await?;
        Ok(())
    }
    pub async fn wait_ready(
        &self,
        image: &ContainerImage,
        options: WaitOptions,
    ) -> Result<ContainerImage> {
        options.validate()?;
        let wait = async {
            loop {
                let current = self.refresh(image).await?;
                match current.status() {
                    ContainerImageStatus::Ready => return Ok(current),
                    ContainerImageStatus::Failed => {
                        return Err(ExtractorError::ImageFailed {
                            rid: current.rid().into(),
                        }
                        .into());
                    }
                    _ => tokio::time::sleep(options.poll_interval()).await,
                }
            }
        };
        match options.timeout_duration() {
            Some(duration) => {
                tokio::time::timeout(duration, wait)
                    .await
                    .map_err(|_| ExtractorError::Timeout {
                        rid: image.rid().into(),
                    })?
            }
            None => wait.await,
        }
    }
    fn convert(value: Option<proto::ContainerImage>, workspace: String) -> Result<ContainerImage> {
        ContainerImage::from_proto(
            value.ok_or(Error::UnexpectedResponse { field: "image" })?,
            workspace,
        )
    }
}
