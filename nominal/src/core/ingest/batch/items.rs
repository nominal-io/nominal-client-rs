use super::super::{ContainerizedIngest, TimeUnit, Timestamp};
use chrono::{DateTime, Utc};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Debug, Clone)]
pub enum BatchNumericTimestamp {
    Epoch(TimeUnit),
    Relative {
        unit: TimeUnit,
        start: DateTime<Utc>,
    },
}
impl Default for BatchNumericTimestamp {
    fn default() -> Self {
        Self::Epoch(TimeUnit::Nanoseconds)
    }
}
impl BatchNumericTimestamp {
    pub(crate) fn timestamp(&self) -> Timestamp {
        match self {
            Self::Epoch(unit) => Timestamp::epoch("timestamps", *unit),
            Self::Relative { unit, start } => {
                Timestamp::relative("timestamps", *unit).with_offset(*start)
            }
        }
    }
}
#[derive(Debug, Clone, Default)]
pub enum Topics {
    #[default]
    All,
    Include(Vec<String>),
    Exclude(Vec<String>),
}
#[derive(Debug, Clone)]
pub enum BatchVideoTiming {
    Start(DateTime<Utc>),
    FrameTimestamps(Vec<i64>),
}
#[derive(Debug, Clone)]
pub struct BatchTabular {
    pub(crate) timestamp: Timestamp,
    pub(crate) tags: BTreeMap<String, String>,
    pub(crate) tag_columns: BTreeMap<String, String>,
    pub(crate) units: BTreeMap<String, String>,
    pub(crate) channel_prefix: Option<String>,
    pub(crate) channel_name_overrides: BTreeMap<String, String>,
}
impl BatchTabular {
    pub fn new(timestamp: Timestamp) -> Self {
        Self {
            timestamp,
            tags: BTreeMap::new(),
            tag_columns: BTreeMap::new(),
            units: BTreeMap::new(),
            channel_prefix: None,
            channel_name_overrides: BTreeMap::new(),
        }
    }
    pub fn tag_column(mut self, key: impl Into<String>, column: impl Into<String>) -> Self {
        self.tag_columns.insert(key.into(), column.into());
        self
    }
    pub fn channel_name_override(mut self, old: impl Into<String>, new: impl Into<String>) -> Self {
        self.channel_name_overrides.insert(old.into(), new.into());
        self
    }
}
#[derive(Debug, Clone, Default)]
pub struct BatchAvroStream {
    pub(crate) timestamp: BatchNumericTimestamp,
    pub(crate) tags: BTreeMap<String, String>,
    pub(crate) units: BTreeMap<String, String>,
    pub(crate) channel_prefix: Option<String>,
}
impl BatchAvroStream {
    pub fn numeric_timestamp(mut self, timestamp: BatchNumericTimestamp) -> Self {
        self.timestamp = timestamp;
        self
    }
}
#[derive(Debug, Clone, Default)]
pub struct BatchMcap {
    pub(crate) topics: Topics,
    pub(crate) ignore_invalid_topics: bool,
    pub(crate) tags: BTreeMap<String, String>,
}
impl BatchMcap {
    pub fn topics(mut self, topics: Topics) -> Self {
        self.topics = topics;
        self
    }
    pub fn ignore_invalid_topics(mut self, value: bool) -> Self {
        self.ignore_invalid_topics = value;
        self
    }
}
#[derive(Debug, Clone, Default)]
pub struct BatchJournalJson {
    pub(crate) timestamp: Option<Timestamp>,
    pub(crate) channel: Option<String>,
    pub(crate) tags: BTreeMap<String, String>,
}
impl BatchJournalJson {
    pub fn timestamp(mut self, timestamp: Timestamp) -> Self {
        self.timestamp = Some(timestamp);
        self
    }
    pub fn channel(mut self, channel: impl Into<String>) -> Self {
        self.channel = Some(channel.into());
        self
    }
}
#[derive(Debug, Clone, Default)]
pub struct BatchDataflash {
    pub(crate) tags: BTreeMap<String, String>,
}
macro_rules! tag_builder { ($($t:ty),+) => { $(impl $t { pub fn tag(mut self, key: impl Into<String>, value: impl Into<String>) -> Self { self.tags.insert(key.into(),value.into()); self } })+ }; }
tag_builder!(
    BatchTabular,
    BatchAvroStream,
    BatchMcap,
    BatchJournalJson,
    BatchDataflash
);
macro_rules! channel_builders { ($($t:ty),+) => { $(impl $t {
    pub fn unit(mut self, channel: impl Into<String>, unit: impl Into<String>) -> Self { self.units.insert(channel.into(), unit.into()); self }
    pub fn channel_prefix(mut self, prefix: impl Into<String>) -> Self { self.channel_prefix = Some(prefix.into()); self }
})+ }; }
channel_builders!(BatchTabular, BatchAvroStream);

#[derive(Debug)]
pub(super) struct PendingUpload {
    pub id: usize,
    pub name: String,
    pub path: PathBuf,
    pub mime: &'static str,
}
#[derive(Debug)]
pub(super) enum PendingItem {
    Tabular {
        file: PendingUpload,
        options: BatchTabular,
        format: TabularFormat,
    },
    Avro {
        file: PendingUpload,
        options: BatchAvroStream,
    },
    Mcap {
        file: PendingUpload,
        options: BatchMcap,
    },
    Journal {
        file: PendingUpload,
        options: BatchJournalJson,
    },
    Dataflash {
        file: PendingUpload,
        options: BatchDataflash,
    },
    Containerized {
        sources: Vec<PendingUpload>,
        options: ContainerizedIngest,
    },
    Video {
        file: PendingUpload,
        sidecar: Option<PendingUpload>,
        start: Option<DateTime<Utc>>,
        channel: String,
        tags: BTreeMap<String, String>,
    },
}
#[derive(Debug, Clone, Copy)]
pub(super) enum TabularFormat {
    Csv,
    Parquet { archive: bool },
}
impl PendingItem {
    pub fn uploads(&self) -> Vec<&PendingUpload> {
        match self {
            Self::Tabular { file, .. }
            | Self::Avro { file, .. }
            | Self::Mcap { file, .. }
            | Self::Journal { file, .. }
            | Self::Dataflash { file, .. } => vec![file],
            Self::Containerized { sources, .. } => sources.iter().collect(),
            Self::Video { file, sidecar, .. } => {
                std::iter::once(file).chain(sidecar.iter()).collect()
            }
        }
    }
}
