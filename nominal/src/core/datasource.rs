use nominal_api::scout::run::api::{ConnectionRid, DataSource as ConjureDataSource};

use crate::core::rid::{parse_rid, rid_to_string};
use crate::{Error, Result};

/// A data source attached to an asset or run by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataSource {
    Dataset(String),
    Video(String),
    Connection(String),
}

impl DataSource {
    pub fn dataset(rid: impl Into<String>) -> Self {
        Self::Dataset(rid.into())
    }

    pub fn video(rid: impl Into<String>) -> Self {
        Self::Video(rid.into())
    }

    pub fn connection(rid: impl Into<String>) -> Self {
        Self::Connection(rid.into())
    }

    /// The RID of this data source, regardless of its kind.
    pub fn rid(&self) -> &str {
        match self {
            Self::Dataset(r) | Self::Video(r) | Self::Connection(r) => r,
        }
    }

    /// Convert a conjure `DataSource`, returning `None` for variants we skip
    /// (log sets — deprecated) or don't recognize.
    pub(crate) fn from_conjure(src: &ConjureDataSource) -> Option<Self> {
        match src {
            ConjureDataSource::Dataset(rid) => Some(Self::Dataset(rid_to_string(rid))),
            ConjureDataSource::Video(rid) => Some(Self::Video(rid_to_string(rid))),
            ConjureDataSource::Connection(rid) => Some(Self::Connection(rid_to_string(rid))),
            ConjureDataSource::LogSet(rid) => {
                tracing::debug!(rid = %rid, "ignoring deprecated log-set data source");
                None
            }
            ConjureDataSource::Unknown(u) => {
                tracing::warn!(type_ = ?u, "ignoring unknown data source variant");
                None
            }
        }
    }

    pub(crate) fn into_conjure(self) -> Result<ConjureDataSource> {
        Ok(match self {
            Self::Dataset(r) => ConjureDataSource::Dataset(parse_rid(&r).map_err(Error::from)?),
            Self::Video(r) => ConjureDataSource::Video(parse_rid(&r).map_err(Error::from)?),
            Self::Connection(r) => {
                ConjureDataSource::Connection(parse_rid::<ConnectionRid>(&r).map_err(Error::from)?)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DATASET_RID: &str = "ri.catalog.cerulean-staging.dataset.00000000-0000-0000-0000-000000000001";
    const VIDEO_RID: &str = "ri.catalog.cerulean-staging.video.00000000-0000-0000-0000-000000000002";
    const CONNECTION_RID: &str =
        "ri.datasource.cerulean-staging.connection.00000000-0000-0000-0000-000000000003";
    const LOGSET_RID: &str = "ri.logset.cerulean-staging.log-set.00000000-0000-0000-0000-000000000004";

    #[test]
    fn rid_accessor() {
        assert_eq!(DataSource::dataset(DATASET_RID).rid(), DATASET_RID);
        assert_eq!(DataSource::video(VIDEO_RID).rid(), VIDEO_RID);
        assert_eq!(DataSource::connection(CONNECTION_RID).rid(), CONNECTION_RID);
    }

    #[test]
    fn round_trip_through_conjure() {
        for ds in [
            DataSource::dataset(DATASET_RID),
            DataSource::video(VIDEO_RID),
            DataSource::connection(CONNECTION_RID),
        ] {
            let conjure = ds.clone().into_conjure().unwrap();
            assert_eq!(DataSource::from_conjure(&conjure), Some(ds));
        }
    }

    #[test]
    fn log_sets_are_filtered() {
        use nominal_api::scout::run::api::LogSetRid;
        let rid = parse_rid::<LogSetRid>(LOGSET_RID).unwrap();
        assert_eq!(DataSource::from_conjure(&ConjureDataSource::LogSet(rid)), None);
    }

    #[test]
    fn into_conjure_rejects_invalid_rid() {
        assert!(DataSource::dataset("not a rid").into_conjure().is_err());
    }
}
