use std::path::Path;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use conjure_http::client::ConjureRuntime;
use conjure_object::BearerToken;
use conjure_runtime::Client;
use futures::TryStreamExt;
use nominal_api::objects::ingest::api::UploadDestination;
use nominal_api::tonic::nominal::file_store::v1::{
    self as proto, files_service_client::FilesServiceClient,
};
use tokio::io::AsyncRead;
use tokio_util::io::StreamReader;
use tonic::service::interceptor::InterceptedService;
use tonic::transport::Channel;

use super::RequiredField;
use crate::core::grpc::{AuthInterceptor, GrpcConnection};
use crate::core::ingest::UploadOptions;
use crate::core::ingest::multipart;
use crate::{FileStoreError, Result};

type FilesService = FilesServiceClient<InterceptedService<Channel, AuthInterceptor>>;

/// The lifecycle state of a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FileState {
    Active,
    Removed,
    Unknown,
}

impl FileState {
    fn from_proto(value: i32) -> Self {
        match proto::FileState::try_from(value) {
            Ok(proto::FileState::Active) => Self::Active,
            Ok(proto::FileState::Removed) => Self::Removed,
            _ => Self::Unknown,
        }
    }
}

impl std::fmt::Display for FileState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Active => "active",
            Self::Removed => "removed",
            Self::Unknown => "unknown",
        })
    }
}

/// A file at a path in a drive, along with its current revision.
#[derive(Debug, Clone)]
pub struct LogicalFile {
    managed_file_rid: Option<String>,
    path: String,
    state: FileState,
    size_bytes: u64,
    created_at: Option<DateTime<Utc>>,
    created_by: Option<String>,
    current_revision_rid: Option<String>,
}

impl LogicalFile {
    /// RID identifying this managed logical file, stable across revisions and moves.
    ///
    /// Virtual-drive files have provider-specific identities rather than Nominal
    /// file RIDs, so this is `None` for them.
    pub fn managed_file_rid(&self) -> Option<&str> {
        self.managed_file_rid.as_deref()
    }

    /// Drive-relative path.
    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn state(&self) -> FileState {
        self.state
    }

    pub fn size_bytes(&self) -> u64 {
        self.size_bytes
    }

    pub fn created_at(&self) -> Option<DateTime<Utc>> {
        self.created_at
    }

    /// RID of the user who created the file.
    pub fn created_by(&self) -> Option<&str> {
        self.created_by.as_deref()
    }

    /// RID of the file's current revision, when the file is backed by a
    /// managed (not virtual) revision.
    pub fn current_revision_rid(&self) -> Option<&str> {
        self.current_revision_rid.as_deref()
    }

    fn from_proto(file: proto::LogicalFile) -> Result<Self> {
        let identity = file.identity.required("LogicalFile.identity")?;
        let managed_file_rid = match identity
            .identity
            .required("LogicalFile.identity.identity")?
        {
            proto::logical_file_identity::Identity::Managed(managed) => Some(managed.file_rid),
            proto::logical_file_identity::Identity::Virtual(_) => None,
        };
        let path = file.path.required("LogicalFile.path")?.path;
        let (created_at, created_by) = attribution_parts(file.created);
        let current_revision_rid = file.current_revision.and_then(|r| match r.reference {
            Some(proto::file_revision_ref::Reference::Managed(m)) => Some(m.file_revision_rid),
            _ => None,
        });
        Ok(Self {
            managed_file_rid,
            path,
            state: FileState::from_proto(file.state),
            size_bytes: file.size_bytes,
            created_at,
            created_by,
            current_revision_rid,
        })
    }
}

/// A directory entry produced by listing a drive path.
#[derive(Debug, Clone)]
pub struct Directory {
    path: String,
}

impl Directory {
    pub fn path(&self) -> &str {
        &self.path
    }
}

/// One entry returned by [`DriveFilesClient::list`]: either a file or a directory.
#[derive(Debug, Clone)]
pub enum FileEntry {
    File(LogicalFile),
    Directory(Directory),
}

impl FileEntry {
    fn from_proto(entry: proto::FileEntry) -> Result<Self> {
        match entry.entry.required("FileEntry.entry")? {
            proto::file_entry::Entry::File(file) => Ok(Self::File(LogicalFile::from_proto(file)?)),
            proto::file_entry::Entry::Directory(directory) => Ok(Self::Directory(Directory {
                path: directory.path.required("FileEntry.Directory.path")?.path,
            })),
        }
    }
}

