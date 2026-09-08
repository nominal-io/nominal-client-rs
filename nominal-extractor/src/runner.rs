use crate::{Error, ExtractResult, ManifestContext, Result, SingleFileContext};
use std::collections::BTreeMap;
pub fn run_single_file(
    extract: impl FnOnce(&mut SingleFileContext) -> ExtractResult,
) -> Result<SingleFileContext> {
    run_single_file_with_env(std::env::vars().collect(), extract)
}
pub fn run_single_file_with_env(
    env: BTreeMap<String, String>,
    extract: impl FnOnce(&mut SingleFileContext) -> ExtractResult,
) -> Result<SingleFileContext> {
    let mut context = SingleFileContext::new(env)?;
    extract(&mut context).map_err(|source| Error::Author { source })?;
    context.finalize()?;
    Ok(context)
}
pub fn run_manifest(
    extract: impl FnOnce(&mut ManifestContext) -> ExtractResult,
) -> Result<ManifestContext> {
    run_manifest_with_env(std::env::vars().collect(), extract)
}
pub fn run_manifest_with_env(
    env: BTreeMap<String, String>,
    extract: impl FnOnce(&mut ManifestContext) -> ExtractResult,
) -> Result<ManifestContext> {
    let mut context = ManifestContext::new(env)?;
    extract(&mut context).map_err(|source| Error::Author { source })?;
    context.finalize()?;
    Ok(context)
}
