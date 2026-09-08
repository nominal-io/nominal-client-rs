use crate::timestamp::TimestampMetadata;
use serde::Serialize;
use std::collections::BTreeMap;
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Manifest {
    pub outputs: Vec<Output>,
    pub video_outputs: Vec<Video>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Output {
    pub ingest_type: &'static str,
    pub relative_path: String,
    pub tag_columns: BTreeMap<String, String>,
    pub channel_prefix: Option<String>,
    pub timestamp_metadata: Option<TimestampMetadata>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Video {
    pub relative_path: String,
    pub channel: String,
    pub timestamp_manifest: VideoTimestamp,
}
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub(super) enum VideoTimestamp {
    NoManifest {
        #[serde(rename = "noManifest")]
        no_manifest: NoManifest,
    },
    FrameTimestampsRelativePath {
        #[serde(rename = "frameTimestampsRelativePath")]
        frame_timestamps_relative_path: String,
    },
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct NoManifest {
    pub starting_timestamp: SecondsNanos,
    pub scale_parameter: Option<Scale>,
}
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub(super) enum Scale {
    EndingTimestamp {
        #[serde(rename = "endingTimestamp")]
        ending_timestamp: SecondsNanos,
    },
    TrueFrameRate {
        #[serde(rename = "trueFrameRate")]
        true_frame_rate: f64,
    },
    #[serde(rename = "scaleFactor")]
    Factor {
        #[serde(rename = "scaleFactor")]
        scale_factor: f64,
    },
}
#[derive(Serialize)]
pub(super) struct SecondsNanos {
    pub seconds: i64,
    pub nanos: u32,
}
impl From<chrono::DateTime<chrono::Utc>> for SecondsNanos {
    fn from(t: chrono::DateTime<chrono::Utc>) -> Self {
        Self {
            seconds: t.timestamp(),
            nanos: t.timestamp_subsec_nanos(),
        }
    }
}
