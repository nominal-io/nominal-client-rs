pub mod args;
mod contract;
pub(crate) mod render;
use crate::{args::wait_options, output::emit};
pub use args::ExtractorCommands;
use args::*;
use nominal::core::*;
use render::*;

fn scoped(
    profile: Option<&str>,
    scope: &ScopeArgs,
) -> anyhow::Result<(ExtractorsClient, ContainerImagesClient)> {
    let client = crate::commands::load_client(profile)?;
    let mut e = client.extractors();
    let mut i = client.container_images();
    if let Some(w) = &scope.workspace {
        e = e.in_workspace(w);
        i = i.in_workspace(w)
    }
    Ok((e, i))
}
pub async fn handle(cmd: ExtractorCommands, profile: Option<&str>) -> anyhow::Result<()> {
    match cmd {
        ExtractorCommands::Create {
            name,
            description,
            scope,
        } => {
            let (e, _) = scoped(profile, &scope)?;
            let mut options = ExtractorCreate::new(name);
            if let Some(d) = description {
                options = options.description(d)
            }
            emit(
                &ExtractorView::from(&e.create(options).await?),
                scope.output.json,
            )
        }
        ExtractorCommands::Get(a) => {
            let (e, _) = scoped(profile, &a.scope)?;
            emit(
                &ExtractorView::from(&e.get(&a.rid).await?),
                a.scope.output.json,
            )
        }
        ExtractorCommands::Search {
            include_archived,
            file_extension,
            scope,
        } => {
            let (e, _) = scoped(profile, &scope)?;
            let mut q = ExtractorQuery::default().include_archived(include_archived);
            if let Some(v) = file_extension {
                q = q.file_extension(v)
            }
            let values = e.search(q).await?;
            emit(
                &values.iter().map(ExtractorView::from).collect::<Vec<_>>(),
                scope.output.json,
            )
        }
        ExtractorCommands::Update {
            rid,
            name,
            description,
            scope,
        } => {
            let (e, _) = scoped(profile, &scope)?;
            let resource = e.get(&rid).await?;
            let mut options = ExtractorUpdate::default();
            if let Some(v) = name {
                options = options.name(v)
            }
            if let Some(v) = description {
                options = options.description(v)
            }
            emit(
                &ExtractorView::from(&e.update(&resource, options).await?),
                scope.output.json,
            )
        }
        ExtractorCommands::Archive(a) => {
            let (e, _) = scoped(profile, &a.scope)?;
            let resource = e.get(&a.rid).await?;
            emit(
                &ExtractorView::from(&e.archive(&resource).await?),
                a.scope.output.json,
            )
        }
        ExtractorCommands::Unarchive(a) => {
            let (e, _) = scoped(profile, &a.scope)?;
            let resource = e.get(&a.rid).await?;
            emit(
                &ExtractorView::from(&e.unarchive(&resource).await?),
                a.scope.output.json,
            )
        }
        ExtractorCommands::Activate {
            rid,
            image_rid,
            wait,
            scope,
        } => {
            let (e, i) = scoped(profile, &scope)?;
            let resource = e.get(&rid).await?;
            let image = i.get(&image_rid).await?;
            let mode = if wait.no_wait {
                Activation::RequireReady
            } else {
                Activation::Wait(wait.options())
            };
            emit(
                &ExtractorView::from(&e.activate(&resource, &image, mode).await?),
                scope.output.json,
            )
        }
        ExtractorCommands::Image { command } => images(command, profile).await,
    }
}
async fn images(cmd: ImageCommands, profile: Option<&str>) -> anyhow::Result<()> {
    match cmd {
        ImageCommands::Register {
            extractor_rid,
            tarball,
            contract,
            scope,
        } => {
            let dto: contract::ImageContract = crate::contract::read(&contract)?;
            let registration = dto.try_into()?;
            let (e, i) = scoped(profile, &scope)?;
            let extractor = e.get(&extractor_rid).await?;
            emit(
                &ImageView::from(&i.register(&extractor, &tarball, registration).await?),
                scope.output.json,
            )
        }
        ImageCommands::Get(a) => {
            let (_, i) = scoped(profile, &a.scope)?;
            emit(&ImageView::from(&i.get(&a.rid).await?), a.scope.output.json)
        }
        ImageCommands::Wait {
            rid,
            timeout,
            scope,
        } => {
            let (_, i) = scoped(profile, &scope)?;
            let image = i.get(&rid).await?;
            emit(
                &ImageView::from(&i.wait_ready(&image, wait_options(timeout)).await?),
                scope.output.json,
            )
        }
        ImageCommands::Delete(a) => {
            let (_, i) = scoped(profile, &a.scope)?;
            let image = i.get(&a.rid).await?;
            i.delete(&image).await?;
            emit(
                &Deleted {
                    rid: &a.rid,
                    deleted: true,
                },
                a.scope.output.json,
            )
        }
        ImageCommands::Search {
            extractor,
            tag,
            status,
            scope,
        } => {
            let (_, i) = scoped(profile, &scope)?;
            let mut q = ContainerImageQuery::default();
            if let Some(v) = extractor {
                q = q.extractor(v)
            }
            if let Some(v) = tag {
                q = q.tag(v)
            }
            if let Some(v) = status {
                q = q.status(match v {
                    ImageStatus::Pending => ContainerImageStatus::Pending,
                    ImageStatus::Ready => ContainerImageStatus::Ready,
                    ImageStatus::Failed => ContainerImageStatus::Failed,
                })
            }
            let values = i.search(q).await?;
            emit(
                &values.iter().map(ImageView::from).collect::<Vec<_>>(),
                scope.output.json,
            )
        }
    }
}
