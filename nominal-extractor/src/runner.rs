use crate::{Error, ExtractResult, ManifestContext, Result, SingleFileContext};
use std::collections::BTreeMap;
/// Run an extractor using the process environment and require exactly one output file.
pub fn run_single_file(
    extract: impl FnOnce(&mut SingleFileContext) -> ExtractResult,
) -> Result<SingleFileContext> {
    run_single_file_with_env(std::env::vars().collect(), extract)
}
/// Run a single-file extractor using the supplied environment map.
/// Leave the process environment unchanged.
pub fn run_single_file_with_env(
    env: BTreeMap<String, String>,
    extract: impl FnOnce(&mut SingleFileContext) -> ExtractResult,
) -> Result<SingleFileContext> {
    let mut context = SingleFileContext::new(env)?;
    extract(&mut context).map_err(|source| Error::Author { source })?;
    context.finalize()?;
    Ok(context)
}
/// Run an extractor using the process environment and write its manifest on success.
pub fn run_manifest(
    extract: impl FnOnce(&mut ManifestContext) -> ExtractResult,
) -> Result<ManifestContext> {
    run_manifest_with_env(std::env::vars().collect(), extract)
}
/// Run a manifest extractor using the supplied environment map.
/// Leave the process environment unchanged.
pub fn run_manifest_with_env(
    env: BTreeMap<String, String>,
    extract: impl FnOnce(&mut ManifestContext) -> ExtractResult,
) -> Result<ManifestContext> {
    let mut context = ManifestContext::new(env)?;
    extract(&mut context).map_err(|source| Error::Author { source })?;
    context.finalize()?;
    Ok(context)
}
