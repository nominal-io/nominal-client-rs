use super::render;
use crate::args::{OutputArgs, WaitArgs};
use crate::contract;
use anyhow::{Context, ensure};
use clap::{ArgGroup, Args};
use nominal::core::*;
use std::{collections::BTreeMap, path::PathBuf};
#[derive(Args)]
#[command(group(ArgGroup::new("container_target").required(true).args(["dataset","name"])))]
pub struct ContainerizedArgs {
    extractor_rid: String,
    #[arg(long)]
    dataset: Option<String>,
    #[arg(long)]
    name: Option<String>,
    #[arg(long, requires = "name")]
    description: Option<String>,
    #[arg(long = "label", requires = "name")]
    labels: Vec<String>,
    #[arg(long="property",num_args=2,value_names=["KEY","VALUE"],requires="name")]
    properties: Vec<String>,
    #[arg(long="source",num_args=2,value_names=["ENV","PATH"])]
    sources: Vec<String>,
    /// Repeated keys use the last value.
    #[arg(long="argument",num_args=2,value_names=["ENV","VALUE"])]
    arguments: Vec<String>,
    /// Repeated keys use the last value.
    #[arg(long="file-tag",num_args=2,value_names=["KEY","VALUE"])]
    tags: Vec<String>,
    #[arg(long, requires = "timestamp_type", conflicts_with = "timestamp_json")]
    timestamp_column: Option<String>,
    #[arg(long, requires = "timestamp_column", conflicts_with = "timestamp_json")]
    timestamp_type: Option<String>,
    #[arg(long,requires_all=["timestamp_column","timestamp_type"],conflicts_with="timestamp_json")]
    relative_to: Option<chrono::DateTime<chrono::Utc>>,
    #[arg(long,conflicts_with_all=["timestamp_column","timestamp_type","relative_to"])]
    timestamp_json: Option<PathBuf>,
    #[command(flatten)]
    wait: WaitArgs,
    #[command(flatten)]
    output: OutputArgs,
}
pub fn pairs(values: Vec<String>, unique: bool) -> anyhow::Result<BTreeMap<String, String>> {
    ensure!(values.len() % 2 == 0, "expected KEY VALUE pairs");
    let mut out = BTreeMap::new();
    let mut it = values.into_iter();
    while let Some(k) = it.next() {
        let v = it.next().unwrap();
        ensure!(
            !unique || !out.contains_key(&k),
            "duplicate source environment key: {k}"
        );
        out.insert(k, v);
    }
    Ok(out)
}
impl ContainerizedArgs {
    fn timestamp(&self) -> anyhow::Result<Option<Timestamp>> {
        if let Some(path) = &self.timestamp_json {
            return Ok(Some(
                Timestamp::try_from(contract::read::<contract::TimestampInput>(path)?)
                    .with_context(|| format!("timestamp in {}", path.display()))?,
            ));
        }
        match (&self.timestamp_column, &self.timestamp_type) {
            (Some(column), Some(spec)) => {
                if spec.trim().eq_ignore_ascii_case("iso8601") {
                    ensure!(
                        self.relative_to.is_none(),
                        "--relative-to requires a numeric timestamp type"
                    );
                    return Ok(Some(Timestamp::iso8601(column)));
                }
                let unit = crate::timestamp::parse_time_unit(spec).map_err(anyhow::Error::msg)?;
                Ok(Some(match self.relative_to {
                    Some(start) => Timestamp::relative(column, unit).with_offset(start),
                    None => Timestamp::epoch(column, unit),
                }))
            }
            (None, None) => Ok(None),
            _ => anyhow::bail!("timestamp column and type are required together"),
        }
    }
}
fn prepare(
    a: ContainerizedArgs,
) -> anyhow::Result<(DatasetTarget, ContainerizedIngest, WaitArgs, bool)> {
    let timestamp = a.timestamp()?;
    let sources = pairs(a.sources, true)?;
    let target = match a.dataset {
        Some(rid) => DatasetTarget::Existing(rid),
        None => {
            let mut create = DatasetCreate::new(a.name.context("name required")?)
                .labels(a.labels)
                .properties(pairs(a.properties, false)?);
            if let Some(d) = a.description {
                create = create.description(d)
            }
            DatasetTarget::New(create)
        }
    };
    let mut options = ContainerizedIngest::new(a.extractor_rid);
    for (k, v) in sources {
        options = options.source(k, v)
    }
    for (k, v) in pairs(a.arguments, false)? {
        options = options.argument(k, v)
    }
    for (k, v) in pairs(a.tags, false)? {
        options = options.tag(k, v)
    }
    if let Some(t) = timestamp {
        options = options.timestamp(t)
    }
    Ok((target, options, a.wait, a.output.json))
}

pub async fn handle(a: ContainerizedArgs, profile: Option<&str>) -> anyhow::Result<()> {
    let (target, options, wait, json) = prepare(a)?;
    let ingest = crate::commands::load_client(profile)?.ingest();
    let submission = ingest.upload_containerized(target, options).await?;
    render::submission(
        &ingest,
        submission.job().rid(),
        submission.dataset_rid(),
        wait,
        json,
    )
    .await
}
#[cfg(test)]
mod conversion_tests {
    use super::*;
    use clap::Parser;
    #[derive(Parser)]
    struct Command {
        #[command(flatten)]
        args: ContainerizedArgs,
    }
    #[test]
    fn extractor_direct_conversion_preserves_full_input() {
        let a = Command::try_parse_from([
            "test",
            "extractor",
            "--dataset",
            "dataset",
            "--source",
            "INPUT",
            "a b=c.flight",
            "--argument",
            "ARG",
            "a b=c",
            "--argument",
            "ARG",
            "last value",
            "--file-tag",
            "tag",
            "first",
            "--file-tag",
            "tag",
            "value",
            "--timestamp-column",
            "clock",
            "--timestamp-type",
            "NANOSECONDS",
            "--relative-to",
            "2026-09-08T00:00:00.123456789Z",
            "--no-wait",
        ])
        .unwrap()
        .args;
        let (_, options, wait, _) = prepare(a).unwrap();
        assert_eq!(options.extractor_rid(), "extractor");
        assert_eq!(options.sources()["INPUT"], PathBuf::from("a b=c.flight"));
        assert_eq!(options.arguments()["ARG"], "last value");
        assert_eq!(options.tags()["tag"], "value");
        assert!(wait.no_wait);
        assert!(matches!(
            options.timestamp_metadata().unwrap().encoding(),
            TimestampKind::Relative {
                unit: TimeUnit::Nanoseconds,
                offset: Some(_)
            }
        ));
    }
}
