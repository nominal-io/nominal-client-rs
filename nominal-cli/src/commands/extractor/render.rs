use crate::timestamp::TimestampView;
use nominal::core::{
    ContainerImage, ContainerImageStatus, ContainerizedExtractor, FileOutputFormat,
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
pub struct Deleted<'a> {
    pub rid: &'a str,
    pub deleted: bool,
}
