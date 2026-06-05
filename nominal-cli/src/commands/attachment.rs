use anyhow::Context;
use chrono::SecondsFormat;
use clap::Subcommand;
use nominal::core::{Attachment, AttachmentCreate, NominalClient};
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum AttachmentCommands {
    /// Get an attachment by RID
    Get {
        /// The RID of the attachment to retrieve
        #[arg(short, long)]
        rid: String,
    },
    /// Upload a local file as an attachment
    Upload {
        /// Name for the attachment
        #[arg(short, long)]
        name: String,

        /// Path to the file to upload
        #[arg(short, long)]
        file: PathBuf,

        /// Description of the attachment
        #[arg(short, long)]
        description: Option<String>,
    },
    /// Download an attachment to disk
    Download {
        /// The RID of the attachment to download
        #[arg(short, long)]
        rid: String,

        /// Full path to write the attachment to
        #[arg(short, long)]
        output: PathBuf,
    },
}

pub async fn handle(cmd: AttachmentCommands, client: NominalClient) -> anyhow::Result<()> {
    match cmd {
        AttachmentCommands::Get { rid } => {
            let attachment = client
                .attachments()
                .get(&rid)
                .await
                .with_context(|| format!("Failed to get attachment '{rid}'"))?;
            print_attachment(&attachment);
        }
        AttachmentCommands::Upload {
            name,
            file,
            description,
        } => {
            let mut create = AttachmentCreate::from_path(file, name);
            if let Some(description) = description {
                create = create.description(description);
            }
            let attachment = client
                .attachments()
                .upload(create)
                .await
                .context("Failed to upload attachment")?;
            print_attachment(&attachment);
        }
        AttachmentCommands::Download { rid, output } => {
            client
                .attachments()
                .download_to(&rid, &output)
                .await
                .with_context(|| format!("Failed to download attachment '{rid}'"))?;
            println!("Downloaded attachment: {}", output.display());
        }
    }

    Ok(())
}

fn print_attachment(attachment: &Attachment) {
    println!("RID: {}", attachment.rid());
    println!("Name: {}", attachment.name());
    if let Some(description) = attachment.description() {
        println!("Description: {description}");
    }
    println!("File Type: {}", attachment.file_type());
    if !attachment.labels().is_empty() {
        println!("Labels: {}", attachment.labels().join(", "));
    }
    if !attachment.properties().is_empty() {
        println!("Properties:");
        for (key, value) in attachment.properties() {
            println!("  {key}: {value}");
        }
    }
    println!(
        "Created: {}",
        attachment
            .created_at()
            .to_rfc3339_opts(SecondsFormat::Nanos, true)
    );
    println!("Created By: {}", attachment.created_by_rid());
    println!("Archived: {}", attachment.is_archived());
}
