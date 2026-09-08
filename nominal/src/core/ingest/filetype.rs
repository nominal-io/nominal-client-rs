use std::path::Path;

/// Recognized file formats for upload and ingest.
///
/// Wraps MIME type and extension metadata so callers never have to spell these
/// out in their own code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileType {
    Csv,
    CsvGz,
    Parquet,
    Mcap,
    JournalJsonl,
    JournalJsonlGz,
    AvroStream,
    Dataflash,
    Mp4,
    Mkv,
    Avi,
    Ts,
}

impl FileType {
    pub const fn extension(self) -> &'static str {
        match self {
            FileType::Csv => ".csv",
            FileType::CsvGz => ".csv.gz",
            FileType::Parquet => ".parquet",
            FileType::Mcap => ".mcap",
            FileType::JournalJsonl => ".jsonl",
            FileType::JournalJsonlGz => ".jsonl.gz",
            FileType::AvroStream => ".avro",
            FileType::Dataflash => ".bin",
            FileType::Mp4 => ".mp4",
            FileType::Mkv => ".mkv",
            FileType::Avi => ".avi",
            FileType::Ts => ".ts",
        }
    }

    pub const fn mime_type(self) -> &'static str {
        match self {
            FileType::Csv => "text/csv",
            FileType::CsvGz => "application/gzip",
            FileType::Parquet => "application/vnd.apache.parquet",
            FileType::Mcap => "application/octet-stream",
            FileType::JournalJsonl | FileType::JournalJsonlGz => "application/jsonl",
            FileType::AvroStream => "application/avro",
            FileType::Dataflash => "application/octet-stream",
            FileType::Mp4 => "video/mp4",
            FileType::Mkv => "video/x-matroska",
            FileType::Avi => "video/x-msvideo",
            FileType::Ts => "video/mp2t",
        }
    }

    /// True if this file type is a recognized standalone video container
    /// (i.e. not MCAP, which is also a possible video carrier).
    pub const fn is_video(self) -> bool {
        matches!(
            self,
            FileType::Mp4 | FileType::Mkv | FileType::Avi | FileType::Ts
        )
    }

    /// Infer a [`FileType`] from the file name portion of `path`. Matches are
    /// case-insensitive. Returns `None` if the extension is not recognized.
    pub fn from_path(path: impl AsRef<Path>) -> Option<Self> {
        let name = path.as_ref().file_name()?.to_str()?.to_ascii_lowercase();
        if name.ends_with(".csv.gz") {
            Some(FileType::CsvGz)
        } else if name.ends_with(".csv") {
            Some(FileType::Csv)
        } else if name.ends_with(".parquet") {
            Some(FileType::Parquet)
        } else if name.ends_with(".mcap") {
            Some(FileType::Mcap)
        } else if name.ends_with(".jsonl.gz") {
            Some(FileType::JournalJsonlGz)
        } else if name.ends_with(".jsonl") {
            Some(FileType::JournalJsonl)
        } else if name.ends_with(".avro") {
            Some(FileType::AvroStream)
        } else if name.ends_with(".bin") {
            Some(FileType::Dataflash)
        } else if name.ends_with(".mp4") {
            Some(FileType::Mp4)
        } else if name.ends_with(".mkv") {
            Some(FileType::Mkv)
        } else if name.ends_with(".avi") {
            Some(FileType::Avi)
        } else if name.ends_with(".ts") {
            Some(FileType::Ts)
        } else {
            None
        }
    }
}

