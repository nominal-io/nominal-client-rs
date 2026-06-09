use nominal_api::objects::api::rids::DataSourceRid;
use nominal_api::objects::api::{
    Channel as ApiChannel, SeriesDataType as ApiSeriesDataType, Unit as ApiUnit,
};
use nominal_api::objects::datasource::api::ChannelMetadata as SearchChannelMetadata;
use nominal_api::objects::storage::series::api::NominalDataType;
use nominal_api::objects::timeseries::channelmetadata::api::ChannelMetadata as StoredChannelMetadata;
use nominal_api::objects::timeseries::metadata::api::{
    CreateSeriesMetadataRequest, LocatorTemplate, NominalLocatorTemplate,
};
use std::collections::BTreeSet;

use crate::core::rid::{parse_rid, rid_to_string};
use crate::{Error, Result};

/// A time-series channel on a data source (dataset, video, connection, etc.).
///
/// Carries the channel's identity and its editable metadata (description + unit)
/// plus the channel data type.
#[derive(Debug, Clone)]
pub struct Channel {
    data_source_rid: String,
    name: String,
    description: Option<String>,
    unit: Option<String>,
    data_type: ChannelDataType,
}

impl Channel {
    /// RID of the data source (dataset, video, connection, etc.) that owns this channel.
    pub fn data_source_rid(&self) -> &str {
        &self.data_source_rid
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// Unit symbol (e.g. `"m/s"`) if set.
    pub fn unit(&self) -> Option<&str> {
        self.unit.as_deref()
    }

    pub fn data_type(&self) -> &ChannelDataType {
        &self.data_type
    }

    pub(crate) fn from_search(meta: SearchChannelMetadata) -> Result<Self> {
        let data_type = meta.data_type().map(ChannelDataType::from).ok_or_else(|| {
            Error::MissingChannelDataType {
                channel: meta.name().to_string(),
            }
        })?;
        Ok(Self {
            data_source_rid: rid_to_string(meta.data_source()),
            name: meta.name().to_string(),
            description: meta.description().map(str::to_string),
            unit: meta.unit().map(|u| u.symbol().to_string()),
            data_type,
        })
    }

    pub(crate) fn from_stored(meta: StoredChannelMetadata) -> Result<Self> {
        let id = meta.channel_identifier();
        let data_type = meta.data_type().map(ChannelDataType::from).ok_or_else(|| {
            Error::MissingChannelDataType {
                channel: id.channel_name().to_string(),
            }
        })?;
        Ok(Self {
            data_source_rid: rid_to_string(id.data_source_rid()),
            name: id.channel_name().to_string(),
            description: meta.description().map(str::to_string),
            unit: meta.unit().map(|u| u.to_string()),
            data_type,
        })
    }

    pub(crate) fn from_update(
        data_source_rid: impl Into<String>,
        name: impl Into<String>,
        update: &ChannelUpdate,
    ) -> Self {
        Self {
            data_source_rid: data_source_rid.into(),
            name: name.into(),
            description: update.description.clone(),
            unit: match &update.unit_update {
                Some(UnitUpdate::Set(unit)) => Some(unit.clone()),
                Some(UnitUpdate::Clear) | None => None,
            },
            data_type: update.data_type.clone(),
        }
    }
}

/// The data type of a channel's values.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ChannelDataType {
    Double,
    Int,
    Uint,
    String,
    Log,
    DoubleArray,
    StringArray,
    Struct,
    Video,
    /// A data type this client does not recognize.
    Unknown(String),
}

impl From<&ApiSeriesDataType> for ChannelDataType {
    fn from(t: &ApiSeriesDataType) -> Self {
        match t {
            ApiSeriesDataType::Double => Self::Double,
            ApiSeriesDataType::Int => Self::Int,
            ApiSeriesDataType::Uint => Self::Uint,
            ApiSeriesDataType::String => Self::String,
            ApiSeriesDataType::Log => Self::Log,
            ApiSeriesDataType::DoubleArray => Self::DoubleArray,
            ApiSeriesDataType::StringArray => Self::StringArray,
            ApiSeriesDataType::Struct => Self::Struct,
            ApiSeriesDataType::Video => Self::Video,
            ApiSeriesDataType::Unknown(u) => Self::Unknown(u.to_string()),
        }
    }
}

impl ChannelDataType {
    /// Returns `None` for `Unknown` — there is no meaningful way to use an
    /// unrecognized data type as a server-side filter, so it is skipped.
    fn into_api(self) -> Option<ApiSeriesDataType> {
        Some(match self {
            Self::Double => ApiSeriesDataType::Double,
            Self::Int => ApiSeriesDataType::Int,
            Self::Uint => ApiSeriesDataType::Uint,
            Self::String => ApiSeriesDataType::String,
            Self::Log => ApiSeriesDataType::Log,
            Self::DoubleArray => ApiSeriesDataType::DoubleArray,
            Self::StringArray => ApiSeriesDataType::StringArray,
            Self::Struct => ApiSeriesDataType::Struct,
            Self::Video => ApiSeriesDataType::Video,
            Self::Unknown(_) => return None,
        })
    }

