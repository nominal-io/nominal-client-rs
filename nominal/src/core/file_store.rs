use chrono::{DateTime, Utc};
use nominal_api::tonic::nominal::file_store::v1::{
    self as proto, internal_drives_service_client::InternalDrivesServiceClient,
};
use tonic::service::interceptor::InterceptedService;
use tonic::transport::Channel;

use crate::core::grpc::{AuthInterceptor, GrpcConnection};
use crate::{Error, Result};

type DrivesService = InternalDrivesServiceClient<InterceptedService<Channel, AuthInterceptor>>;

/// The lifecycle state of a drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DriveState {
    Active,
    Archived,
    Unknown,
}

impl DriveState {
    fn from_proto(value: i32) -> Self {
        match proto::DriveState::try_from(value) {
            Ok(proto::DriveState::Active) => Self::Active,
            Ok(proto::DriveState::Archived) => Self::Archived,
            _ => Self::Unknown,
        }
    }
}

impl std::fmt::Display for DriveState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Active => "active",
            Self::Archived => "archived",
            Self::Unknown => "unknown",
        })
    }
}

/// Whether a drive's files are stored by Nominal (managed) or backed by an
/// external source such as S3 or Google Drive (virtual).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DriveKind {
    Managed,
    Virtual,
    Unknown,
}

impl DriveKind {
    fn from_proto(value: i32) -> Self {
        match proto::DriveKind::try_from(value) {
            Ok(proto::DriveKind::Managed) => Self::Managed,
            Ok(proto::DriveKind::Virtual) => Self::Virtual,
            _ => Self::Unknown,
        }
    }
}

impl std::fmt::Display for DriveKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Managed => "managed",
            Self::Virtual => "virtual",
            Self::Unknown => "unknown",
        })
    }
}

/// Represents a drive in the Nominal file store.
///
/// A drive is a named container of files within a workspace, similar to a
/// bucket or a shared folder.
#[derive(Debug, Clone)]
pub struct Drive {
    rid: String,
    workspace_rid: String,
    id: String,
    state: DriveState,
    kind: DriveKind,
    created_at: Option<DateTime<Utc>>,
    created_by: Option<String>,
}

impl Drive {
    pub fn rid(&self) -> &str {
        &self.rid
    }

    pub fn workspace_rid(&self) -> &str {
        &self.workspace_rid
    }

    /// The drive's human-readable identifier, unique within the workspace.
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn state(&self) -> DriveState {
        self.state
    }

    pub fn kind(&self) -> DriveKind {
        self.kind
    }

    pub fn created_at(&self) -> Option<DateTime<Utc>> {
        self.created_at
    }

    /// RID of the user who created the drive.
    pub fn created_by(&self) -> Option<&str> {
        self.created_by.as_deref()
    }

