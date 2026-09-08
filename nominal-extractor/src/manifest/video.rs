use super::{ManifestContext, wire::*};
use crate::{Error, Result};
use chrono::{DateTime, Utc};
use std::{io::Write, path::PathBuf};
#[derive(Clone, Debug)]
pub enum VideoScale {
    EndingTimestamp(DateTime<Utc>),
    TrueFrameRate(f64),
    Factor(f64),
}
#[derive(Clone, Debug)]
pub enum VideoTiming {
    Start {
        at: DateTime<Utc>,
        scale: Option<VideoScale>,
    },
    FrameTimestamps(Vec<i64>),
}
pub struct VideoOutput {
    path: PathBuf,
    channel: String,
    timing: VideoTiming,
}
impl VideoOutput {
    pub fn new(path: impl Into<PathBuf>, channel: impl Into<String>, timing: VideoTiming) -> Self {
        Self {
            path: path.into(),
            channel: channel.into(),
            timing,
        }
    }
}
impl ManifestContext {
    pub fn add_video(&mut self, video: VideoOutput) -> Result<PathBuf> {
        if video.channel.is_empty() {
            return Err(Error::InvalidOutput("video channel is empty".into()));
        }
        let (path, relative) = self.output.resolve(&video.path, true)?;
        crate::paths::extension(&path, &[".avi", ".m2ts", ".mkv", ".mp4", ".ts"])?;
        let mut sidecar_relative = None;
        let timestamp_manifest = match video.timing {
            VideoTiming::Start { at, scale } => {
                let scale_parameter = scale
                    .map(|s| match s {
                        VideoScale::EndingTimestamp(t) => Ok(Scale::EndingTimestamp {
                            ending_timestamp: t.into(),
                        }),
                        VideoScale::TrueFrameRate(v) if v.is_finite() => {
                            Ok(Scale::TrueFrameRate { true_frame_rate: v })
                        }
                        VideoScale::Factor(v) if v.is_finite() => {
                            Ok(Scale::Factor { scale_factor: v })
                        }
                        _ => Err(Error::InvalidOutput(
                            "video scale must be finite for JSON".into(),
                        )),
                    })
                    .transpose()?;
                VideoTimestamp::NoManifest {
                    no_manifest: NoManifest {
                        starting_timestamp: at.into(),
                        scale_parameter,
                    },
                }
            }
            VideoTiming::FrameTimestamps(frames) => {
                if frames.is_empty() {
                    return Err(Error::InvalidOutput("frame timestamps are empty".into()));
                }
                let count = self
                    .manifest
                    .video_outputs
                    .iter()
                    .filter(|v| v.relative_path == relative)
                    .count();
                let suffix = if count == 0 {
                    ".timestamps.json".into()
                } else {
                    format!(".timestamps.{count}.json")
                };
                let rel = format!("{relative}{suffix}");
                let sidecar = self.output.root.join(&rel);
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&sidecar)
                    .map_err(|e| {
                        if e.kind() == std::io::ErrorKind::AlreadyExists {
                            Error::SidecarCollision(sidecar.clone())
                        } else {
                            Error::Io(e)
                        }
                    })?;
                let written = (|| -> Result<()> {
                    serde_json::to_writer(&mut file, &frames)?;
                    file.flush()?;
                    Ok(())
                })();
                if let Err(error) = written {
                    drop(file);
                    let _ = std::fs::remove_file(&sidecar);
                    return Err(error);
                }
                sidecar_relative = Some(rel.clone());
                VideoTimestamp::FrameTimestampsRelativePath {
                    frame_timestamps_relative_path: rel,
                }
            }
        };
        self.manifest.video_outputs.push(Video {
            relative_path: relative.clone(),
            channel: video.channel,
            timestamp_manifest,
        });
        self.output.account(relative);
        if let Some(sidecar) = sidecar_relative {
            self.output.account(sidecar);
        }
        Ok(path)
    }
}
