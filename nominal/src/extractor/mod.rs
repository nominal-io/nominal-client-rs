//! Write a Rust extractor that produces files and a manifest for Nominal.
//!
//! Register the container image with the `manifest` output format. In the extractor,
//! read inputs through [`ManifestContext`], write files under its output directory,
//! and declare each completed file with an `add_*` method. [`run_manifest`] writes
//! `manifest.json` after the callback succeeds. One manifest can contain one or many
//! outputs, including multiple declarations of the same file.
//!
//! The runtime is synchronous and experimental. It reads local files and environment
//! metadata; it does not upload data or require credentials or an async executor.
//!
//! # Example
//!
//! This example copies an input CSV and declares its timestamp column. A real
//! extractor replaces the copy with the decoding needed for its input format.
//!
//! ```
//! use nominal::extractor::{
//!     Result, ManifestContext,
//!     TabularOptions, run_manifest_with_env,
//! };
//! use nominal::core::{TimeUnit, Timestamp};
//! use std::collections::BTreeMap;
//!
//! fn extract(ctx: &mut ManifestContext) -> Result {
//!     let input = ctx.input("DATA")?;
//!     let output = ctx.output_dir().join("telemetry.csv");
//!     std::fs::copy(input, &output)?;
//!     // Declare the file after writing it. Nominal reads ts as epoch nanoseconds.
//!     ctx.add_tabular("telemetry.csv", TabularOptions::new()
//!         .timestamp(Timestamp::epoch("ts", TimeUnit::Nanoseconds)))?;
//!     Ok(())
//! }
//!
//! // Create local files for this example. The output directory must already exist.
//! let dir = tempfile::tempdir()?;
//! let input = dir.path().join("input.csv");
//! std::fs::write(&input, "ts,value\n0,1\n")?;
//! let output = dir.path().join("output");
//! std::fs::create_dir(&output)?;
//! // Supply an environment map for a local test without changing process state.
//! let env = BTreeMap::from([
//!     ("DATA".into(), input.display().to_string()),
//!     ("OUTPUT_DIR".into(), output.display().to_string()),
//! ]);
//! run_manifest_with_env(env, extract)?;
//! # assert!(output.join("manifest.json").is_file());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! In a container, call `run_manifest(extract)` from `main` to read the environment
//! provided by Nominal. Return its error from `main` so extraction failure gives
//! the container a nonzero exit status. See the extractor guide for a Docker recipe.
mod environment;
mod error;
mod manifest;
mod timestamp;
pub use error::Error;
pub use manifest::*;
/// A runtime operation result, with `()` as the default success value.
pub type Result<T = ()> = std::result::Result<T, Error>;

use std::collections::BTreeMap;
/// Runs the callback using the environment provided to the container.
///
/// `OUTPUT_DIR` must name an existing directory. Registered inputs and parameters
/// are supplied by Nominal; without registration metadata, names refer directly to
/// environment variables. The callback must declare at least one completed output.
///
/// Returns success after writing the manifest. An existing manifest is replaced
/// only after the callback succeeds. Emits lifecycle events through `tracing`; the
/// application configures its subscriber, as shown in the extractor guide.
///
/// # Errors
///
/// Returns an error for invalid environment metadata, an incompatible registered
/// output format, missing output declarations, or a filesystem failure. Callback
/// errors are wrapped in [`Error::Author`]; the runner does not write a new manifest
/// after a callback error. Files already written by the callback are left on disk.
pub fn run_manifest<E>(
    extract: impl FnOnce(&mut ManifestContext) -> std::result::Result<(), E>,
) -> Result
where
    E: std::error::Error + Send + Sync + 'static,
{
    run_manifest_with_env(std::env::vars().collect(), extract)
}
/// Runs the callback with an explicit environment map, leaving process state unchanged.
///
/// Use this for local tests or an embedding application. The map contains the same
/// keys that [`run_manifest`] reads from the process environment, including
/// `OUTPUT_DIR` and any named input paths. All paths refer to the local filesystem;
/// relative paths resolve from the current working directory.
///
/// Uses the current tracing subscriber without installing one. Returns success
/// after publication and has the same errors and manifest-writing behavior as
/// [`run_manifest`]. The callback can return any error implementing
/// `std::error::Error + Send + Sync + 'static`, including this module's [`Error`].
pub fn run_manifest_with_env<E>(
    env: BTreeMap<String, String>,
    extract: impl FnOnce(&mut ManifestContext) -> std::result::Result<(), E>,
) -> Result
where
    E: std::error::Error + Send + Sync + 'static,
{
    tracing::info!("starting manifest extractor");
    let mut context = ManifestContext::new(env).map_err(|error| {
        tracing::error!(phase = "setup", error = %error, "extractor failed");
        error
    })?;
    tracing::info!(output_dir = %context.output_dir().display(), "starting extraction");
    extract(&mut context).map_err(|source| {
        let error = Error::Author {
            source: Box::new(source),
        };
        tracing::error!(phase = "callback", error = %error, "extractor failed");
        error
    })?;
    context.finalize().map_err(|error| {
        tracing::error!(phase = "finalize", error = %error, "extractor failed");
        error
    })?;
    tracing::info!("extractor completed");
    Ok(())
}