/// Additional formats accepted by batch ingestion, without expanding native `FileType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IngestFileFormat {
    Native(FileType),
    ParquetGz,
    ParquetTar,
    ParquetTarGz,
    ParquetZip,
    AvroGz,
    M2ts,
    Json,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TabularFormat {
    Csv,
    Parquet { archive: bool },
}
impl IngestFileFormat {
    pub(crate) fn from_path(path: &Path) -> Option<Self> {
        if let Some(native) = FileType::from_path(path) {
            return Some(Self::Native(native));
        }
        let name = path.file_name()?.to_str()?.to_ascii_lowercase();
        [
            (".parquet.gz", Self::ParquetGz),
            (".parquet.tar.gz", Self::ParquetTarGz),
            (".parquet.tar", Self::ParquetTar),
            (".parquet.zip", Self::ParquetZip),
            (".avro.gz", Self::AvroGz),
            (".m2ts", Self::M2ts),
            (".json", Self::Json),
        ]
        .into_iter()
        .find_map(|(suffix, format)| name.ends_with(suffix).then_some(format))
    }
    pub(crate) fn tabular(self) -> Option<TabularFormat> {
        match self {
            Self::Native(FileType::Csv | FileType::CsvGz) => Some(TabularFormat::Csv),
            Self::Native(FileType::Parquet) | Self::ParquetGz => {
                Some(TabularFormat::Parquet { archive: false })
            }
            Self::ParquetTar | Self::ParquetTarGz | Self::ParquetZip => {
                Some(TabularFormat::Parquet { archive: true })
            }
            _ => None,
        }
    }
    pub(crate) fn is_avro(self) -> bool {
        matches!(self, Self::Native(FileType::AvroStream) | Self::AvroGz)
    }
    pub(crate) fn is_journal(self) -> bool {
        matches!(
            self,
            Self::Native(FileType::JournalJsonl | FileType::JournalJsonlGz)
        )
    }
    pub(crate) fn is_video(self) -> bool {
        match self {
            Self::Native(native) => native.is_video(),
            Self::M2ts => true,
            _ => false,
        }
    }
    /// Python batch uses the uncompressed CSV MIME for gzip CSV; native uploads retain application/gzip.
    pub(crate) fn batch_mime(self) -> &'static str {
        match self {
            Self::Native(FileType::CsvGz) => "text/csv",
            Self::Native(native) => native.mime_type(),
            Self::ParquetGz => "application/octet-stream",
            Self::ParquetTar | Self::ParquetTarGz => "application/x-tar",
            Self::ParquetZip => "application/zip",
            Self::AvroGz => "application/avro",
            Self::M2ts => "video/mp2t",
            Self::Json => "application/json",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_extended_formats_preserve_native_behavior_and_mime() {
        for (path, mime, tabular) in [
            ("a.csv.gz", "text/csv", Some(TabularFormat::Csv)),
            (
                "a.parquet.gz",
                "application/octet-stream",
                Some(TabularFormat::Parquet { archive: false }),
            ),
            (
                "a.parquet.tar",
                "application/x-tar",
                Some(TabularFormat::Parquet { archive: true }),
            ),
            (
                "a.parquet.tar.gz",
                "application/x-tar",
                Some(TabularFormat::Parquet { archive: true }),
            ),
            (
                "a.parquet.zip",
                "application/zip",
                Some(TabularFormat::Parquet { archive: true }),
            ),
            ("a.avro.gz", "application/avro", None),
            ("a.m2ts", "video/mp2t", None),
        ] {
            let format = IngestFileFormat::from_path(Path::new(path)).unwrap();
            assert_eq!(format.batch_mime(), mime, "{path}");
            assert_eq!(format.tabular(), tabular, "{path}");
            assert_eq!(
                IngestFileFormat::from_path(Path::new(&path.to_uppercase())),
                Some(format)
            );
            if path != "a.csv.gz" {
                assert_eq!(FileType::from_path(path), None);
            }
        }
        assert_eq!(FileType::CsvGz.mime_type(), "application/gzip");
        assert!(
            IngestFileFormat::from_path(Path::new("a.avro.gz"))
                .unwrap()
                .is_avro()
        );
        assert!(
            IngestFileFormat::from_path(Path::new("a.m2ts"))
                .unwrap()
                .is_video()
        );
        assert!(
            IngestFileFormat::from_path(Path::new("a.jsonl.gz"))
                .unwrap()
                .is_journal()
        );
    }

    #[test]
    fn from_path_matches_known_extensions() {
        assert_eq!(FileType::from_path("foo.csv"), Some(FileType::Csv));
        assert_eq!(FileType::from_path("FOO.CSV"), Some(FileType::Csv));
        assert_eq!(FileType::from_path("foo.csv.gz"), Some(FileType::CsvGz));
        assert_eq!(FileType::from_path("data.parquet"), Some(FileType::Parquet));
        assert_eq!(
            FileType::from_path("/tmp/nested/data.parquet"),
            Some(FileType::Parquet)
        );
    }

    #[test]
    fn from_path_returns_none_for_unknown() {
        assert_eq!(FileType::from_path("foo.txt"), None);
        assert_eq!(FileType::from_path("foo"), None);
    }

    #[test]
    fn from_path_matches_new_extensions() {
        assert_eq!(FileType::from_path("log.mcap"), Some(FileType::Mcap));
        assert_eq!(
            FileType::from_path("journal.jsonl"),
            Some(FileType::JournalJsonl)
        );
        assert_eq!(
            FileType::from_path("journal.jsonl.gz"),
            Some(FileType::JournalJsonlGz)
        );
        assert_eq!(
            FileType::from_path("stream.avro"),
            Some(FileType::AvroStream)
        );
        assert_eq!(FileType::from_path("flight.bin"), Some(FileType::Dataflash));
    }

    #[test]
    fn from_path_matches_video_extensions() {
        assert_eq!(FileType::from_path("clip.mp4"), Some(FileType::Mp4));
        assert_eq!(FileType::from_path("clip.MKV"), Some(FileType::Mkv));
        assert_eq!(FileType::from_path("clip.avi"), Some(FileType::Avi));
        assert_eq!(FileType::from_path("clip.ts"), Some(FileType::Ts));
    }

    #[test]
    fn is_video_matches_only_video_extensions() {
        assert!(FileType::Mp4.is_video());
        assert!(FileType::Mkv.is_video());
        assert!(FileType::Avi.is_video());
        assert!(FileType::Ts.is_video());
        assert!(!FileType::Mcap.is_video());
        assert!(!FileType::Csv.is_video());
    }
}
