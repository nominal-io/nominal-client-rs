use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NumericTimeUnit {
    Seconds,
    Milliseconds,
    Microseconds,
    Nanoseconds,
}
#[derive(Clone, Debug)]
pub enum NumericTimestamp {
    Epoch(NumericTimeUnit),
    Relative {
        unit: NumericTimeUnit,
        start: DateTime<Utc>,
    },
}
/// Full injected catalog metadata, kept separate from numeric output overrides.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobTimestampMetadata {
    pub series_name: String,
    pub timestamp_type: JobTimestampType,
}
#[derive(Clone, Debug)]
pub enum JobTimestampType {
    Absolute(AbsoluteTimestampType),
    Relative(RelativeTimestamp),
    Unknown {
        kind: String,
        value: serde_json::Value,
    },
}
#[derive(Clone, Debug)]
pub enum AbsoluteTimestampType {
    EpochOfTimeUnit(EpochTimeUnit),
    Iso8601,
    CustomFormat(CustomTimestampFormat),
    Unknown {
        kind: String,
        value: serde_json::Value,
    },
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpochTimeUnit {
    pub time_unit: String,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelativeTimestamp {
    pub time_unit: String,
    pub offset: DateTime<Utc>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomTimestampFormat {
    pub format: String,
    pub default_year: Option<i32>,
    pub default_day_of_year: Option<u32>,
}
impl<'de> Deserialize<'de> for JobTimestampType {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        use serde::de::Error;
        let value = serde_json::Value::deserialize(deserializer)?;
        let kind = value
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| D::Error::custom("timestamp type is missing"))?;
        match kind {
            "absolute" => {
                serde_json::from_value(value.get("absolute").cloned().unwrap_or_default())
                    .map(Self::Absolute)
                    .map_err(D::Error::custom)
            }
            "relative" => {
                serde_json::from_value(value.get("relative").cloned().unwrap_or_default())
                    .map(Self::Relative)
                    .map_err(D::Error::custom)
            }
            _ => Ok(Self::Unknown {
                kind: kind.into(),
                value,
            }),
        }
    }
}
impl<'de> Deserialize<'de> for AbsoluteTimestampType {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        use serde::de::Error;
        let value = serde_json::Value::deserialize(deserializer)?;
        let kind = value
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| D::Error::custom("absolute timestamp type is missing"))?;
        match kind {
            "epochOfTimeUnit" => {
                serde_json::from_value(value.get(kind).cloned().unwrap_or_default())
                    .map(Self::EpochOfTimeUnit)
                    .map_err(D::Error::custom)
            }
            "customFormat" => serde_json::from_value(value.get(kind).cloned().unwrap_or_default())
                .map(Self::CustomFormat)
                .map_err(D::Error::custom),
            "iso8601" if value.get(kind).is_some_and(|v| v.is_object()) => Ok(Self::Iso8601),
            "iso8601" => Err(D::Error::custom("iso8601 metadata must be an object")),
            _ => Ok(Self::Unknown {
                kind: kind.into(),
                value,
            }),
        }
    }
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TimestampMetadata {
    series_name: String,
    epoch_time_unit: NumericTimeUnit,
    relative_offset: Option<String>,
}
impl TimestampMetadata {
    pub fn new(series_name: String, t: NumericTimestamp) -> Self {
        let (unit, offset) = match t {
            NumericTimestamp::Epoch(u) => (u, None),
            NumericTimestamp::Relative { unit, start } => (
                unit,
                Some(start.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)),
            ),
        };
        Self {
            series_name,
            epoch_time_unit: unit,
            relative_offset: offset,
        }
    }
}
