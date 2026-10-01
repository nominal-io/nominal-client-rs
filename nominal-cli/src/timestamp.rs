use nominal::core::{TimeUnit, Timestamp, TimestampKind};
use serde::Serialize;

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TimestampView {
    Epoch {
        column: String,
        unit: &'static str,
    },
    Relative {
        column: String,
        unit: &'static str,
        start: Option<String>,
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
impl From<&Timestamp> for TimestampView {
    fn from(v: &Timestamp) -> Self {
        let column = v.series_name().to_owned();
        match v.encoding() {
            TimestampKind::Epoch(u) => Self::Epoch {
                column,
                unit: unit(*u),
            },
            TimestampKind::Relative { unit: u, offset } => Self::Relative {
                column,
                unit: unit(*u),
                start: offset.map(|t| t.to_rfc3339()),
            },
            TimestampKind::Iso8601 => Self::Iso8601 { column },
            TimestampKind::Custom {
                format,
                default_year,
                default_day_of_year,
            } => Self::Custom {
                column,
                format: format.clone(),
                default_year: *default_year,
                default_day_of_year: *default_day_of_year,
            },
        }
    }
}
fn unit(v: TimeUnit) -> &'static str {
    match v {
        TimeUnit::Nanoseconds => "nanoseconds",
        TimeUnit::Microseconds => "microseconds",
        TimeUnit::Milliseconds => "milliseconds",
        TimeUnit::Seconds => "seconds",
        TimeUnit::Minutes => "minutes",
        TimeUnit::Hours => "hours",
        TimeUnit::Days => "days",
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extractor_timestamp_json_is_lossless() {
        let t = Timestamp::custom("ts", "yyyy-DDD")
            .with_default_year(2026)
            .with_default_day_of_year(5);
        let v = serde_json::to_value(TimestampView::from(&t)).unwrap();
        assert_eq!(v["default_year"], 2026);
        assert_eq!(v["default_day_of_year"], 5);
        let t = Timestamp::relative("ts", TimeUnit::Nanoseconds)
            .with_offset(chrono::DateTime::from_timestamp(1, 123456789).unwrap());
        let v = serde_json::to_value(TimestampView::from(&t)).unwrap();
        assert_eq!(v["start"], "1970-01-01T00:00:01.123456789+00:00");
    }
}
