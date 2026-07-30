use anyhow::Context;
use chrono::SecondsFormat;
use clap::Subcommand;
use nominal::core::{Drive, DrivesClient, FileEntry, FileState, NominalClient};

#[derive(Subcommand)]
pub enum FsCommands {
    /// Drive management commands
    Drive {
        #[command(subcommand)]
        drive_command: DriveCommands,
    },
    /// List files and directories in a Nominal Drive.
    #[command(after_help = "Examples:\n  nomctl fs ls eng:/\n  nomctl fs ls eng:/telemetry")]
    Ls {
        /// Drive-qualified path, formatted as DRIVE:/PATH.
        #[arg(value_name = "DRIVE:/PATH")]
        path: String,
        /// Include removed (soft-deleted) files
        #[arg(long)]
        include_removed: bool,
    },
    /// Put a local file in a drive.
    Put {
        /// Path to the local file to upload
        local_path: std::path::PathBuf,
        /// Drive-qualified destination path, formatted as DRIVE:/PATH.
        #[arg(value_name = "DRIVE:/PATH")]
        destination_path: String,
    },
    /// Move a file to a new path in a drive.
    Mv {
        /// Drive-qualified source path, formatted as DRIVE:/PATH.
        #[arg(value_name = "DRIVE:/SOURCE_PATH")]
        source_path: String,
        /// Drive-qualified destination path, formatted as DRIVE:/PATH.
        #[arg(value_name = "DRIVE:/DESTINATION_PATH")]
        destination_path: String,
    },
    /// Remove a file from a drive.
    Rm {
        /// Drive-qualified path, formatted as DRIVE:/PATH.
        #[arg(value_name = "DRIVE:/PATH")]
        path: String,
    },
    /// List the revision history of a file in a drive.
    Revisions {
        /// Drive-qualified path, formatted as DRIVE:/PATH.
        #[arg(value_name = "DRIVE:/PATH")]
        path: String,
    },
    /// Restore a past revision of a file in a drive.
    Restore {
        /// Revision RID to restore (see `nomctl fs revisions`)
        revision_rid: String,
        /// Drive-qualified destination path, formatted as DRIVE:/PATH.
        #[arg(value_name = "DRIVE:/PATH")]
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
    /// Get a drive by ID
    Get {
        /// Drive ID
        id: String,
    },
    /// Get a drive by RID
    GetByRid {
        /// Drive RID
        rid: String,
    },
    /// Change a drive's ID
    Rename {
        /// Drive ID
        drive: String,
        /// The new drive ID
        new_id: String,
    },
    /// Archive a drive. Archived drives are hidden from the UI but not deleted
    Archive {
        /// Drive ID
        drive: String,
    },
    /// Unarchive a drive, restoring its visibility in the UI
    Unarchive {
        /// Drive ID
        drive: String,
    },
}

pub async fn handle(cmd: FsCommands, client: NominalClient) -> anyhow::Result<()> {
    match cmd {
        FsCommands::Drive { drive_command } => handle_drive(drive_command, client).await,
        FsCommands::Ls {
            path,
            include_removed,
        } => {
            let (drive, path) = parse_qualified_path(&path)?;
            let drive_rid = drive_rid_for_id(&client.drives(), &drive).await?;
            let entries = client
                .files(drive_rid)
                .list(&path, include_removed)
                .await
                .with_context(|| format!("Failed to list '{path}' in drive '{drive}'"))?;
            print_listing(&entries, include_removed);
            Ok(())
        }
        FsCommands::Put {
            local_path,
            destination_path,
        } => {
            let (drive, destination_path) = parse_qualified_path(&destination_path)?;
            let drive_rid = drive_rid_for_id(&client.drives(), &drive).await?;
            let file = client
                .files(drive_rid)
                .put(
                    &local_path,
                    &destination_path,
                    nominal::core::UploadOptions::new(),
                )
                .await
                .with_context(|| {
                    format!(
                        "Failed to put '{}' at '{destination_path}' in drive '{drive}'",
                        local_path.display()
                    )
                })?;
            print_file(&file);
            Ok(())
        }
        FsCommands::Mv {
            source_path,
            destination_path,
        } => {
            let (drive, source_path, destination_path) =
                parse_move_paths(source_path, destination_path)?;
            let drive_rid = drive_rid_for_id(&client.drives(), &drive).await?;
            let files = client.files(drive_rid);
            let source = files
                .get(&source_path)
                .await
                .with_context(|| format!("Failed to resolve '{source_path}' in drive '{drive}'"))?;
            let revision_rid = current_revision_rid(&source, &source_path)?;
            let file = files
                .move_file(revision_rid, destination_path.as_str())
                .await
                .with_context(|| {
                    format!(
                        "Failed to move '{source_path}' to '{destination_path}' in drive '{drive}'"
                    )
                })?;
            print_file(&file);
            Ok(())
        }
        FsCommands::Rm { path } => {
            let (drive, path) = parse_qualified_path(&path)?;
            let drive_rid = drive_rid_for_id(&client.drives(), &drive).await?;
            let files = client.files(drive_rid);
            let file = files
                .get(&path)
                .await
                .with_context(|| format!("Failed to resolve '{path}' in drive '{drive}'"))?;
            let revision_rid = current_revision_rid(&file, &path)?;
            let file = files
                .remove(revision_rid)
                .await
                .with_context(|| format!("Failed to remove '{path}' in drive '{drive}'"))?;
            print_file(&file);
            Ok(())
        }
        FsCommands::Revisions { path } => {
            let (drive, path) = parse_qualified_path(&path)?;
            let drive_rid = drive_rid_for_id(&client.drives(), &drive).await?;
            let files = client.files(drive_rid);
            let file = files
                .get(&path)
                .await
                .with_context(|| format!("Failed to resolve '{path}' in drive '{drive}'"))?;
            let revisions = files
                .list_revisions(file.file_rid())
                .await
                .with_context(|| {
                    format!("Failed to list revisions for '{path}' in drive '{drive}'")
                })?;
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
            revision_rid,
            destination_path,
        } => {
            let (drive, destination_path) = parse_qualified_path(&destination_path)?;
            let drive_rid = drive_rid_for_id(&client.drives(), &drive).await?;
            let files = client.files(drive_rid);
            let file = files
                .restore(&revision_rid, destination_path.as_str())
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
        DriveCommands::Get { id } => {
            let drive = drives
                .get_by_id(&id)
                .await
                .with_context(|| format!("Failed to get drive '{id}'"))?;
            print_drive(&drive);
        }
        DriveCommands::GetByRid { rid } => {
            let drive = drives
                .get(&rid)
                .await
                .with_context(|| format!("Failed to get drive '{rid}'"))?;
            print_drive(&drive);
        }
        DriveCommands::Rename { drive, new_id } => {
            let resolved = get_drive_by_id(&drives, &drive).await?;
            let drive = drives
                .rename(resolved.rid(), &new_id)
                .await
                .with_context(|| format!("Failed to rename drive '{drive}'"))?;
            print_drive(&drive);
        }
        DriveCommands::Archive { drive } => {
            let resolved = get_drive_by_id(&drives, &drive).await?;
            let drive = drives
                .archive(resolved.rid())
                .await
                .with_context(|| format!("Failed to archive drive '{drive}'"))?;
            print_drive(&drive);
        }
        DriveCommands::Unarchive { drive } => {
            let resolved = get_drive_by_id(&drives, &drive).await?;
            let drive = drives
                .unarchive(resolved.rid())
                .await
                .with_context(|| format!("Failed to unarchive drive '{drive}'"))?;
            print_drive(&drive);
        }
    }

    Ok(())
}

async fn get_drive_by_id(drives: &DrivesClient, id: &str) -> anyhow::Result<Drive> {
    drives
        .get_by_id(id)
        .await
        .with_context(|| format!("Failed to get drive '{id}'"))
}

async fn drive_rid_for_id(drives: &DrivesClient, id: &str) -> anyhow::Result<String> {
    Ok(get_drive_by_id(drives, id).await?.rid().to_string())
}

fn parse_move_paths(
    source_path: String,
    destination_path: String,
) -> anyhow::Result<(String, String, String)> {
    let (source_drive, source_path) = parse_qualified_path(&source_path)?;
    let (destination_drive, destination_path) = parse_qualified_path(&destination_path)?;
    if source_drive != destination_drive {
        anyhow::bail!("cross-drive moves are not supported");
    }
    Ok((source_drive, source_path, destination_path))
}

fn parse_qualified_path(path: &str) -> anyhow::Result<(String, String)> {
    let (drive, path) = path.split_once(':').ok_or_else(|| {
        anyhow::anyhow!("expected a drive-qualified path formatted as DRIVE:/PATH")
    })?;
    let path = path.strip_prefix('/').ok_or_else(|| {
        anyhow::anyhow!("expected a drive-qualified path formatted as DRIVE:/PATH")
    })?;
    if drive.is_empty() {
        anyhow::bail!("expected a drive-qualified path formatted as DRIVE:/PATH");
    }
    Ok((drive.to_string(), path.to_string()))
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

fn print_listing(entries: &[FileEntry], include_removed: bool) {
    for entry in entries {
        print_entry(entry, include_removed);
    }
}

fn print_entry(entry: &FileEntry, include_removed: bool) {
    match entry {
        FileEntry::File(file) => println!(
            "{}",
            format_file_listing_line(file.path(), file.state(), include_removed)
        ),
        FileEntry::Directory(dir) => println!("{}/", dir.path()),
    }
}

fn format_file_listing_line(path: &str, state: FileState, include_removed: bool) -> String {
    let state_suffix = if include_removed && state != FileState::Active {
        format!("  [{state}]")
    } else {
        String::new()
    };
    format!("{path}{state_suffix}")
}

fn print_file(file: &nominal::core::LogicalFile) {
    println!("{}\t{}\t{}", file.path(), file.size_bytes(), file.state());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_listing_only_prints_the_path_by_default() {
        assert_eq!(
            format_file_listing_line("apollo.txt", FileState::Active, false),
            "apollo.txt"
        );
    }

    #[test]
    fn file_listing_only_shows_non_active_state_when_requested() {
        assert_eq!(
            format_file_listing_line("old.csv", FileState::Removed, false),
            "old.csv"
        );
        assert_eq!(
            format_file_listing_line("old.csv", FileState::Removed, true),
            "old.csv  [removed]"
        );
    }

    #[test]
    fn qualified_path_parses_a_drive_and_relative_path() {
        assert_eq!(
            parse_qualified_path("eng:/telemetry").unwrap(),
            ("eng".to_string(), "telemetry".to_string())
        );
    }

    #[test]
    fn qualified_path_rejects_an_unqualified_path() {
        assert!(parse_qualified_path("telemetry").is_err());
    }

    #[test]
    fn move_paths_must_reference_the_same_drive() {
        assert_eq!(
            parse_move_paths(
                "eng:/telemetry/flight-042.mcap".to_string(),
                "eng:/archived/flight-042.mcap".to_string(),
            )
            .unwrap(),
            (
                "eng".to_string(),
                "telemetry/flight-042.mcap".to_string(),
                "archived/flight-042.mcap".to_string(),
            )
        );
        assert!(
            parse_move_paths(
                "eng:/telemetry/flight-042.mcap".to_string(),
                "ops:/archived/flight-042.mcap".to_string(),
            )
            .is_err()
        );
    }
}

fn print_drive(drive: &Drive) {
    println!("RID: {}", drive.rid());
    println!("ID: {}", drive.id());
    println!("Source: {}", drive.source());
    println!("Content mutability: {}", drive.content_mutability());
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