    pub(crate) fn into_nominal_data_type(self) -> Result<NominalDataType> {
        Ok(match self {
            Self::Double => NominalDataType::Double,
            Self::Int => NominalDataType::Int64,
            Self::Uint => NominalDataType::Uint64,
            Self::String => NominalDataType::String,
            Self::Log => NominalDataType::Log,
            Self::DoubleArray => NominalDataType::DoubleArray,
            Self::StringArray => NominalDataType::StringArray,
            Self::Struct => NominalDataType::Struct,
            Self::Video => NominalDataType::Video,
            Self::Unknown(data_type) => {
                return Err(Error::UnsupportedChannelDataType { data_type });
            }
        })
    }
}

/// A query for searching channels.
///
/// All fields are AND'd together. An empty query (the default) matches every
/// channel the caller is authorized to see.
#[derive(Debug, Default, Clone)]
pub struct ChannelQuery {
    data_source_rids: Vec<String>,
    substring_matches: Vec<String>,
    data_types: Vec<ChannelDataType>,
}

impl ChannelQuery {
    pub fn new() -> Self {
        Self::default()
    }

    /// Restrict the search to channels on a specific data source. Repeatable.
    #[must_use]
    pub fn data_source(mut self, rid: impl Into<String>) -> Self {
        self.data_source_rids.push(rid.into());
        self
    }

    /// Restrict the search to channels on any of the given data sources.
    #[must_use]
    pub fn data_sources<I, S>(mut self, rids: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.data_source_rids
            .extend(rids.into_iter().map(Into::into));
        self
    }

    /// Require the channel name to contain this substring (case-insensitive).
    /// Repeatable; all required substrings must be present.
    #[must_use]
    pub fn substring_match(mut self, text: impl Into<String>) -> Self {
        self.substring_matches.push(text.into());
        self
    }

    /// Restrict the search to channels of the given data type. Repeatable.
    #[must_use]
    pub fn data_type(mut self, data_type: ChannelDataType) -> Self {
        self.data_types.push(data_type);
        self
    }

    pub(crate) fn substring_match_filters(&self) -> &[String] {
        &self.substring_matches
    }

    pub(crate) fn into_parts(self) -> Result<ChannelSearchParts> {
        let data_source_rids = self
            .data_source_rids
            .iter()
            .map(|s| parse_rid::<DataSourceRid>(s).map_err(crate::Error::from))
            .collect::<Result<BTreeSet<_>>>()?;
        let data_types = self
            .data_types
            .into_iter()
            .filter_map(ChannelDataType::into_api)
            .collect::<BTreeSet<_>>();
        Ok(ChannelSearchParts {
            data_source_rids,
            substring_matches: self.substring_matches,
            data_types,
        })
    }
}

pub(crate) struct ChannelSearchParts {
    pub data_source_rids: BTreeSet<DataSourceRid>,
    pub substring_matches: Vec<String>,
    pub data_types: BTreeSet<ApiSeriesDataType>,
}

/// An update to a channel's metadata. Only optional fields that are set will change.
#[derive(Debug, Clone)]
pub struct ChannelUpdate {
    description: Option<String>,
    unit_update: Option<UnitUpdate>,
    data_type: ChannelDataType,
}

#[derive(Debug, Clone)]
enum UnitUpdate {
    Set(String),
    Clear,
}

impl ChannelUpdate {
    pub fn new(data_type: ChannelDataType) -> Self {
        Self {
            description: None,
            unit_update: None,
            data_type,
        }
    }

    /// Set the channel's description.
    #[must_use]
    pub fn description(mut self, value: impl Into<String>) -> Self {
        self.description = Some(value.into());
        self
    }

    /// Set the channel's unit by symbol (e.g. `"m/s"`, `"celsius"`).
    #[must_use]
    pub fn unit(mut self, symbol: impl Into<String>) -> Self {
        self.unit_update = Some(UnitUpdate::Set(symbol.into()));
        self
    }

    /// Clear any unit previously set on the channel.
    #[must_use]
    pub fn clear_unit(mut self) -> Self {
        self.unit_update = Some(UnitUpdate::Clear);
        self
    }

    pub(crate) fn into_series_metadata_request(
        self,
        data_source_rid: &str,
        name: &str,
    ) -> Result<CreateSeriesMetadataRequest> {
        let channel = ApiChannel(name.to_string());
        let data_type = self.data_type.into_nominal_data_type()?;
        let locator =
            LocatorTemplate::Nominal(NominalLocatorTemplate::new(channel.clone(), data_type));
        let mut b = CreateSeriesMetadataRequest::builder()
            .channel(channel)
            .data_source_rid(parse_rid::<DataSourceRid>(data_source_rid)?)
            .locator(locator);

        if let Some(d) = self.description {
            b = b.description(d);
        }
        if let Some(UnitUpdate::Set(symbol)) = self.unit_update {
            b = b.unit(ApiUnit(symbol));
        }

        Ok(b.build())
    }
}
