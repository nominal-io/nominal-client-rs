use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use conjure_http::client::{AsyncService, ConjureRuntime};
use conjure_object::BearerToken;
use conjure_runtime::Client;
use futures::TryStreamExt;
use nominal_api::clients::attachments::api::{
    AsyncAttachmentService, AsyncAttachmentServiceClient,
};
use nominal_api::objects::api::rids::{AttachmentRid, WorkspaceRid};
use nominal_api::objects::api::{Label, PropertyName, PropertyValue, S3Path};
use nominal_api::objects::attachments::api::CreateAttachmentRequest;
use tokio::io::AsyncWriteExt;

use crate::core::ingest::upload_file;
use crate::core::rid::{parse_rid, rid_to_string};
use crate::core::{FileType, UploadOptions};
use crate::{Error, Result};

/// Represents a file attachment in Nominal.
#[derive(Debug, Clone)]
pub struct Attachment {
    rid: String,
    name: String,
    description: Option<String>,
    properties: HashMap<String, String>,
    labels: Vec<String>,
    file_type: String,
    created_at: DateTime<Utc>,
    created_by_rid: String,
    is_archived: bool,
}

impl Attachment {
    pub fn rid(&self) -> &str {
        &self.rid
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub fn properties(&self) -> &HashMap<String, String> {
        &self.properties
    }

    pub fn labels(&self) -> &[String] {
        &self.labels
    }

    pub fn file_type(&self) -> &str {
        &self.file_type
    }

    pub fn created_at(&self) -> &DateTime<Utc> {
        &self.created_at
    }

    pub fn created_by_rid(&self) -> &str {
        &self.created_by_rid
    }

    pub fn is_archived(&self) -> bool {
        self.is_archived
    }

    pub(crate) fn from_conjure(
        attachment: nominal_api::objects::attachments::api::Attachment,
    ) -> Self {
        Self {
            rid: rid_to_string(attachment.rid()),
            name: attachment.title().to_string(),
            description: if attachment.description().is_empty() {
                None
            } else {
                Some(attachment.description().to_string())
            },
            properties: attachment
                .properties()
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            labels: attachment.labels().iter().map(|l| l.to_string()).collect(),
            file_type: attachment.file_type().to_string(),
            created_at: attachment.created_at().to_utc(),
            created_by_rid: attachment.created_by().to_string(),
            is_archived: attachment.is_archived(),
        }
    }
}

/// Parameters for uploading a new attachment.
#[derive(Debug, Clone)]
pub struct AttachmentCreate {
    path: std::path::PathBuf,
    name: String,
    description: Option<String>,
    properties: HashMap<String, String>,
    labels: Vec<String>,
    upload_options: UploadOptions,
}

impl AttachmentCreate {
    pub fn from_path(path: impl Into<std::path::PathBuf>, name: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            name: name.into(),
            description: None,
            properties: HashMap::new(),
            labels: Vec::new(),
            upload_options: UploadOptions::default(),
        }
    }

    #[must_use]
    pub fn description(mut self, value: impl Into<String>) -> Self {
        self.description = Some(value.into());
        self
    }

    #[must_use]
    pub fn properties<I, K, V>(mut self, value: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.properties = value
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();
        self
    }

    #[must_use]
    pub fn labels<I>(mut self, value: I) -> Self
    where
        I: IntoIterator,
        I::Item: Into<String>,
    {
        self.labels = value.into_iter().map(Into::into).collect();
        self
    }

    #[must_use]
    pub fn upload_options(mut self, options: UploadOptions) -> Self {
        self.upload_options = options;
        self
    }
}

/// Client for attachment operations.
#[derive(Clone)]
pub struct AttachmentsClient {
    client: Client,
    runtime: Arc<ConjureRuntime>,
    token: BearerToken,
    workspace_rid: Option<String>,
}

impl AttachmentsClient {
    pub(crate) fn new(
        client: Client,
        runtime: &Arc<ConjureRuntime>,
        token: BearerToken,
        workspace_rid: Option<String>,
    ) -> Self {
        Self {
            client,
            runtime: Arc::clone(runtime),
            token,
            workspace_rid,
        }
    }

    /// Upload a local file as an attachment.
    pub async fn upload(&self, create: AttachmentCreate) -> Result<Attachment> {
        let AttachmentCreate {
            path,
            name,
            description,
            properties,
            labels,
            upload_options,
        } = create;

        let metadata = tokio::fs::metadata(&path).await?;
        if !metadata.is_file() {
            return Err(Error::Upload {
                details: format!("attachment path is not a file: {}", path.display()),
            });
        }

        let mime_type = FileType::from_path(&path)
            .map(|file_type| file_type.mime_type())
            .unwrap_or("application/octet-stream")
            .to_string();
        let s3_path = upload_file(
            self.client.clone(),
            &self.runtime,
            self.token.clone(),
            self.workspace_rid.clone(),
            &path,
            name.clone(),
            mime_type,
            upload_options,
        )
        .await?;

        let mut request = CreateAttachmentRequest::builder()
            .s3_path(S3Path(s3_path))
            .title(name)
            .description(description.unwrap_or_default());

        if let Some(workspace_rid) = self.workspace_rid.as_deref() {
            request = request.workspace(Some(parse_rid::<WorkspaceRid>(workspace_rid)?));
        }
        if !properties.is_empty() {
            request = request.properties(
                properties
                    .into_iter()
                    .map(|(k, v)| (PropertyName(k), PropertyValue(v)))
                    .collect::<BTreeMap<_, _>>(),
            );
        }
        if !labels.is_empty() {
            request = request.labels(labels.into_iter().map(Label).collect::<BTreeSet<_>>());
        }

        let attachment = self.service().create(&self.token, &request.build()).await?;
        Ok(Attachment::from_conjure(attachment))
    }

    /// Get attachment metadata by RID.
    pub async fn get(&self, rid: &str) -> Result<Attachment> {
        let rid = parse_rid::<AttachmentRid>(rid)?;
        let attachment = self.service().get(&self.token, &rid).await?;
        Ok(Attachment::from_conjure(attachment))
    }

    /// Download an attachment's binary content to `path`.
    pub async fn download_to(&self, rid: &str, path: impl AsRef<Path>) -> Result<()> {
        let rid = parse_rid::<AttachmentRid>(rid)?;
        let path = path.as_ref();
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            tokio::fs::create_dir_all(parent).await?;
        }

        let stream = self.service().get_content(&self.token, &rid).await?;
        futures::pin_mut!(stream);
        let mut file = tokio::fs::File::create(path).await?;
        while let Some(chunk) = stream.try_next().await? {
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        Ok(())
    }

    fn service(&self) -> AsyncAttachmentServiceClient<Client> {
        AsyncAttachmentServiceClient::new(self.client.clone(), &self.runtime)
    }
}
