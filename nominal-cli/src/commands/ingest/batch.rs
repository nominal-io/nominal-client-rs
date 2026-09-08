use super::{
    contract::{self, TimestampInput, Unit},
    render,
};
use crate::commands::extractor::args::{OutputArgs, WaitArgs};
use anyhow::Context;
use clap::Args;
use nominal::core::*;
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    num::NonZeroUsize,
    path::{Path, PathBuf},
};
#[derive(Args)]
pub struct BatchArgs {
    file: PathBuf,
    #[arg(long)]
    allow_partial: bool,
    #[arg(long)]
    max_uploads: Option<NonZeroUsize>,
    #[command(flatten)]
    wait: WaitArgs,
    #[command(flatten)]
    output: OutputArgs,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchRequest {
    schema_version: u32,
    dataset: String,
    #[serde(default)]
    tags: BTreeMap<String, String>,
    #[serde(default)]
    runs_to_expand: Vec<String>,
    items: Vec<Item>,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Item {
    Containerized {
        extractor: String,
        sources: BTreeMap<String, PathBuf>,
        #[serde(default)]
        arguments: BTreeMap<String, String>,
        timestamp: Option<TimestampInput>,
        #[serde(default)]
        tags: BTreeMap<String, String>,
    },
    Tabular {
        path: PathBuf,
        timestamp: TimestampInput,
        #[serde(default)]
        tag_columns: BTreeMap<String, String>,
        #[serde(default)]
        units: BTreeMap<String, String>,
        #[serde(default)]
        channel_name_overrides: BTreeMap<String, String>,
        channel_prefix: Option<String>,
        #[serde(default)]
        tags: BTreeMap<String, String>,
    },
    AvroStream {
        path: PathBuf,
        timestamp: Option<NumericTimestamp>,
        #[serde(default)]
        units: BTreeMap<String, String>,
        channel_prefix: Option<String>,
        #[serde(default)]
        tags: BTreeMap<String, String>,
    },
    Mcap {
        path: PathBuf,
        topics: Option<TopicInput>,
        #[serde(default)]
        ignore_invalid_topics: bool,
        #[serde(default)]
        tags: BTreeMap<String, String>,
    },
    JournalJson {
        path: PathBuf,
        channel: Option<String>,
        timestamp: Option<TimestampInput>,
        #[serde(default)]
        tags: BTreeMap<String, String>,
    },
    Dataflash {
        path: PathBuf,
        #[serde(default)]
        tags: BTreeMap<String, String>,
    },
    Video {
        path: PathBuf,
        channel: String,
        timing: TimingInput,
        #[serde(default)]
        tags: BTreeMap<String, String>,
    },
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum NumericTimestamp {
    Epoch { unit: Unit },
    Relative { unit: Unit, start: String },
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum TopicInput {
    Include { names: Vec<String> },
    Exclude { names: Vec<String> },
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum TimingInput {
    Start { at: String },
    Frames { timestamps_file: PathBuf },
}
impl BatchRequest {
    fn build(self, ingest: &IngestClient, parent: &Path) -> anyhow::Result<IngestBatch> {
        contract::version(self.schema_version)?;
        let mut batch = ingest.batch(self.dataset).add_tags(self.tags);
        for (index, item) in self.items.into_iter().enumerate() {
            batch = add_item(batch, item, parent).with_context(|| format!("items[{index}]"))?;
        }
        Ok(batch)
    }
}
fn add_item(batch: IngestBatch, item: Item, parent: &Path) -> anyhow::Result<IngestBatch> {
    Ok(match item {
        Item::Containerized {
            extractor,
            sources,
            arguments,
            timestamp,
            tags,
        } => {
            let mut o = ContainerizedIngest::new(extractor);
            for (k, v) in sources {
                o = o.source(k, parent.join(v))
            }
            for (k, v) in arguments {
                o = o.argument(k, v)
            }
            for (k, v) in tags {
                o = o.tag(k, v)
            }
            if let Some(t) = timestamp {
                o = o.timestamp(t.try_into()?)
            }
            batch.add_containerized(o)?
        }
        Item::Tabular {
            path,
            timestamp,
            tag_columns,
            units,
            channel_name_overrides,
            channel_prefix,
            tags,
        } => {
            let mut o = BatchTabular::new(timestamp.try_into()?);
            for (k, v) in tag_columns {
                o = o.tag_column(k, v)
            }
            for (k, v) in units {
                o = o.unit(k, v)
            }
            for (k, v) in channel_name_overrides {
                o = o.channel_name_override(k, v)
            }
            for (k, v) in tags {
                o = o.tag(k, v)
            }
            if let Some(v) = channel_prefix {
                o = o.channel_prefix(v)
            }
            batch.add_tabular(parent.join(path), o)?
        }
        Item::AvroStream {
            path,
            timestamp,
            units,
            channel_prefix,
            tags,
        } => {
            let mut o = BatchAvroStream::default();
            if let Some(t) = timestamp {
                o = o.numeric_timestamp(match t {
                    NumericTimestamp::Epoch { unit } => BatchNumericTimestamp::Epoch(unit.into()),
                    NumericTimestamp::Relative { unit, start } => BatchNumericTimestamp::Relative {
                        unit: unit.into(),
                        start: start.parse().context("timestamp.start must be RFC3339")?,
                    },
                })
            }
            for (k, v) in units {
                o = o.unit(k, v)
            }
            for (k, v) in tags {
                o = o.tag(k, v)
            }
            if let Some(v) = channel_prefix {
                o = o.channel_prefix(v)
            }
            batch.add_avro_stream(parent.join(path), o)?
        }
        Item::Mcap {
            path,
            topics,
            ignore_invalid_topics,
            tags,
        } => {
            let mut o = BatchMcap::default().ignore_invalid_topics(ignore_invalid_topics);
            if let Some(t) = topics {
                o = o.topics(match t {
                    TopicInput::Include { names } => Topics::Include(names),
                    TopicInput::Exclude { names } => Topics::Exclude(names),
                })
            }
            for (k, v) in tags {
                o = o.tag(k, v)
            }
            batch.add_mcap(parent.join(path), o)?
        }
        Item::JournalJson {
            path,
            channel,
            timestamp,
            tags,
        } => {
            let mut o = BatchJournalJson::default();
            if let Some(v) = channel {
                o = o.channel(v)
            }
            if let Some(v) = timestamp {
                o = o.timestamp(v.try_into()?)
            }
            for (k, v) in tags {
                o = o.tag(k, v)
            }
            batch.add_journal_json(parent.join(path), o)?
        }
        Item::Dataflash { path, tags } => {
            let mut o = BatchDataflash::default();
            for (k, v) in tags {
                o = o.tag(k, v)
            }
            batch.add_dataflash(parent.join(path), o)?
        }
        Item::Video {
            path,
            channel,
            timing,
            tags,
        } => {
            let timing = match timing {
                TimingInput::Start { at } => {
                    BatchVideoTiming::Start(at.parse().context("timing.at must be RFC3339")?)
                }
                TimingInput::Frames { timestamps_file } => BatchVideoTiming::FrameTimestamps(
                    contract::read(&parent.join(timestamps_file))?,
                ),
            };
            batch.add_video_with_tags(parent.join(path), channel, timing, tags)?
        }
    })
}
pub async fn handle(a: BatchArgs, client: NominalClient) -> anyhow::Result<()> {
    let request: BatchRequest = contract::read(&a.file)?;
    let dataset = request.dataset.clone();
    let mut options = BatchOptions::default().failure_policy(if a.allow_partial {
        FailurePolicy::AllowPartial
    } else {
        FailurePolicy::FailFast
    });
    if let Some(n) = a.max_uploads {
        options = options.max_uploads(n)
    }
    for rid in &request.runs_to_expand {
        options = options.run_to_expand(rid)
    }
    let ingest = client.ingest();
    let batch = request
        .build(&ingest, a.file.parent().unwrap_or(Path::new(".")))
        .with_context(|| format!("batch request {}", a.file.display()))?;
    let result = match batch.submit(options).await {
        Ok(result) => result,
        Err(nominal::Error::BatchUpload(error)) => {
            let failures: Vec<render::Omitted> =
                error.failures.into_iter().map(Into::into).collect();
            anyhow::bail!(
                "batch upload failed; no ingest request submitted: {}",
                serde_json::to_string(&failures)?
            );
        }
        Err(error) => return Err(error.into()),
    };
    render::submission(
        &ingest,
        result.job.rid(),
        &dataset,
        result.omitted.into_iter().map(Into::into).collect(),
        a.wait,
        a.output.json,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    const DATASET: &str = "ri.scout.main.dataset.00000000-0000-0000-0000-000000000001";
    fn request(item: &str) -> BatchRequest {
        serde_json::from_str(&format!(
            r#"{{"schema_version":1,"dataset":"{DATASET}","items":[{item}]}}"#
        ))
        .unwrap()
    }
    #[tokio::test]
    async fn extractor_batch_all_kinds_convert_without_network() {
        let client = NominalClient::builder("test-token")
            .base_url("http://localhost:1/api")
            .build()
            .unwrap();
        let ingest = client.ingest();
        for item in [
            r#"{"kind":"containerized","extractor":"ri.ingest.main.containerized-extractor.00000000-0000-0000-0000-000000000002","sources":{"INPUT":"a.flight"},"arguments":{"A":"a b=c"},"tags":{"K":"V"}}"#,
            r#"{"kind":"tabular","path":"a.csv","timestamp":{"kind":"custom","column":"ts","format":"yyyy","default_year":2026},"tag_columns":{"tag":"col"},"units":{"a":"m"},"channel_name_overrides":{"a":"b"},"channel_prefix":"prefix/"}"#,
            r#"{"kind":"avro_stream","path":"a.avro","timestamp":{"kind":"relative","unit":"milliseconds","start":"2026-09-08T00:00:00Z"},"units":{"a":"m"},"channel_prefix":"prefix/"}"#,
            r#"{"kind":"mcap","path":"a.mcap","topics":{"kind":"exclude","names":["topic"]},"ignore_invalid_topics":true}"#,
            r#"{"kind":"journal_json","path":"a.jsonl","channel":"logs","timestamp":{"kind":"epoch","column":"ts","unit":"microseconds"}}"#,
            r#"{"kind":"dataflash","path":"a.bin","tags":{"K":"V"}}"#,
            r#"{"kind":"video","path":"a.mp4","channel":"video","timing":{"kind":"start","at":"2026-09-08T00:00:00Z"},"tags":{"K":"V"}}"#,
        ] {
            assert!(
                request(item)
                    .build(&ingest, Path::new("/tmp/contracts"))
                    .is_ok(),
                "{item}"
            );
        }
    }
    #[tokio::test]
    async fn extractor_batch_rejects_unknown_numeric_column_and_version() {
        assert!(serde_json::from_str::<Item>(r#"{"kind":"avro_stream","path":"a.avro","timestamp":{"kind":"epoch","unit":"seconds","column":"forbidden"}}"#).is_err());
        assert!(
            serde_json::from_str::<Item>(r#"{"kind":"dataflash","path":"a.bin","mystery":true}"#)
                .is_err()
        );
        let client = NominalClient::builder("test-token")
            .base_url("http://localhost:1/api")
            .build()
            .unwrap();
        let ingest = client.ingest();
        let mut dto = request(r#"{"kind":"dataflash","path":"a.bin"}"#);
        dto.schema_version = 2;
        assert!(dto.build(&ingest, Path::new("/tmp")).is_err());
    }
    #[tokio::test]
    async fn extractor_batch_rejects_empty_sources_and_string_journal_timestamp() {
        let client = NominalClient::builder("test-token")
            .base_url("http://localhost:1/api")
            .build()
            .unwrap();
        let ingest = client.ingest();
        for item in [
            r#"{"kind":"containerized","extractor":"x","sources":{}}"#,
            r#"{"kind":"journal_json","path":"x.jsonl","timestamp":{"kind":"iso8601","column":"ts"}}"#,
        ] {
            assert!(request(item).build(&ingest, Path::new("/tmp")).is_err());
        }
    }
}

#[cfg(test)]
mod sidecar_tests {
    use super::*;
    #[tokio::test]
    async fn extractor_frame_sidecar_resolves_against_request_parent_and_preserves_i64() {
        let directory = std::env::temp_dir().join(format!(
            "nominal-cli-frames-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let file = directory.join("frames.json");
        std::fs::write(&file, "[9223372036854775806,9223372036854775807]").unwrap();
        let timestamps: Vec<i64> = contract::read(&file).unwrap();
        assert_eq!(timestamps[1], i64::MAX);
        let client = NominalClient::builder("token")
            .base_url("http://localhost:1/api")
            .build()
            .unwrap();
        let item:Item=serde_json::from_str(r#"{"kind":"video","path":"clip.mp4","channel":"camera","timing":{"kind":"frames","timestamps_file":"frames.json"}}"#).unwrap();
        let result = add_item(
            client
                .ingest()
                .batch("ri.scout.main.dataset.00000000-0000-0000-0000-000000000001"),
            item,
            &directory,
        );
        std::fs::remove_dir_all(&directory).unwrap();
        assert!(result.is_ok());
    }
}

impl BatchArgs {
    pub fn validate(&self) -> anyhow::Result<()> {
        let request: BatchRequest = contract::read(&self.file)?;
        contract::version(request.schema_version)
            .with_context(|| format!("batch request {}", self.file.display()))
    }
}
