use super::render::{FileView, JobView, wait_job};
use crate::commands::extractor::{
    args::{OutputArgs, wait_options},
    render::emit,
};
use clap::{Args, Subcommand, ValueEnum};
use nominal::core::*;
#[derive(Args)]
pub struct JobRidArgs {
    rid: String,
    #[command(flatten)]
    output: OutputArgs,
}
#[derive(Clone, ValueEnum)]
pub enum Status {
    Submitted,
    Queued,
    InProgress,
    Completed,
    Failed,
    Cancelled,
}
#[derive(Subcommand)]
pub enum JobCommands {
    Get(JobRidArgs),
    Cancel(JobRidArgs),
    Wait {
        rid: String,
        #[arg(long,value_parser=clap::value_parser!(u64).range(1..))]
        timeout: Option<u64>,
        #[command(flatten)]
        output: OutputArgs,
    },
    Files {
        rid: String,
        #[arg(long)]
        wait: bool,
        #[arg(long,requires="wait",value_parser=clap::value_parser!(u64).range(1..))]
        timeout: Option<u64>,
        #[command(flatten)]
        output: OutputArgs,
    },
    Search {
        #[arg(long = "dataset")]
        datasets: Vec<String>,
        #[arg(long = "created-by")]
        creators: Vec<String>,
        #[arg(long)]
        status: Vec<Status>,
        #[arg(long)]
        search_text: Option<String>,
        #[arg(long)]
        start_after: Option<chrono::DateTime<chrono::Utc>>,
        #[arg(long)]
        start_before: Option<chrono::DateTime<chrono::Utc>>,
        #[arg(long, conflicts_with = "all_workspaces")]
        workspace: Option<String>,
        #[arg(long)]
        all_workspaces: bool,
        #[command(flatten)]
        output: OutputArgs,
    },
}
pub async fn handle(cmd: JobCommands, client: NominalClient) -> anyhow::Result<()> {
    let ingest = client.ingest();
    match cmd {
        JobCommands::Get(a) => emit(
            &JobView::from(&ingest.get_ingest_job(&a.rid).await?),
            a.output.json,
        ),
        JobCommands::Cancel(a) => emit(
            &JobView::from(&ingest.cancel_ingest_job(&a.rid).await?),
            a.output.json,
        ),
        JobCommands::Wait {
            rid,
            timeout,
            output,
        } => emit(
            &JobView::from(&wait_job(&ingest, &rid, timeout).await?),
            output.json,
        ),
        JobCommands::Files {
            rid,
            wait,
            timeout,
            output,
        } => {
            let files = if wait {
                ingest
                    .wait_for_job_files(&rid, wait_options(timeout))
                    .await?
            } else {
                ingest.dataset_files(&rid).await?
            };
            emit(
                &files.iter().map(FileView::from).collect::<Vec<_>>(),
                output.json,
            )
        }
        JobCommands::Search {
            datasets,
            creators,
            status,
            search_text,
            start_after,
            start_before,
            workspace,
            all_workspaces,
            output,
        } => {
            let mut q = IngestJobQuery::default().workspace(if all_workspaces {
                WorkspaceSelection::All
            } else if let Some(rid) = workspace {
                WorkspaceSelection::Specific(rid)
            } else {
                WorkspaceSelection::Default
            });
            for v in datasets {
                q = q.dataset(v)
            }
            for v in creators {
                q = q.created_by(v)
            }
            for v in status {
                q = q.status(match v {
                    Status::Submitted => IngestJobStatus::Submitted,
                    Status::Queued => IngestJobStatus::Queued,
                    Status::InProgress => IngestJobStatus::InProgress,
                    Status::Completed => IngestJobStatus::Completed,
                    Status::Failed => IngestJobStatus::Failed,
                    Status::Cancelled => IngestJobStatus::Cancelled,
                })
            }
            if let Some(v) = search_text {
                q = q.search_text(v)
            }
            if let Some(v) = start_after {
                q = q.start_after(v)
            }
            if let Some(v) = start_before {
                q = q.start_before(v)
            }
            let jobs = ingest.search_ingest_jobs(q).await?;
            emit(
                &jobs.iter().map(JobView::from).collect::<Vec<_>>(),
                output.json,
            )
        }
    }
}
