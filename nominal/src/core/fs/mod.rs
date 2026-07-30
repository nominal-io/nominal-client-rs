mod drives;
mod files;

use crate::{Error, Result};

pub use drives::{Drive, DriveKind, DriveMutability, DriveSource, DriveState, DrivesClient};
pub use files::{
    Directory, DriveFilesClient, FileEntry, FileOperationDestination, FileRevision, FileState,
    LogicalFile,
};

trait RequiredField<T> {
    fn required(self, field: &'static str) -> Result<T>;
}

impl<T> RequiredField<T> for Option<T> {
    fn required(self, field: &'static str) -> Result<T> {
        self.ok_or(Error::UnexpectedResponse { field })
    }
}
