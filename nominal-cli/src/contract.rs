use anyhow::{Context, ensure};
use nominal::core::{TimeUnit, Timestamp};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub fn read<T: serde::de::DeserializeOwned>(path: &Path) -> anyhow::Result<T> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("invalid JSON in {}", path.display()))
}
pub fn version(version: u32) -> anyhow::Result<()> {
    ensure!(version == 1, "schema_version must be 1, got {version}");
    Ok(())
}
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    Nanoseconds,
    Microseconds,
    Milliseconds,
    Seconds,
    Minutes,
    Hours,
    Days,
}
impl From<Unit> for TimeUnit {
    fn from(v: Unit) -> Self {
        match v {
            Unit::Nanoseconds => Self::Nanoseconds,
            Unit::Microseconds => Self::Microseconds,
            Unit::Milliseconds => Self::Milliseconds,
            Unit::Seconds => Self::Seconds,
            Unit::Minutes => Self::Minutes,
            Unit::Hours => Self::Hours,
            Unit::Days => Self::Days,
        }
    }
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TimestampInput {
    Epoch {
        column: String,
        unit: Unit,
    },
    Relative {
        column: String,
        unit: Unit,
        start: String,
    },
    Iso8601 {
        column: String,
    },
    Custom {
        column: String,
        format: String,
        default_year: Option<i32>,
        default_day_of_year: Option<i32>,
    },
}
impl TryFrom<TimestampInput> for Timestamp {
    type Error = anyhow::Error;
    fn try_from(v: TimestampInput) -> anyhow::Result<Self> {
        Ok(match v {
            TimestampInput::Epoch { column, unit } => Timestamp::epoch(column, unit.into()),
            TimestampInput::Relative {
                column,
                unit,
                start,
            } => Timestamp::relative(column, unit.into())
                .with_offset(start.parse().context("timestamp.start must be RFC3339")?),
            TimestampInput::Iso8601 { column } => Timestamp::iso8601(column),
            TimestampInput::Custom {
                column,
                format,
                default_year,
                default_day_of_year,
            } => {
                let mut t = Timestamp::custom(column, format);
                if let Some(v) = default_year {
                    t = t.with_default_year(v)
                }
                if let Some(v) = default_day_of_year {
                    t = t.with_default_day_of_year(v)
                }
                t
            }
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extractor_timestamp_contract() {
        for text in [
            r#"{"kind":"relative","column":"ts","unit":"seconds","start":"2026-09-08T00:00:00Z"}"#,
            r#"{"kind":"custom","column":"ts","format":"yyyy","default_year":2026}"#,
        ] {
            let dto: TimestampInput = serde_json::from_str(text).unwrap();
            assert!(Timestamp::try_from(dto).is_ok());
        }
        assert!(
            serde_json::from_str::<TimestampInput>(
                r#"{"kind":"epoch","column":"ts","unit":"seconds","extra":1}"#
            )
            .is_err()
        );
    }
}
