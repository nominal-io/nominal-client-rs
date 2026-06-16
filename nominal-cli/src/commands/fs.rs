use anyhow::bail;
use clap::Subcommand;

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
    /// Create a managed drive
    Create {
        /// The drive name
        #[arg(short, long)]
        name: String,
    },
    /// Get a drive by RID
    Get {
        /// The RID of the drive to retrieve
        drive_rid: String,
    },
    /// Get virtual drive details by drive RID
    GetVirtual {
        /// The RID of the virtual drive to retrieve
        drive_rid: String,
    },
    /// List drives in a workspace
    List {
        /// Include archived drives
        #[arg(long)]
        include_archived: bool,
    },
}

pub async fn handle(cmd: FsCommands) -> anyhow::Result<()> {
    match cmd {
        FsCommands::Drive { drive_command } => handle_drive(drive_command).await,
    }
}

async fn handle_drive(cmd: DriveCommands) -> anyhow::Result<()> {
    match cmd {
        DriveCommands::Create { name } => {
            let _ = name;
            file_store_unimplemented()
        }
        DriveCommands::Get { drive_rid } => {
            let _ = drive_rid;
            file_store_unimplemented()
        }
        DriveCommands::GetVirtual { drive_rid } => {
            let _ = drive_rid;
            file_store_unimplemented()
        }
        DriveCommands::List { include_archived } => {
            let _ = include_archived;
            file_store_unimplemented()
        }
    }
}

fn file_store_unimplemented() -> anyhow::Result<()> {
    bail!(
        "file store drive commands are not implemented yet; wire this to nominal.file_store.v1 once the API crate is published"
    )
}
