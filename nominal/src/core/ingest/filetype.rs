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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(
            FileType::from_path("flight.bin"),
            Some(FileType::Dataflash)
        );
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