/// A single revision in a managed file's history.
#[derive(Debug, Clone)]
pub struct FileRevision {
    file_revision_rid: String,
    file_rid: String,
    path: String,
    size_bytes: u64,
    created_at: Option<DateTime<Utc>>,
    created_by: Option<String>,
}

impl FileRevision {
    pub fn file_revision_rid(&self) -> &str {
        &self.file_revision_rid
    }

    pub fn file_rid(&self) -> &str {
        &self.file_rid
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn size_bytes(&self) -> u64 {
        self.size_bytes
    }

    pub fn created_at(&self) -> Option<DateTime<Utc>> {
        self.created_at
    }

    pub fn created_by(&self) -> Option<&str> {
        self.created_by.as_deref()
    }
}

impl TryFrom<proto::ManagedFileRevision> for FileRevision {
    type Error = crate::Error;

    fn try_from(revision: proto::ManagedFileRevision) -> Result<Self> {
        let path = revision.path.required("ManagedFileRevision.path")?.path;
        let (created_at, created_by) = attribution_parts(revision.created);
        Ok(Self {
            file_revision_rid: revision.file_revision_rid,
            file_rid: revision.file_rid,
            path,
            size_bytes: revision.size_bytes,
            created_at,
            created_by,
        })
    }
}

fn attribution_parts(
    attribution: Option<proto::Attribution>,
) -> (Option<DateTime<Utc>>, Option<String>) {
    let Some(attribution) = attribution else {
        return (None, None);
    };
    let at = attribution
        .time
        .and_then(|t| DateTime::from_timestamp(t.seconds, u32::try_from(t.nanos).unwrap_or(0)));
    let by = (!attribution.user_rid.is_empty()).then_some(attribution.user_rid);
    (at, by)
}

/// Client for file operations scoped to a single drive in the Nominal file store.
pub struct DriveFilesClient {
    service: FilesService,
    conjure_client: Client,
    runtime: Arc<ConjureRuntime>,
    token: BearerToken,
    drive_rid: String,
}

impl DriveFilesClient {
    pub(crate) fn new(
        connection: &GrpcConnection,
        conjure_client: Client,
        runtime: Arc<ConjureRuntime>,
        token: BearerToken,
        drive_rid: impl Into<String>,
    ) -> Self {
        Self {
            service: FilesServiceClient::with_interceptor(
                connection.channel(),
                connection.interceptor(),
            ),
            conjure_client,
            runtime,
            token,
            drive_rid: drive_rid.into(),
        }
    }

    fn service(&self) -> FilesService {
        self.service.clone()
    }

    /// The RID of the drive this client operates on.
    pub fn drive_rid(&self) -> &str {
        &self.drive_rid
    }

    /// Upload a local file and place it at `destination_path` in the drive,
    /// creating the file. Fails if a file already exists at that path.
    pub async fn put(
        &self,
        local_path: impl AsRef<Path>,
        destination_path: &str,
        options: UploadOptions,
    ) -> Result<LogicalFile> {
        let local_path = local_path.as_ref();
        let size_bytes = tokio::fs::metadata(local_path).await?.len();
        let filename = local_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| destination_path.to_string());
        let upload = multipart::upload_file_to(
            self.conjure_client.clone(),
            &self.runtime,
            self.token.clone(),
            None,
            Some(UploadDestination::FileStore),
            local_path,
            filename,
            "application/octet-stream".to_string(),
            options,
        )
        .await?;
        self.put_uploaded_object(destination_path, upload.object_key, size_bytes)
            .await
    }

    /// List files and directories directly under `parent_path`.
    /// The empty path lists the drive root. Collects all pages eagerly.
    pub async fn list(&self, parent_path: &str, include_removed: bool) -> Result<Vec<FileEntry>> {
        let mut entries = Vec::new();
        let mut page_token: Option<String> = None;
        loop {
            let request = proto::ListFilesRequest {
                drive_rid: self.drive_rid.clone(),
                parent_path: Some(proto::LogicalPath {
                    path: parent_path.to_string(),
                }),
                include_removed,
                page_size: None,
                page_token: page_token.take(),
            };
            let response = self.service().list_files(request).await?.into_inner();
            for entry in response.entries {
                entries.push(FileEntry::from_proto(entry)?);
            }
            match response.next_page_token {
                Some(token) if !token.is_empty() => page_token = Some(token),
                _ => break,
            }
        }
        Ok(entries)
    }

