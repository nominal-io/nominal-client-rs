use anyhow::Context;
use clap::Subcommand;
use nominal::core::{ConnectionUpdate, NominalClient};

#[derive(Subcommand)]
pub enum ConnectionCommands {
    /// List all connections
    List,
    /// Get a specific connection by RID
    Get {
        /// The RID of the connection to retrieve
        rid: String,
    },
    /// Update connection metadata
    Update {
        /// The RID of the connection to update
        rid: String,

        /// Set the connection name
        #[arg(short, long)]
        name: Option<String>,

        /// Set the connection description
        #[arg(short, long)]
        description: Option<String>,
    },
    /// Archive a connection
    Archive {
        /// The RID of the connection to archive
        rid: String,
    },
    /// Unarchive a connection
    Unarchive {
        /// The RID of the connection to unarchive
        rid: String,
    },
}

pub async fn handle(cmd: ConnectionCommands, client: NominalClient) -> anyhow::Result<()> {
    match cmd {
        ConnectionCommands::List => {
            let connections = client
                .catalog()
                .list_connections()
                .await
                .context("Failed to list connections")?;

            for connection in connections {
                println!("{}", connection.rid());
            }
        }
        ConnectionCommands::Get { rid } => {
            let connection = client
                .catalog()
                .get_connection(&rid)
                .await
                .with_context(|| format!("Failed to get connection '{rid}'"))?;

            print_connection(&connection);
        }
        ConnectionCommands::Update {
            rid,
            name,
            description,
        } => {
            let mut update = ConnectionUpdate::new();

            if let Some(n) = name {
                update = update.name(n);
            }
            if let Some(d) = description {
                update = update.description(d);
            }

            let connection = client
                .catalog()
                .update_connection(&rid, update)
                .await
                .with_context(|| format!("Failed to update connection '{rid}'"))?;

            print_connection(&connection);
        }
        ConnectionCommands::Archive { rid } => {
            client
                .catalog()
                .archive_connection(&rid)
                .await
                .with_context(|| format!("Failed to archive connection '{rid}'"))?;

            println!("Archived connection: {rid}");
        }
        ConnectionCommands::Unarchive { rid } => {
            client
                .catalog()
                .unarchive_connection(&rid)
                .await
                .with_context(|| format!("Failed to unarchive connection '{rid}'"))?;

            println!("Unarchived connection: {rid}");
        }
    }

    Ok(())
}

fn print_connection(connection: &nominal::core::Connection) {
    println!("RID: {}", connection.rid());
    println!("Name: {}", connection.name());
    if let Some(description) = connection.description() {
        println!("Description: {description}");
    }
}
