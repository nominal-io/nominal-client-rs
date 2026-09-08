use crate::{Error, Result, environment::Environment, paths::OutputDirectory};
use std::{collections::BTreeMap, path::PathBuf};
pub struct SingleFileContext {
    env: Environment,
    output: OutputDirectory,
    declared: Option<PathBuf>,
}
impl SingleFileContext {
    pub(crate) fn new(env: BTreeMap<String, String>) -> Result<Self> {
        let env = Environment::new(env, false)?;
        let output = OutputDirectory::new(
            env.value("OUTPUT_DIR")
                .ok_or_else(|| Error::MissingEnvironment("OUTPUT_DIR".into()))?,
        )?;
        Ok(Self {
            env,
            output,
            declared: None,
        })
    }
    pub fn set_output(&mut self, path: impl AsRef<std::path::Path>) -> Result<PathBuf> {
        if self.declared.is_some() {
            return Err(Error::InvalidOutput(
                "single output already declared".into(),
            ));
        }
        let (path, relative) = self.output.resolve(path.as_ref(), false)?;
        self.output.account(relative);
        self.declared = Some(path.clone());
        Ok(path)
    }
    pub(crate) fn finalize(&self) -> Result<()> {
        if self.declared.is_none() {
            return Err(Error::EmptyOutputs);
        }
        self.output.warn_strays()
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
