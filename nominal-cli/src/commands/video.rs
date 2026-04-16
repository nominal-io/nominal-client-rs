use anyhow::Context;
use chrono::SecondsFormat;
use clap::Subcommand;
use nominal::core::{NominalClient, VideoCreate, VideoUpdate};

#[derive(Subcommand)]
pub enum VideoCommands {
    /// List all videos
    List,
    /// Get a specific video by RID
    Get {
        /// The RID of the video to retrieve
        rid: String,
    },
    /// Create a new video
    Create {
        /// The video name
        #[arg(short, long)]
        name: String,

        /// Set the video description
        #[arg(short, long)]
        description: Option<String>,

        /// Add labels. Repeatable
        #[arg(short, long = "label", value_name = "LABEL")]
        labels: Vec<String>,

        /// Add properties as KEY VALUE pairs. Repeatable
        #[arg(short, long = "property", value_names = ["KEY", "VALUE"], num_args = 2, action = clap::ArgAction::Append)]
        properties: Vec<String>,
    },
    /// Update video metadata
    Update {
        /// The RID of the video to update
        rid: String,

        /// Set the video name
        #[arg(short, long)]
        name: Option<String>,

        /// Set the video description
        #[arg(short, long)]
        description: Option<String>,

        /// Replace all labels. Repeatable. Omit to leave labels unchanged
        #[arg(
            short,
            long = "label",
            value_name = "LABEL",
            conflicts_with = "clear_labels"
        )]
        labels: Vec<String>,

        /// Clear all labels
        #[arg(long, conflicts_with = "labels")]
        clear_labels: bool,

        /// Replace all properties as KEY VALUE pairs. Repeatable. Omit to leave properties unchanged
        #[arg(short, long = "property", value_names = ["KEY", "VALUE"], num_args = 2, action = clap::ArgAction::Append, conflicts_with = "clear_properties")]
        properties: Vec<String>,

        /// Clear all properties
        #[arg(long, conflicts_with = "properties")]
        clear_properties: bool,
    },
    /// Archive a video
    Archive {
        /// The RID of the video to archive
        rid: String,
    },
    /// Unarchive a video
    Unarchive {
        /// The RID of the video to unarchive
        rid: String,
    },
}

pub async fn handle(cmd: VideoCommands, client: NominalClient) -> anyhow::Result<()> {
    match cmd {
        VideoCommands::List => {
            let videos = client
                .catalog()
                .list_videos()
                .await
                .context("Failed to list videos")?;

            for video in videos {
                println!("{}", video.rid());
            }
        }
        VideoCommands::Get { rid } => {
            let video = client
                .catalog()
                .get_video(&rid)
                .await
                .with_context(|| format!("Failed to get video '{rid}'"))?;

            print_video(&video);
        }
        VideoCommands::Create {
            name,
            description,
            labels,
            properties,
        } => {
            let mut create = VideoCreate::new(name);

            if let Some(d) = description {
                create = create.description(d);
            }
            if !labels.is_empty() {
                create = create.labels(labels);
            }
            if !properties.is_empty() {
                let props: std::collections::HashMap<_, _> = properties
                    .chunks(2)
                    .map(|pair| (pair[0].clone(), pair[1].clone()))
                    .collect();
                create = create.properties(props);
            }

            let video = client
                .catalog()
                .create_video(create)
                .await
                .context("Failed to create video")?;

            print_video(&video);
        }
        VideoCommands::Update {
            rid,
            name,
            description,
            labels,
            clear_labels,
            properties,
            clear_properties,
        } => {
            let mut update = VideoUpdate::new();

            if let Some(n) = name {
                update = update.name(n);
            }
            if let Some(d) = description {
                update = update.description(d);
            }
            if clear_labels {
                update = update.labels([] as [String; 0]);
            } else if !labels.is_empty() {
                update = update.labels(labels);
            }
            if clear_properties {
                update = update.properties([] as [(String, String); 0]);
            } else if !properties.is_empty() {
                let props: std::collections::HashMap<_, _> = properties
                    .chunks(2)
                    .map(|pair| (pair[0].clone(), pair[1].clone()))
                    .collect();
                update = update.properties(props);
            }

            let video = client
                .catalog()
                .update_video(&rid, update)
                .await
                .with_context(|| format!("Failed to update video '{rid}'"))?;

            print_video(&video);
        }
        VideoCommands::Archive { rid } => {
            client
                .catalog()
                .archive_video(&rid)
                .await
                .with_context(|| format!("Failed to archive video '{rid}'"))?;

            println!("Archived video: {rid}");
        }
        VideoCommands::Unarchive { rid } => {
            client
                .catalog()
                .unarchive_video(&rid)
                .await
                .with_context(|| format!("Failed to unarchive video '{rid}'"))?;

            println!("Unarchived video: {rid}");
        }
    }

    Ok(())
}

fn print_video(video: &nominal::core::Video) {
    println!("RID: {}", video.rid());
    println!("Name: {}", video.name());
    if let Some(description) = video.description() {
        println!("Description: {description}");
    }
    if !video.labels().is_empty() {
        println!("Labels: {}", video.labels().join(", "));
    }
    if !video.properties().is_empty() {
        println!("Properties:");
        for (key, value) in video.properties() {
            println!("  {key}: {value}");
        }
    }
    println!(
        "Created: {}",
        video
            .created_at()
            .to_rfc3339_opts(SecondsFormat::Nanos, true)
    );
    println!("URL: {}", video.nominal_url());
}
