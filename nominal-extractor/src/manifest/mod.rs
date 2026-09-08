mod tabular;
mod video;
mod wire;
use crate::{Error, Result, environment::Environment, paths::OutputDirectory};
use std::{collections::BTreeMap, path::PathBuf};
pub use tabular::*;
pub use video::*;
pub struct ManifestContext {
    env: Environment,
    output: OutputDirectory,
    manifest: wire::Manifest,
}
impl ManifestContext {
    pub(crate) fn new(env: BTreeMap<String, String>) -> Result<Self> {
        let env = Environment::new(env, true)?;
        let output = OutputDirectory::new(
            env.value("OUTPUT_DIR")
                .ok_or_else(|| Error::MissingEnvironment("OUTPUT_DIR".into()))?,
        )?;
        Ok(Self {
            env,
            output,
            manifest: wire::Manifest {
                outputs: vec![],
                video_outputs: vec![],
            },
        })
    }
    pub fn add_tabular(&mut self, out: TabularOutput) -> Result<PathBuf> {
        self.record(
            out.path,
            &[".csv", ".csv.gz", ".parquet", ".parquet.gz"],
            wire::Output {
                ingest_type: "TABULAR",
                relative_path: String::new(),
                tag_columns: out.tags,
                channel_prefix: out.prefix,
                timestamp_metadata: out.timestamp,
            },
        )
    }
    pub fn add_avro_stream(&mut self, out: AvroStreamOutput) -> Result<PathBuf> {
        self.record(
            out.path,
            &[".avro", ".avro.gz"],
            wire::Output {
                ingest_type: "AVRO_STREAM",
                relative_path: String::new(),
                tag_columns: BTreeMap::new(),
                channel_prefix: out.prefix,
                timestamp_metadata: out.timestamp,
            },
        )
    }
    pub fn add_journal_json(&mut self, out: JournalJsonOutput) -> Result<PathBuf> {
        self.record(
            out.path,
            &[".jsonl", ".jsonl.gz"],
            wire::Output {
                ingest_type: "JSON_L",
                relative_path: String::new(),
                tag_columns: BTreeMap::new(),
                channel_prefix: None,
                timestamp_metadata: out.timestamp,
            },
        )
    }
    fn record(
        &mut self,
        path: PathBuf,
        extensions: &[&str],
        mut entry: wire::Output,
    ) -> Result<PathBuf> {
        let (path, relative) = self.output.resolve(&path, true)?;
        crate::paths::extension(&path, extensions)?;
        entry.relative_path = relative.clone();
        self.manifest.outputs.push(entry);
        self.output.account(relative);
        Ok(path)
    }
    pub fn build_manifest(&self) -> Result<serde_json::Value> {
        Ok(serde_json::to_value(&self.manifest)?)
    }
    pub(crate) fn finalize(&self) -> Result<()> {
        if self.manifest.outputs.is_empty() && self.manifest.video_outputs.is_empty() {
            return Err(Error::EmptyOutputs);
        }
        self.output.warn_strays()?;
        self.output.write_manifest(&self.manifest)?;
        tracing::info!("wrote extractor manifest");
        Ok(())
    }

    pub fn inputs(&self) -> Result<Vec<PathBuf>> {
        self.env.inputs()
    }
    pub fn input(&self, name: &str) -> Result<PathBuf> {
        self.env.input(name)
    }
    pub fn sole_input(&self) -> Result<PathBuf> {
        self.env.sole_input()
    }
    pub fn output_dir(&self) -> &std::path::Path {
        &self.output.root
    }
    pub fn param<T: std::str::FromStr>(&self, name: &str) -> Result<T>
    where
        T::Err: std::fmt::Display,
    {
        self.env.param(name)
    }
    pub fn optional_param<T: std::str::FromStr>(&self, name: &str) -> Result<Option<T>>
    where
        T::Err: std::fmt::Display,
    {
        self.env.optional_param(name)
    }
    pub fn ingest_job_rid(&self) -> Option<&str> {
        self.env.value("_NOMINAL_INGEST_JOB_RID")
    }
    pub fn dataset_rid(&self) -> Option<&str> {
        self.env.value("_NOMINAL_DATASET_RID")
    }
    pub fn additional_tags(&self) -> &std::collections::BTreeMap<String, String> {
        &self.env.tags
    }
    pub fn job_timestamp_metadata(&self) -> Option<&crate::JobTimestampMetadata> {
        self.env.timestamp.as_ref()
    }
}
