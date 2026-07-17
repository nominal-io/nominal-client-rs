use anyhow::Context;
use chrono::SecondsFormat;
use clap::Subcommand;
use nominal::core::{Drive, DrivesClient, FileEntry, NominalClient};

#[derive(Subcommand)]
pub enum FsCommands {
    /// Drive management commands
    Drive {
        #[command(subcommand)]
        drive_command: DriveCommands,
    },
    /// List files and directories at a path in a drive
    Ls {
        /// Drive ID or RID
        #[arg(long)]
        drive: String,
        /// Drive-relative path to list. Defaults to the drive root
        #[arg(default_value = "")]
        path: String,
        /// Include removed (soft-deleted) files
        #[arg(long)]
        include_removed: bool,
    },
    /// Upload a local file to a drive
    Push {
        /// Drive ID or RID
        #[arg(long)]
        drive: String,
        /// Path to the local file to upload
        local_path: std::path::PathBuf,
        /// Destination path in the drive
        destination_path: String,
    },
    /// Move a file to a new path in a drive
    Mv {
        /// Drive ID or RID
        #[arg(long)]
        drive: String,
        /// Current drive-relative path of the file
        source_path: String,
        /// New drive-relative path for the file
        destination_path: String,
    },
    /// Remove a file from a drive
    Rm {
        /// Drive ID or RID
        #[arg(long)]
        drive: String,
        /// Drive-relative path of the file to remove
        path: String,
    },
    /// List the revision history of a file in a drive
    Revisions {
        /// Drive ID or RID
        #[arg(long)]
        drive: String,
        /// Drive-relative path of the file
        path: String,
    },
    /// Restore a past revision of a file in a drive
    Restore {
        /// Drive ID or RID
        #[arg(long)]
        drive: String,
        /// Revision RID to restore (see `nomctl fs revisions`)
        revision_rid: String,
        /// Destination path in the drive
        destination_path: String,
    },
}

#[derive(Subcommand)]
pub enum DriveCommands {
    /// List drives in the workspace
    List {
        /// Include archived drives
        #[arg(long)]
        include_archived: bool,
    },
    /// Create a managed drive
    Create {
        /// The drive ID
        id: String,
    },
    /// Get a drive by ID or RID
    Get {
        /// Drive ID or RID
        drive: String,
    },
    /// Change a drive's ID
    Rename {
        /// Drive ID or RID
        drive: String,
        /// The new drive ID
        new_id: String,
    },
    /// Archive a drive. Archived drives are hidden from the UI but not deleted
    Archive {
        /// Drive ID or RID
        drive: String,
    },
    /// Unarchive a drive, restoring its visibility in the UI
    Unarchive {
        /// Drive ID or RID
        drive: String,
    },
}

pub async fn handle(cmd: FsCommands, client: NominalClient) -> anyhow::Result<()> {
    match cmd {
        FsCommands::Drive { drive_command } => handle_drive(drive_command, client).await,
        FsCommands::Ls {
            drive,
            path,
            include_removed,
        } => {
            let drive_rid = resolve_drive_rid(&client.drives(), &drive).await?;
            let entries = client
                .files()
                .list(&drive_rid, &path, include_removed)
                .await
                .with_context(|| format!("Failed to list '{path}' in drive '{drive}'"))?;
            for entry in entries {
                print_entry(&entry);
            }
            Ok(())
        }
        FsCommands::Push {
            drive,
            local_path,
            destination_path,
        } => {
            let drive_rid = resolve_drive_rid(&client.drives(), &drive).await?;
            let file = client
                .files()
                .push(
                    &drive_rid,
                    &local_path,
                    &destination_path,
                    nominal::core::UploadOptions::new(),
                )
                .await
                .with_context(|| {
                    format!(
                        "Failed to push '{}' to '{destination_path}' in drive '{drive}'",
                        local_path.display()
                    )
                })?;
            print_file(&file);
            Ok(())
        }
        FsCommands::Mv {
            drive,
            source_path,
            destination_path,
        } => {
            let files = client.files();
            let drive_rid = resolve_drive_rid(&client.drives(), &drive).await?;
            let source = files
                .get(&drive_rid, &source_path)
                .await
                .with_context(|| format!("Failed to resolve '{source_path}' in drive '{drive}'"))?;
            let revision_rid = current_revision_rid(&source, &source_path)?;
            let file = files
                .move_file(&drive_rid, revision_rid, &destination_path)
                .await
                .with_context(|| {
                    format!("Failed to move '{source_path}' to '{destination_path}' in drive '{drive}'")
                })?;
            print_file(&file);
            Ok(())
        }
        FsCommands::Rm { drive, path } => {
            let files = client.files();
            let drive_rid = resolve_drive_rid(&client.drives(), &drive).await?;
            let file = files
                .get(&drive_rid, &path)
                .await
                .with_context(|| format!("Failed to resolve '{path}' in drive '{drive}'"))?;
            let revision_rid = current_revision_rid(&file, &path)?;
            let file = files
                .remove(&drive_rid, revision_rid)
                .await
                .with_context(|| format!("Failed to remove '{path}' in drive '{drive}'"))?;
            print_file(&file);
            Ok(())
        }
        FsCommands::Revisions { drive, path } => {
            let files = client.files();
            let drive_rid = resolve_drive_rid(&client.drives(), &drive).await?;
            let file = files
                .get(&drive_rid, &path)
                .await
                .with_context(|| format!("Failed to resolve '{path}' in drive '{drive}'"))?;
            let revisions = files
                .list_revisions(&drive_rid, file.file_rid())
                .await
                .with_context(|| format!("Failed to list revisions for '{path}' in drive '{drive}'"))?;
            for revision in revisions {
                println!(
                    "{}\t{}\t{}",
                    revision.file_revision_rid(),
                    revision.size_bytes(),
                    revision
                        .created_at()
                        .map(|t| t.to_rfc3339_opts(SecondsFormat::Nanos, true))
                        .unwrap_or_default(),
                );
            }
            Ok(())
        }
        FsCommands::Restore {
            drive,
            revision_rid,
            destination_path,
        } => {
            let files = client.files();
            let drive_rid = resolve_drive_rid(&client.drives(), &drive).await?;
            let file = files
                .restore(&drive_rid, &revision_rid, &destination_path)
                .await
                .with_context(|| {
                    format!("Failed to restore revision '{revision_rid}' in drive '{drive}'")
                })?;
            print_file(&file);
            Ok(())
        }
    }
}

