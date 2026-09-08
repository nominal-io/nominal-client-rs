use crate::{Error, Result};
use std::{
    collections::BTreeSet,
    io::Write,
    path::{Path, PathBuf},
};
pub(crate) struct OutputDirectory {
    pub root: PathBuf,
    accounted: BTreeSet<String>,
}
impl OutputDirectory {
    pub fn new(path: &str) -> Result<Self> {
        Ok(Self {
            root: std::fs::canonicalize(path)?,
            accounted: BTreeSet::new(),
        })
    }
    pub fn resolve(&self, path: &Path, reserved: bool) -> Result<(PathBuf, String)> {
        let resolved = std::fs::canonicalize(path)?;
        if !resolved.is_file() {
            return Err(Error::InvalidOutput(format!(
                "{} is not a file",
                path.display()
            )));
        }
        let relative = resolved.strip_prefix(&self.root).map_err(|_| {
            Error::InvalidOutput(format!("{} is outside output directory", path.display()))
        })?;
        let relative = wire_path(relative)?;
        if reserved && relative == "manifest.json" {
            return Err(Error::InvalidOutput("manifest.json is reserved".into()));
        }
        // Validate the author's filename extension, but retain canonical wire identity.
        Ok((path.to_path_buf(), relative))
    }
    pub fn account(&mut self, path: String) {
        self.accounted.insert(path);
    }
    pub fn warn_strays(&self) -> Result<()> {
        self.walk(&self.root)
    }
    fn walk(&self, dir: &Path) -> Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let p = entry.path();
            let t = entry.file_type()?;
            if t.is_dir() {
                self.walk(&p)?;
            } else if p.is_file() {
                let relative = p.strip_prefix(&self.root).expect("walk under root");
                if !wire_path(relative).is_ok_and(|rel| self.accounted.contains(&rel)) {
                    tracing::warn!(path=%relative.display(),"undeclared output file will not be ingested");
                }
            }
        }
        Ok(())
    }
    pub fn write_manifest(&self, value: &impl serde::Serialize) -> Result<()> {
        let mut temp = tempfile::NamedTempFile::new_in(&self.root)?;
        serde_json::to_writer(&mut temp, value)?;
        temp.flush()?;
        temp.persist(self.root.join("manifest.json"))
            .map_err(|e| e.error)?;
        Ok(())
    }
}

/// Convert separators between components, never characters inside a filename.
fn wire_path(path: &Path) -> Result<String> {
    path.components()
        .map(|component| {
            component
                .as_os_str()
                .to_str()
                .map(str::to_owned)
                .ok_or_else(|| {
                    Error::InvalidOutput("output filename cannot be represented as UTF-8".into())
                })
        })
        .collect::<Result<Vec<_>>>()
        .map(|parts| parts.join("/"))
}
pub(crate) fn extension(path: &Path, allowed: &[&str]) -> Result<()> {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    if allowed.iter().any(|ext| name.ends_with(ext)) {
        Ok(())
    } else {
        Err(Error::InvalidOutput(format!(
            "unsupported extension: {}",
            path.display()
        )))
    }
}
