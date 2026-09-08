use crate::{NumericTimestamp, timestamp::TimestampMetadata};
use std::{collections::BTreeMap, path::PathBuf};
/// A CSV or Parquet output with optional channel and timestamp settings.
pub struct TabularOutput {
    pub(crate) path: PathBuf,
    pub(crate) tags: BTreeMap<String, String>,
    pub(crate) prefix: Option<String>,
    pub(crate) timestamp: Option<TimestampMetadata>,
}
impl TabularOutput {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            tags: BTreeMap::new(),
            prefix: None,
            timestamp: None,
        }
    }
    pub fn tag_column(mut self, key: impl Into<String>, column: impl Into<String>) -> Self {
        self.tags.insert(key.into(), column.into());
        self
    }
    pub fn channel_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }
    pub fn timestamp(mut self, column: impl Into<String>, timestamp: NumericTimestamp) -> Self {
        self.timestamp = Some(TimestampMetadata::new(column.into(), timestamp));
        self
    }
}
/// An Avro stream output. The timestamp field is always `timestamps`.
///
/// ```compile_fail
/// use nominal_extractor::*;
/// AvroStreamOutput::new("x.avro").timestamp("column", NumericTimestamp::Epoch(NumericTimeUnit::Seconds));
/// ```
pub struct AvroStreamOutput {
    pub(crate) path: PathBuf,
    pub(crate) prefix: Option<String>,
    pub(crate) timestamp: Option<TimestampMetadata>,
}
impl AvroStreamOutput {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            prefix: None,
            timestamp: None,
        }
    }
    pub fn channel_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }
    pub fn timestamp(mut self, timestamp: NumericTimestamp) -> Self {
        self.timestamp = Some(TimestampMetadata::new("timestamps".into(), timestamp));
        self
    }
}
/// A journal JSON output with optional timestamps. Tag columns and channel
/// prefixes are not supported.
///
/// ```compile_fail
/// use nominal_extractor::*;
/// JournalJsonOutput::new("x.jsonl").tag_column("tag", "column");
/// ```
pub struct JournalJsonOutput {
    pub(crate) path: PathBuf,
    pub(crate) timestamp: Option<TimestampMetadata>,
}
impl JournalJsonOutput {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            timestamp: None,
        }
    }
    pub fn timestamp(mut self, column: impl Into<String>, timestamp: NumericTimestamp) -> Self {
        self.timestamp = Some(TimestampMetadata::new(column.into(), timestamp));
        self
    }
}
