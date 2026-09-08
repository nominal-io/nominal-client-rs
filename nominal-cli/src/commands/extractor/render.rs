use nominal::core::{
    ContainerImage, ContainerImageStatus, ContainerizedExtractor, FileOutputFormat, TimeUnit,
    Timestamp, TimestampKind,
};
use serde::Serialize;
#[derive(Serialize)]
pub struct ExtractorView<'a> {
    rid: &'a str,
    workspace_rid: &'a str,
    name: &'a str,
    description: Option<&'a str>,
    archived: bool,
    created_at: Option<String>,
    active_image: Option<ImageView<'a>>,
}
impl<'a> From<&'a ContainerizedExtractor> for ExtractorView<'a> {
    fn from(v: &'a ContainerizedExtractor) -> Self {
        Self {
            rid: v.rid(),
            workspace_rid: v.workspace_rid(),
            name: v.name(),
            description: v.description(),
            archived: v.is_archived(),
            created_at: v.created_at().map(|t| t.to_rfc3339()),
            active_image: v.active_container_image().map(Into::into),
        }
    }
}
#[derive(Serialize)]
pub struct ImageView<'a> {
    rid: &'a str,
    workspace_rid: &'a str,
    extractor_rid: &'a str,
    tag: &'a str,
    size_bytes: Option<i64>,
    created_at: Option<String>,
    status: String,
    output_format: String,
    default_timestamp: Option<TimestampView>,
    inputs: Vec<InputView<'a>>,
    parameters: Vec<ParameterView<'a>>,
}
#[derive(Serialize)]
struct InputView<'a> {
    name: &'a str,
    environment_variable: &'a str,
    description: Option<&'a str>,
    file_suffixes: &'a [String],
    required: bool,
}
#[derive(Serialize)]
struct ParameterView<'a> {
    name: &'a str,
    environment_variable: &'a str,
    description: Option<&'a str>,
    required: bool,
}
impl<'a> From<&'a ContainerImage> for ImageView<'a> {
    fn from(v: &'a ContainerImage) -> Self {
        Self {
            rid: v.rid(),
            workspace_rid: v.workspace_rid(),
            extractor_rid: v.extractor_rid(),
            tag: v.tag(),
            size_bytes: v.size_bytes(),
            created_at: v.created_at().map(|t| t.to_rfc3339()),
            status: match v.status() {
                ContainerImageStatus::Pending => "pending".into(),
                ContainerImageStatus::Ready => "ready".into(),
                ContainerImageStatus::Failed => "failed".into(),
                ContainerImageStatus::Unknown(n) => format!("unknown({n})"),
            },
            output_format: match v.output_format() {
                FileOutputFormat::Parquet => "parquet".into(),
                FileOutputFormat::Csv => "csv".into(),
                FileOutputFormat::AvroStream => "avro_stream".into(),
                FileOutputFormat::Manifest => "manifest".into(),
                FileOutputFormat::ParquetTar => "parquet_tar".into(),
                FileOutputFormat::JsonL => "json_l".into(),
                FileOutputFormat::Unknown(n) => format!("unknown({n})"),
            },
            default_timestamp: v.default_timestamp().map(Into::into),
            inputs: v
                .inputs()
                .iter()
                .map(|i| InputView {
                    name: i.name(),
                    environment_variable: i.environment_variable(),
                    description: i.description_text(),
                    file_suffixes: i.suffixes(),
                    required: i.is_required(),
                })
                .collect(),
            parameters: v
                .parameters()
                .iter()
                .map(|p| ParameterView {
                    name: p.name(),
                    environment_variable: p.environment_variable(),
                    description: p.description_text(),
                    required: p.is_required(),
                })
                .collect(),
        }
    }
}
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
pub fn emit<T: Serialize>(value: &T, json: bool) -> anyhow::Result<()> {
    if json {
        println!("{}", serde_json::to_string(value)?)
    } else {
        print_human(&serde_json::to_value(value)?, 0)
    }
    Ok(())
}
#[derive(Serialize)]
pub struct Deleted<'a> {
    pub rid: &'a str,
    pub deleted: bool,
}

fn print_human(value: &serde_json::Value, indent: usize) {
    match value {
        serde_json::Value::Object(fields) => {
            for (key, value) in fields {
                match value {
                    serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
                        println!("{}{}:", " ".repeat(indent), key);
                        print_human(value, indent + 2);
                    }
                    serde_json::Value::String(text) => {
                        println!("{}{}: {}", " ".repeat(indent), key, text)
                    }
                    value => println!("{}{}: {}", " ".repeat(indent), key, value),
                }
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                print_human(value, indent);
                if value.is_object() {
                    println!();
                }
            }
        }
        serde_json::Value::String(text) => println!("{}{}", " ".repeat(indent), text),
        value => println!("{}{}", " ".repeat(indent), value),
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
