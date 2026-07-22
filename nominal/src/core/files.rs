use std::path::Path;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use conjure_http::client::ConjureRuntime;
use conjure_object::BearerToken;
use conjure_runtime::Client;
use nominal_api::objects::ingest::api::UploadDestination;
use nominal_api::tonic::nominal::file_store::v1::{
    self as proto, files_service_client::FilesServiceClient,
};
use tonic::service::interceptor::InterceptedService;
use tonic::transport::Channel;

use crate::core::grpc::{AuthInterceptor, GrpcConnection};
use crate::core::ingest::multipart;
use crate::core::ingest::UploadOptions;
use crate::{Error, Result};

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
    file_rid: String,
    path: String,
    state: FileState,
    size_bytes: u64,
    created_at: Option<DateTime<Utc>>,
    created_by: Option<String>,
    current_revision_rid: Option<String>,
}

impl LogicalFile {
    /// RID identifying this logical file, stable across revisions and moves.
    pub fn file_rid(&self) -> &str {
        &self.file_rid
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
        let file_rid = match file.identity.and_then(|i| i.identity) {
            Some(proto::logical_file_identity::Identity::Managed(m)) => m.file_rid,
            _ => {
                return Err(Error::MissingResponseField {
                    field: "identity.managed",
                });
            }
        };
        let path = file
            .path
            .ok_or(Error::MissingResponseField { field: "path" })?
            .path;
        let (created_at, created_by) = attribution_parts(file.created);
        let current_revision_rid = file.current_revision.and_then(|r| match r.reference {
            Some(proto::file_revision_ref::Reference::Managed(m)) => Some(m.file_revision_rid),
            _ => None,
        });
        Ok(Self {
            file_rid,
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

/// One entry returned by [`FilesClient::list`]: either a file or a directory.
#[derive(Debug, Clone)]
pub enum FileEntry {
    File(LogicalFile),
    Directory(Directory),
}

impl FileEntry {
    fn from_proto(entry: proto::FileEntry) -> Result<Self> {
        match entry.entry {
            Some(proto::file_entry::Entry::File(f)) => {
                Ok(Self::File(LogicalFile::from_proto(f)?))
            }
            Some(proto::file_entry::Entry::Directory(d)) => Ok(Self::Directory(Directory {
                path: d
                    .path
                    .ok_or(Error::MissingResponseField { field: "path" })?
                    .path,
            })),
            None => Err(Error::MissingResponseField { field: "entry" }),
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

    fn from_proto(revision: proto::ManagedFileRevision) -> Result<Self> {
        let path = revision
            .path
            .ok_or(Error::MissingResponseField { field: "path" })?
            .path;
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

/// Client for file operations in the Nominal file store.
pub struct FilesClient {
    service: FilesService,
    conjure_client: Client,
    runtime: Arc<ConjureRuntime>,
    token: BearerToken,
}

impl FilesClient {
    pub(crate) fn new(
        connection: &GrpcConnection,
        conjure_client: Client,
        runtime: Arc<ConjureRuntime>,
        token: BearerToken,
    ) -> Self {
        Self {
            service: FilesServiceClient::with_interceptor(
                connection.channel(),
                connection.interceptor(),
            ),
            conjure_client,
            runtime,
            token,
        }
    }

    fn service(&self) -> FilesService {
        self.service.clone()
    }

    /// Upload a local file and place it at `destination_path` in the drive,
    /// creating the file. Fails if a file already exists at that path.
    pub async fn push(
        &self,
        drive_rid: &str,
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
        self.put(drive_rid, destination_path, upload.object_key, size_bytes)
            .await
    }

    /// List files and directories directly under `parent_path` in a drive.
    /// The empty path lists the drive root. Collects all pages eagerly.
    pub async fn list(
        &self,
        drive_rid: &str,
        parent_path: &str,
        include_removed: bool,
    ) -> Result<Vec<FileEntry>> {
        let mut entries = Vec::new();
        let mut page_token: Option<String> = None;
        loop {
            let request = proto::ListFilesRequest {
                drive_rid: drive_rid.to_string(),
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
    pub async fn get(&self, drive_rid: &str, path: &str) -> Result<LogicalFile> {
        let request = proto::GetFileRequest {
            drive_rid: drive_rid.to_string(),
            path: Some(proto::LogicalPath {
                path: path.to_string(),
            }),
            include_removed: false,
        };
        let response = self.service().get_file(request).await?.into_inner();
        LogicalFile::from_proto(
            response
                .file
                .ok_or(Error::MissingResponseField { field: "file" })?,
        )
    }

    /// List revisions for a managed file, oldest first. Collects all pages eagerly.
    pub async fn list_revisions(&self, drive_rid: &str, file_rid: &str) -> Result<Vec<FileRevision>> {
        let mut revisions = Vec::new();
        let mut page_token: Option<String> = None;
        loop {
            let request = proto::ListFileRevisionsRequest {
                drive_rid: drive_rid.to_string(),
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
                revisions.push(FileRevision::from_proto(revision)?);
            }
            match response.next_page_token {
                Some(token) if !token.is_empty() => page_token = Some(token),
                _ => break,
            }
        }
        Ok(revisions)
    }

    /// Place a freshly-uploaded object at `path` in the drive, creating the
    /// file. Fails if a file already exists at that path.
    pub async fn put(
        &self,
        drive_rid: &str,
        path: &str,
        object_key: String,
        size_bytes: u64,
    ) -> Result<LogicalFile> {
        self.apply_one(drive_rid, put_change(path, object_key, size_bytes))
            .await
    }

    /// Move a file, identified by its current head revision, to a new path.
    /// Fails if `source_revision_rid` is not the file's current head, or if
    /// a file already exists at the destination path.
    pub async fn move_file(
        &self,
        drive_rid: &str,
        source_revision_rid: &str,
        destination_path: &str,
    ) -> Result<LogicalFile> {
        let change = proto::FileChange {
            change: Some(proto::file_change::Change::Move(proto::MoveFile {
                source_revision_rid: source_revision_rid.to_string(),
                destination: Some(path_destination(destination_path)),
            })),
        };
        self.apply_one(drive_rid, change).await
    }

    /// Soft-delete a file, identified by its current head revision. Fails if
    /// `revision_rid` is not the file's current head.
    pub async fn remove(&self, drive_rid: &str, revision_rid: &str) -> Result<LogicalFile> {
        let change = proto::FileChange {
            change: Some(proto::file_change::Change::Remove(proto::RemoveFile {
                revision_rid: revision_rid.to_string(),
            })),
        };
        self.apply_one(drive_rid, change).await
    }

    /// Reinstate a past revision of a file at `destination_path`.
    pub async fn restore(
        &self,
        drive_rid: &str,
        restore_revision_rid: &str,
        destination_path: &str,
    ) -> Result<LogicalFile> {
        let change = proto::FileChange {
            change: Some(proto::file_change::Change::Restore(proto::RestoreFile {
                restore_revision_rid: restore_revision_rid.to_string(),
                destination: Some(path_destination(destination_path)),
            })),
        };
        self.apply_one(drive_rid, change).await
    }

    /// Apply a single file change and unwrap its result, surfacing a
    /// server-reported failure (e.g. path already exists) as an [`Error`].
    async fn apply_one(&self, drive_rid: &str, change: proto::FileChange) -> Result<LogicalFile> {
        let request = proto::ApplyFileChangesRequest {
            drive_rid: drive_rid.to_string(),
            changes: vec![change],
        };
        let response = self
            .service()
            .apply_file_changes(request)
            .await?
            .into_inner();
        let result = response
            .results
            .into_iter()
            .next()
            .ok_or(Error::MissingResponseField { field: "results" })?;
        match result.result {
            Some(proto::file_change_result::Result::Success(success)) => LogicalFile::from_proto(
                success
                    .file
                    .ok_or(Error::MissingResponseField { field: "file" })?,
            ),
            Some(proto::file_change_result::Result::Failure(failure)) => {
                Err(Error::FileStoreChangeFailed {
                    code: proto::FileStoreError::try_from(failure.code)
                        .map(|c| c.as_str_name().to_string())
                        .unwrap_or_else(|_| failure.code.to_string()),
                    message: failure.message,
                })
            }
            None => Err(Error::MissingResponseField { field: "result" }),
        }
    }
}

fn path_destination(path: &str) -> proto::Destination {
    proto::Destination {
        target: Some(proto::destination::Target::Path(proto::PathTarget {
            path: Some(proto::LogicalPath {
                path: path.to_string(),
            }),
        })),
    }
}

fn put_change(path: &str, object_key: String, size_bytes: u64) -> proto::FileChange {
    proto::FileChange {
        change: Some(proto::file_change::Change::Put(proto::PutFile {
            object: Some(proto::UploadedObjectRef { object_key }),
            size_bytes,
            destination: Some(path_destination(path)),
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
}
