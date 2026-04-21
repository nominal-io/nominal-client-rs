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
}

impl FileType {
    pub const fn extension(self) -> &'static str {
        match self {
            FileType::Csv => ".csv",
            FileType::CsvGz => ".csv.gz",
            FileType::Parquet => ".parquet",
        }
    }

    pub const fn mime_type(self) -> &'static str {
        match self {
            FileType::Csv => "text/csv",
            FileType::CsvGz => "application/gzip",
            FileType::Parquet => "application/vnd.apache.parquet",
        }
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
        assert_eq!(FileType::from_path("/tmp/nested/data.parquet"), Some(FileType::Parquet));
    }

    #[test]
    fn from_path_returns_none_for_unknown() {
        assert_eq!(FileType::from_path("foo.txt"), None);
        assert_eq!(FileType::from_path("foo"), None);
    }
}