    /// Get a file by its drive-relative path.
    pub async fn get(&self, path: &str) -> Result<LogicalFile> {
        let request = proto::GetFileRequest {
            drive_rid: self.drive_rid.clone(),
            path: Some(proto::LogicalPath {
                path: path.to_string(),
            }),
            include_removed: false,
        };
        let response = self.service().get_file(request).await?.into_inner();
        LogicalFile::from_proto(response.file.required("GetFileResponse.file")?)
    }

    /// Open the current content of the file at `path` for streaming download.
    ///
    /// The file's current managed revision is resolved first, then its
    /// short-lived presigned URL is opened. The returned reader owns the HTTP
    /// response and should be consumed promptly because the URL is short-lived.
    pub async fn download(&self, path: &str) -> Result<impl AsyncRead + Unpin + use<>> {
        let file = self.get(path).await?;
        let revision_rid = file
            .current_revision_rid()
            .ok_or_else(|| crate::Error::Download {
                details: format!("'{path}' has no managed revision to download (read-only drive?)"),
            })?;

        let response = self
            .service()
            .get_download_url(proto::GetDownloadUrlRequest {
                file_revision_rid: revision_rid.to_string(),
            })
            .await?
            .into_inner();
        let response = reqwest::Client::new()
            .get(response.url)
            .send()
            .await
            .map_err(|error| crate::Error::Download {
                details: format!("failed to fetch presigned URL: {error}"),
            })?
            .error_for_status()
            .map_err(|error| crate::Error::Download {
                details: format!("download request returned an error: {error}"),
            })?;

        let body = response.bytes_stream().map_err(|error| {
            std::io::Error::other(format!("failed while reading download: {error}"))
        });
        Ok(StreamReader::new(body))
    }

    /// List revisions for a managed file, oldest first. Collects all pages eagerly.
    pub async fn list_revisions(&self, file_rid: &str) -> Result<Vec<FileRevision>> {
        let mut revisions = Vec::new();
        let mut page_token: Option<String> = None;
        loop {
            let request = proto::ListFileRevisionsRequest {
                drive_rid: self.drive_rid.clone(),
                file_rid: file_rid.to_string(),
                page_size: None,
                page_token: page_token.take(),
            };
            let response = self
                .service()
                .list_file_revisions(request)
                .await?
                .into_inner();
            for revision in response.file_revisions {
                revisions.push(FileRevision::try_from(revision)?);
            }
            match response.next_page_token {
                Some(token) if !token.is_empty() => page_token = Some(token),
                _ => break,
            }
        }
        Ok(revisions)
    }

    async fn put_uploaded_object(
        &self,
        path: &str,
        object_key: String,
        size_bytes: u64,
    ) -> Result<LogicalFile> {
        self.apply_one(put_change(path, object_key, size_bytes))
            .await
    }

    /// Move a file, identified by its current head revision, to a destination.
    /// Fails if `source_revision_rid` is not the file's current head.
    pub async fn move_file(
        &self,
        source_revision_rid: &str,
        destination: impl Into<FileOperationDestination>,
    ) -> Result<LogicalFile> {
        let change = proto::FileChange {
            change: Some(proto::file_change::Change::Move(proto::MoveFile {
                source_revision_rid: source_revision_rid.to_string(),
                destination: Some(destination.into().into_proto()),
            })),
        };
        self.apply_one(change).await
    }

    /// Soft-delete a file, identified by its current head revision. Fails if
    /// `revision_rid` is not the file's current head.
    pub async fn remove(&self, revision_rid: &str) -> Result<LogicalFile> {
        let change = proto::FileChange {
            change: Some(proto::file_change::Change::Remove(proto::RemoveFile {
                revision_rid: revision_rid.to_string(),
            })),
        };
        self.apply_one(change).await
    }

    /// Reinstate a past revision at a destination path or by replacing an existing revision.
    pub async fn restore(
        &self,
        restore_revision_rid: &str,
        destination: impl Into<FileOperationDestination>,
    ) -> Result<LogicalFile> {
        let change = proto::FileChange {
            change: Some(proto::file_change::Change::Restore(proto::RestoreFile {
                restore_revision_rid: restore_revision_rid.to_string(),
                destination: Some(destination.into().into_proto()),
            })),
        };
        self.apply_one(change).await
    }

