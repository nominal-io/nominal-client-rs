use crate::extractor::{Error, Result};
use nominal_api::objects::ingest::api::TimestampMetadata;
use serde::{Deserialize, de::DeserializeOwned};
use std::{collections::BTreeMap, fmt::Display, path::PathBuf, str::FromStr};
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InputSpec {
    environment_variable: String,
    name: Option<String>,
    path: PathBuf,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ParamSpec {
    environment_variable: String,
    name: Option<String>,
    required: bool,
}
pub(crate) struct Environment {
    env: BTreeMap<String, String>,
    inputs: Option<Vec<InputSpec>>,
    params: Option<Vec<ParamSpec>>,
    pub tags: BTreeMap<String, String>,
    pub timestamp: Option<TimestampMetadata>,
}
fn decode<T: DeserializeOwned>(env: &BTreeMap<String, String>, key: &str) -> Result<Option<T>> {
    let Some(value) = env.get(key).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    serde_json::from_str(value).map_err(|source| Error::Metadata {
        variable: key.into(),
        source,
    })
}
impl Environment {
    pub fn new(env: BTreeMap<String, String>) -> Result<Self> {
        if let Some(format) = env.get("_NOMINAL_OUTPUT_FORMAT").filter(|s| !s.is_empty()) {
            if format != "MANIFEST" {
                return Err(Error::FormatMismatch(format.clone()));
            }
        }
        let this = Self {
            inputs: decode(&env, "_NOMINAL_INPUTS")?,
            params: decode(&env, "_NOMINAL_PARAMETERS")?,
            tags: decode(&env, "_NOMINAL_ADDITIONAL_TAGS")?.unwrap_or_default(),
            timestamp: decode(&env, "_NOMINAL_TIMESTAMP_METADATA")?,
            env,
        };
        for input in this.inputs.iter().flatten() {
            if !input.path.is_file() {
                tracing::warn!(input=%input.environment_variable,"registered input is missing");
            }
        }
        for param in this.params.iter().flatten() {
            if param.required && !this.env.contains_key(&param.environment_variable) {
                tracing::warn!(parameter=%param.environment_variable,"required parameter is absent");
            }
        }
        Ok(this)
    }
    pub fn value(&self, key: &str) -> Option<&str> {
        self.env
            .get(key)
            .filter(|s| !s.is_empty())
            .map(String::as_str)
    }
    pub fn inputs(&self) -> Result<Vec<PathBuf>> {
        if let Some(inputs) = &self.inputs {
            return Ok(inputs.iter().map(|i| i.path.clone()).collect());
        }
        let dir = PathBuf::from(
            self.value("NOMINAL_EXTRACTOR_INPUT_DIR")
                .unwrap_or("/input"),
        );
        if !dir.is_dir() {
            return Ok(vec![]);
        }
        let mut paths = std::fs::read_dir(dir)?
            .map(|e| e.map(|e| e.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        paths.retain(|p| p.is_file());
        paths.sort();
        Ok(paths)
    }
    pub fn input(&self, name: &str) -> Result<PathBuf> {
        if let Some(specs) = &self.inputs {
            return specs
                .iter()
                .find(|s| s.environment_variable == name || s.name.as_deref() == Some(name))
                .map(|s| s.path.clone())
                .ok_or_else(|| Error::Input(name.into()));
        }
        self.value(name)
            .map(PathBuf::from)
            .ok_or_else(|| Error::Input(name.into()))
    }
    pub fn optional_param<T: FromStr>(&self, name: &str) -> Result<Option<T>>
    where
        T::Err: Display,
    {
        let variable = match &self.params {
            None => name,
            Some(specs) => {
                &specs
                    .iter()
                    .find(|s| s.environment_variable == name || s.name.as_deref() == Some(name))
                    .ok_or_else(|| Error::UnknownParameter(name.into()))?
                    .environment_variable
            }
        };
        self.env
            .get(variable)
            .map(|s| {
                s.parse().map_err(|e: T::Err| Error::ParseParameter {
                    name: name.into(),
                    message: e.to_string(),
                })
            })
            .transpose()
    }
}
