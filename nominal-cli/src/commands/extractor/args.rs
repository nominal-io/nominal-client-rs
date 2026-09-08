use crate::args::{OutputArgs, WaitArgs};
use clap::{Args, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Args, Default)]
pub struct ScopeArgs {
    #[arg(long)]
    pub workspace: Option<String>,
    #[command(flatten)]
    pub output: OutputArgs,
}
#[derive(Args)]
pub struct RidArgs {
    pub rid: String,
    #[command(flatten)]
    pub scope: ScopeArgs,
}
#[derive(Subcommand)]
pub enum ExtractorCommands {
    /// Create an extractor.
    Create {
        name: String,
        #[arg(long)]
        description: Option<String>,
        #[command(flatten)]
        scope: ScopeArgs,
    },
    /// Inspect a resource by RID.
    Get(RidArgs),
    /// Search resources in the selected workspace.
    Search {
        #[arg(long)]
        include_archived: bool,
        #[arg(long)]
        file_extension: Option<String>,
        #[command(flatten)]
        scope: ScopeArgs,
    },
    /// Update the extractor name or description.
    Update {
        rid: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        description: Option<String>,
        #[command(flatten)]
        scope: ScopeArgs,
    },
    /// Archive an extractor.
    Archive(RidArgs),
    /// Restore an archived extractor.
    Unarchive(RidArgs),
    /// Activate an image, waiting for readiness by default.
    Activate {
        rid: String,
        image_rid: String,
        #[command(flatten)]
        wait: WaitArgs,
        #[command(flatten)]
        scope: ScopeArgs,
    },
    /// Register and inspect container images.
    Image {
        #[command(subcommand)]
        command: ImageCommands,
    },
}
#[derive(ValueEnum, Clone, Copy)]
pub enum ImageStatus {
    Pending,
    Ready,
    Failed,
}
#[derive(Subcommand)]
pub enum ImageCommands {
    /// Upload an image tarball using a version 1 JSON execution contract.
    Register {
        extractor_rid: String,
        tarball: PathBuf,
        #[arg(long)]
        contract: PathBuf,
        #[command(flatten)]
        scope: ScopeArgs,
    },
    /// Inspect a resource by RID.
    Get(RidArgs),
    /// Search resources in the selected workspace.
    Search {
        #[arg(long)]
        extractor: Option<String>,
        #[arg(long)]
        tag: Option<String>,
        #[arg(long)]
        status: Option<ImageStatus>,
        #[command(flatten)]
        scope: ScopeArgs,
    },
    /// Wait for image readiness.
    Wait {
        rid: String,
        #[arg(long, value_parser=clap::value_parser!(u64).range(1..))]
        timeout: Option<u64>,
        #[command(flatten)]
        scope: ScopeArgs,
    },
    /// Delete an image.
    Delete(RidArgs),
}