async fn handle_drive(cmd: DriveCommands, client: NominalClient) -> anyhow::Result<()> {
    let drives = client.drives();
    match cmd {
        DriveCommands::List { include_archived } => {
            let all = drives
                .list(include_archived)
                .await
                .context("Failed to list drives")?;
            for drive in all {
                println!(
                    "{}\t{}\t{}\t{}",
                    drive.id(),
                    drive.rid(),
                    drive.kind(),
                    drive.state()
                );
            }
        }
        DriveCommands::Create { id } => {
            let drive = drives
                .create(&id)
                .await
                .with_context(|| format!("Failed to create drive '{id}'"))?;
            print_drive(&drive);
        }
        DriveCommands::Get { drive } => {
            let drive = resolve_drive(&drives, &drive).await?;
            print_drive(&drive);
        }
        DriveCommands::Rename { drive, new_id } => {
            let resolved = resolve_drive(&drives, &drive).await?;
            let drive = drives
                .rename(resolved.rid(), &new_id)
                .await
                .with_context(|| format!("Failed to rename drive '{drive}'"))?;
            print_drive(&drive);
        }
        DriveCommands::Archive { drive } => {
            let resolved = resolve_drive(&drives, &drive).await?;
            let drive = drives
                .archive(resolved.rid())
                .await
                .with_context(|| format!("Failed to archive drive '{drive}'"))?;
            print_drive(&drive);
        }
        DriveCommands::Unarchive { drive } => {
            let resolved = resolve_drive(&drives, &drive).await?;
            let drive = drives
                .unarchive(resolved.rid())
                .await
                .with_context(|| format!("Failed to unarchive drive '{drive}'"))?;
            print_drive(&drive);
        }
    }

    Ok(())
}

/// Accept either a drive RID or a drive ID, and resolve it to a drive.
async fn resolve_drive(drives: &DrivesClient, id_or_rid: &str) -> anyhow::Result<Drive> {
    if id_or_rid.starts_with("ri.") {
        drives.get(id_or_rid).await
    } else {
        drives.get_by_id(id_or_rid).await
    }
    .with_context(|| format!("Failed to resolve drive '{id_or_rid}'"))
}

/// Accept either a drive RID or a drive ID, and resolve it to just the RID,
/// for use by file-op commands that only need to address the drive.
async fn resolve_drive_rid(drives: &DrivesClient, id_or_rid: &str) -> anyhow::Result<String> {
    if id_or_rid.starts_with("ri.") {
        return Ok(id_or_rid.to_string());
    }
    Ok(resolve_drive(drives, id_or_rid).await?.rid().to_string())
}

/// A file's current revision RID, or an error if the file has no managed
/// current revision (e.g. it lives in a virtual, read-only drive).
fn current_revision_rid<'a>(
    file: &'a nominal::core::LogicalFile,
    path: &str,
) -> anyhow::Result<&'a str> {
    file.current_revision_rid().ok_or_else(|| {
        anyhow::anyhow!("'{path}' has no managed revision to operate on (read-only drive?)")
    })
}

fn print_entry(entry: &FileEntry) {
    match entry {
        FileEntry::File(file) => print_file(file),
        FileEntry::Directory(dir) => println!("{}/", dir.path()),
    }
}

fn print_file(file: &nominal::core::LogicalFile) {
    println!(
        "{}\t{}\t{}",
        file.path(),
        file.size_bytes(),
        file.state()
    );
}

fn print_drive(drive: &Drive) {
    println!("RID: {}", drive.rid());
    println!("ID: {}", drive.id());
    println!("Kind: {}", drive.kind());
    println!("State: {}", drive.state());
    if let Some(created_at) = drive.created_at() {
        println!(
            "Created: {}",
            created_at.to_rfc3339_opts(SecondsFormat::Nanos, true)
        );
    }
    if let Some(created_by) = drive.created_by() {
        println!("Created by: {created_by}");
    }
}
