use super::*;
use crate::core::grpc::{GrpcConnection, GrpcMutationTransport, GrpcTransport};
use crate::{Error, Result};
use conjure_http::client::ConjureRuntime;
use conjure_object::BearerToken;
use conjure_runtime::Client;
use ingest_proto::containerized_extractor_service_client::ContainerizedExtractorServiceClient;
use nominal_api::objects::api::rids::WorkspaceRid;
use std::sync::Arc;

/// Workspace-scoped extractor management. Snapshot operations retain their original workspace.
#[derive(Clone)]
pub struct ExtractorsClient {
    read: ContainerizedExtractorServiceClient<GrpcTransport>,
    write: ContainerizedExtractorServiceClient<GrpcMutationTransport>,
    images: ContainerImagesClient,
}
impl ExtractorsClient {
    pub(crate) fn new(
        grpc: GrpcConnection,
        client: Client,
        runtime: Arc<ConjureRuntime>,
        token: BearerToken,
        workspace_rid: Option<WorkspaceRid>,
    ) -> Self {
        Self {
            read: ContainerizedExtractorServiceClient::with_interceptor(
                grpc.channel(),
                grpc.interceptor(),
            ),
            write: ContainerizedExtractorServiceClient::with_interceptor(
                grpc.mutation_channel(),
                grpc.interceptor(),
            ),
            images: ContainerImagesClient::new(grpc, client, runtime, token, workspace_rid),
        }
    }
    pub fn in_workspace(mut self, rid: impl Into<String>) -> Self {
        self.images = self.images.in_workspace(rid);
        self
    }
    pub async fn create(&self, options: ExtractorCreate) -> Result<ContainerizedExtractor> {
        let workspace_rid = self.images.workspace().await?;
        Self::convert(
            self.write
                .clone()
                .create_containerized_extractor(ingest_proto::CreateContainerizedExtractorRequest {
                    workspace_rid,
                    name: options.name,
                    description: options.description,
                })
                .await?
                .into_inner()
                .extractor,
        )
    }
    pub async fn get(&self, rid: &str) -> Result<ContainerizedExtractor> {
        self.get_scoped(rid, self.images.workspace().await?).await
    }
    async fn get_scoped(&self, rid: &str, workspace_rid: String) -> Result<ContainerizedExtractor> {
        Self::convert(
            self.read
                .clone()
                .get_containerized_extractor(ingest_proto::GetContainerizedExtractorRequest {
                    rid: rid.into(),
                    workspace_rid,
                })
                .await?
                .into_inner()
                .extractor,
        )
    }
    pub async fn refresh(
        &self,
        extractor: &ContainerizedExtractor,
    ) -> Result<ContainerizedExtractor> {
        self.get_scoped(extractor.rid(), extractor.workspace_rid().into())
            .await
    }
    pub async fn search(&self, query: ExtractorQuery) -> Result<Vec<ContainerizedExtractor>> {
        let workspace_rid = self.images.workspace().await?;
        let mut token = None;
        let mut results = Vec::new();
        loop {
            let response = self
                .read
                .clone()
                .search_containerized_extractors(
                    ingest_proto::SearchContainerizedExtractorsRequest {
                        workspace_rid: workspace_rid.clone(),
                        include_archived: query.include_archived,
                        file_extension: query.file_extension.clone(),
                        page_size: 100,
                        next_page_token: token,
                    },
                )
                .await?
                .into_inner();
            results.extend(
                response
                    .extractors
                    .into_iter()
                    .map(ContainerizedExtractor::from_proto)
                    .collect::<Result<Vec<_>>>()?,
            );
            token = response.next_page_token;
            if token.is_none() {
                break;
            }
        }
        Ok(results)
    }
    pub async fn update(
        &self,
        extractor: &ContainerizedExtractor,
        update: ExtractorUpdate,
    ) -> Result<ContainerizedExtractor> {
        self.update_request(update.into_request(extractor.rid(), extractor.workspace_rid()))
            .await
    }
    async fn update_request(
        &self,
        request: ingest_proto::UpdateContainerizedExtractorRequest,
    ) -> Result<ContainerizedExtractor> {
        Self::convert(
            self.write
                .clone()
                .update_containerized_extractor(request)
                .await?
                .into_inner()
                .extractor,
        )
    }
    pub async fn archive(
        &self,
        extractor: &ContainerizedExtractor,
    ) -> Result<ContainerizedExtractor> {
        self.update(extractor, ExtractorUpdate::default().archived(true))
            .await
    }
    pub async fn unarchive(
        &self,
        extractor: &ContainerizedExtractor,
    ) -> Result<ContainerizedExtractor> {
        self.update(extractor, ExtractorUpdate::default().archived(false))
            .await
    }
    pub async fn activate(
        &self,
        extractor: &ContainerizedExtractor,
        image: &ContainerImage,
        activation: Activation,
    ) -> Result<ContainerizedExtractor> {
        if image.workspace_rid() != extractor.workspace_rid()
            || image.extractor_rid() != extractor.rid()
        {
            return Err(ExtractorError::ImageMismatch.into());
        }
        let ready = match activation {
            Activation::RequireReady => {
                let current = self.images.refresh(image).await?;
                if current.status() != ContainerImageStatus::Ready {
                    return Err(ExtractorError::NotReady {
                        rid: current.rid().into(),
                        status: current.status(),
                    }
                    .into());
                }
                current
            }
            Activation::Wait(options) => self.images.wait_ready(image, options).await?,
        };
        let mut request =
            ExtractorUpdate::default().into_request(extractor.rid(), extractor.workspace_rid());
        request.active_container_image_rid = Some(ready.rid().into());
        self.update_request(request).await
    }
    fn convert(
        value: Option<ingest_proto::ContainerizedExtractor>,
    ) -> Result<ContainerizedExtractor> {
        ContainerizedExtractor::from_proto(
            value.ok_or(Error::UnexpectedResponse { field: "extractor" })?,
        )
    }
}
