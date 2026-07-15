use anyhow::Context;
use chrono::SecondsFormat;
use clap::Subcommand;
use nominal::core::{Drive, DrivesClient, NominalClient};

#[derive(Subcommand)]
pub enum FsCommands {
    /// Drive management commands
    Drive {
        #[command(subcommand)]
        drive_command: DriveCommands,
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