    /// Apply a single file change and unwrap its result, surfacing a
    /// server-reported failure (e.g. path already exists) as an [`Error`].
    async fn apply_one(&self, change: proto::FileChange) -> Result<LogicalFile> {
        let request = proto::ApplyFileChangesRequest {
            drive_rid: self.drive_rid.clone(),
            changes: vec![change],
        };
        let response = self
            .service()
            .apply_file_changes(request)
            .await?
            .into_inner();
        debug_assert!(response.results.len() <= 1);
        let result = response
            .results
            .into_iter()
            .next()
            .required("ApplyFileChangesResponse.results")?;
        match result.result.required("FileChangeResult.result")? {
            proto::file_change_result::Result::Success(success) => {
                LogicalFile::from_proto(success.file.required("FileChangeResult.Success.file")?)
            }
            proto::file_change_result::Result::Failure(failure) => {
                Err(FileStoreError::ChangeFailed {
                    code: proto::FileStoreError::try_from(failure.code)
                        .map(|c| c.as_str_name().to_string())
                        .unwrap_or_else(|_| failure.code.to_string()),
                    message: failure.message,
                }
                .into())
            }
        }
    }
}

/// A target for a file-store operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileOperationDestination {
    /// Place the result at this logical path. The operation fails if the path exists.
    Path(String),
    /// Replace the file currently associated with this managed revision.
    FileRevisionRid(String),
}

impl FileOperationDestination {
    pub fn path(path: impl Into<String>) -> Self {
        Self::Path(path.into())
    }

    pub fn file_revision_rid(file_revision_rid: impl Into<String>) -> Self {
        Self::FileRevisionRid(file_revision_rid.into())
    }

    fn into_proto(self) -> proto::Destination {
        let target = match self {
            Self::Path(path) => proto::destination::Target::Path(proto::PathTarget {
                path: Some(proto::LogicalPath { path }),
            }),
            Self::FileRevisionRid(file_revision_rid) => {
                proto::destination::Target::FileRevisionRid(file_revision_rid)
            }
        };
        proto::Destination {
            target: Some(target),
        }
    }
}

impl From<String> for FileOperationDestination {
    fn from(path: String) -> Self {
        Self::path(path)
    }
}

impl From<&str> for FileOperationDestination {
    fn from(path: &str) -> Self {
        Self::path(path)
    }
}

fn put_change(path: &str, object_key: String, size_bytes: u64) -> proto::FileChange {
    proto::FileChange {
        change: Some(proto::file_change::Change::Put(proto::PutFile {
            object: Some(proto::UploadedObjectRef { object_key }),
            size_bytes,
            destination: Some(FileOperationDestination::path(path).into_proto()),
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_change_uses_the_multipart_object_key() {
        let object_key = "file-store/uploads/abc123";
        let change = put_change("example.txt", object_key.to_string(), 42);

        let Some(proto::file_change::Change::Put(put)) = change.change else {
            panic!("expected put change");
        };
        assert_eq!(put.object.unwrap().object_key, object_key);
    }

    #[test]
    fn file_revision_destination_replaces_an_existing_file() {
        let destination =
            FileOperationDestination::file_revision_rid("ri.file-revision.123").into_proto();

        assert!(matches!(
            destination.target,
            Some(proto::destination::Target::FileRevisionRid(rid)) if rid == "ri.file-revision.123"
        ));
    }

    #[test]
    fn logical_file_from_proto_accepts_virtual_identity() {
        let file = proto::LogicalFile {
            identity: Some(proto::LogicalFileIdentity {
                identity: Some(proto::logical_file_identity::Identity::Virtual(
                    proto::VirtualFileIdentity {
                        kind: Some(proto::virtual_file_identity::Kind::S3(
                            proto::S3FileIdentity {
                                drive_rid: "ri.drive.virtual".to_string(),
                                path: "telemetry/flight.csv".to_string(),
                            },
                        )),
                    },
                )),
            }),
            path: Some(proto::LogicalPath {
                path: "telemetry/flight.csv".to_string(),
            }),
            state: proto::FileState::Active as i32,
            created: None,
            size_bytes: 42,
            observed: None,
            current_revision: None,
        };

        let entry = proto::FileEntry {
            entry: Some(proto::file_entry::Entry::File(file)),
        };
        let FileEntry::File(file) = FileEntry::from_proto(entry).unwrap() else {
            panic!("expected file entry");
        };
        assert_eq!(file.path(), "telemetry/flight.csv");
        assert_eq!(file.managed_file_rid(), None);
    }
}