    pub(crate) fn from_proto(drive: proto::Drive) -> Self {
        let (created_at, created_by) = attribution_parts(drive.created);
        Self {
            state: DriveState::from_proto(drive.state),
            kind: DriveKind::from_proto(drive.kind),
            rid: drive.rid,
            workspace_rid: drive.workspace_rid,
            id: drive.id,
            created_at,
            created_by,
        }
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

fn required_drive(drive: Option<proto::Drive>) -> Result<Drive> {
    drive
        .map(Drive::from_proto)
        .ok_or(Error::MissingResponseField { field: "drive" })
}

/// Client for drive operations in the Nominal file store.
pub struct DrivesClient {
    service: DrivesService,
    workspace_rid: Option<String>,
}

impl DrivesClient {
    pub(crate) fn new(connection: &GrpcConnection, workspace_rid: Option<String>) -> Self {
        Self {
            service: InternalDrivesServiceClient::with_interceptor(
                connection.channel(),
                connection.interceptor(),
            ),
            workspace_rid,
        }
    }

    /// The generated tonic clients take `&mut self`; cloning is cheap and
    /// shares the underlying channel.
    fn service(&self) -> DrivesService {
        self.service.clone()
    }

    fn workspace_rid(&self) -> Result<String> {
        self.workspace_rid.clone().ok_or(Error::WorkspaceRequired)
    }

    /// Create a managed drive in the workspace.
    pub async fn create(&self, id: impl Into<String>) -> Result<Drive> {
        let request = proto::CreateDriveRequest {
            workspace_rid: self.workspace_rid()?,
            id: id.into(),
        };
        let response = self.service().create_drive(request).await?.into_inner();
        required_drive(response.drive)
    }

    /// Get a drive by RID.
    pub async fn get(&self, rid: &str) -> Result<Drive> {
        let request = proto::GetDriveRequest {
            drive_rid: rid.to_string(),
        };
        let response = self.service().get_drive(request).await?.into_inner();
        required_drive(response.drive)
    }

    /// Get a drive by ID within the workspace.
    pub async fn get_by_id(&self, id: &str) -> Result<Drive> {
        let request = proto::GetDriveByIdRequest {
            workspace_rid: self.workspace_rid()?,
            id: id.to_string(),
        };
        let response = self.service().get_drive_by_id(request).await?.into_inner();
        required_drive(response.drive)
    }

    /// List drives in the workspace, collecting all pages eagerly.
    pub async fn list(&self, include_archived: bool) -> Result<Vec<Drive>> {
        let workspace_rid = self.workspace_rid()?;
        let mut drives = Vec::new();
        let mut page_token: Option<String> = None;
        loop {
            let request = proto::ListDrivesRequest {
                workspace_rid: workspace_rid.clone(),
                include_archived,
                page_size: None,
                page_token: page_token.take(),
            };
            let response = self.service().list_drives(request).await?.into_inner();
            drives.extend(response.drives.into_iter().map(Drive::from_proto));
            match response.next_page_token {
                Some(token) if !token.is_empty() => page_token = Some(token),
                _ => break,
            }
        }
        Ok(drives)
    }

    /// Change a drive's ID. Returns the updated drive.
    pub async fn rename(&self, rid: &str, new_id: impl Into<String>) -> Result<Drive> {
        let request = proto::UpdateDriveDetailsRequest {
            drive_rid: rid.to_string(),
            id: Some(new_id.into()),
        };
        let response = self
            .service()
            .update_drive_details(request)
            .await?
            .into_inner();
        required_drive(response.drive)
    }

    /// Archive a drive. Archived drives are hidden from the UI but not deleted.
    pub async fn archive(&self, rid: &str) -> Result<Drive> {
        let request = proto::ArchiveDriveRequest {
            drive_rid: rid.to_string(),
        };
        let response = self.service().archive_drive(request).await?.into_inner();
        required_drive(response.drive)
    }

    /// Unarchive a drive, restoring its visibility in the UI.
    pub async fn unarchive(&self, rid: &str) -> Result<Drive> {
        let request = proto::UnarchiveDriveRequest {
            drive_rid: rid.to_string(),
        };
        let response = self.service().unarchive_drive(request).await?.into_inner();
        required_drive(response.drive)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drive_state_from_proto() {
        assert_eq!(
            DriveState::from_proto(proto::DriveState::Active as i32),
            DriveState::Active
        );
        assert_eq!(
            DriveState::from_proto(proto::DriveState::Archived as i32),
            DriveState::Archived
        );
        assert_eq!(DriveState::from_proto(0), DriveState::Unknown);
        assert_eq!(DriveState::from_proto(999), DriveState::Unknown);
    }

    #[test]
    fn drive_kind_from_proto() {
        assert_eq!(
            DriveKind::from_proto(proto::DriveKind::Managed as i32),
            DriveKind::Managed
        );
        assert_eq!(
            DriveKind::from_proto(proto::DriveKind::Virtual as i32),
            DriveKind::Virtual
        );
        assert_eq!(DriveKind::from_proto(0), DriveKind::Unknown);
    }

    #[test]
    fn drive_from_proto_maps_fields() {
        let drive = Drive::from_proto(proto::Drive {
            rid: "ri.filestore.test.drive.abc".to_string(),
            workspace_rid: "ri.security.test.workspace.def".to_string(),
            id: "flight-logs".to_string(),
            state: proto::DriveState::Active as i32,
            kind: proto::DriveKind::Managed as i32,
            created: Some(proto::Attribution {
                time: Some(nominal_api::tonic::google::protobuf::Timestamp {
                    seconds: 1_700_000_000,
                    nanos: 500,
                }),
                user_rid: "ri.security.test.user.ghi".to_string(),
            }),
        });
        assert_eq!(drive.rid(), "ri.filestore.test.drive.abc");
        assert_eq!(drive.workspace_rid(), "ri.security.test.workspace.def");
        assert_eq!(drive.id(), "flight-logs");
        assert_eq!(drive.state(), DriveState::Active);
        assert_eq!(drive.kind(), DriveKind::Managed);
        assert_eq!(
            drive.created_at().map(|t| t.timestamp()),
            Some(1_700_000_000)
        );
        assert_eq!(drive.created_by(), Some("ri.security.test.user.ghi"));
    }

    #[test]
    fn drive_from_proto_missing_attribution() {
        let drive = Drive::from_proto(proto::Drive {
            rid: "ri.filestore.test.drive.abc".to_string(),
            workspace_rid: String::new(),
            id: "d".to_string(),
            state: 0,
            kind: 0,
            created: None,
        });
        assert_eq!(drive.created_at(), None);
        assert_eq!(drive.created_by(), None);
    }
}
