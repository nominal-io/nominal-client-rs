use nominal_api::objects::api::rids::DataSourceRid;
use nominal_api::objects::api::{
    Channel as ApiChannel, Empty, SeriesDataType as ApiSeriesDataType, Unit as ApiUnit,
};
use nominal_api::objects::datasource::api::ChannelMetadata as SearchChannelMetadata;
use nominal_api::objects::timeseries::channelmetadata::api::{
    ChannelIdentifier, ChannelMetadata as StoredChannelMetadata, UpdateChannelMetadataRequest,
};
use nominal_api::objects::timeseries::logicalseries::api::UnitUpdate as ApiUnitUpdate;
use std::collections::BTreeSet;

use crate::Result;
use crate::core::rid::{parse_rid, rid_to_string};

/// A time-series channel on a data source (dataset, video, connection, etc.).
///
/// Carries the channel's identity and its editable metadata (description + unit)
/// plus the server-detected data type.
#[derive(Debug, Clone)]
pub struct Channel {
    data_source_rid: String,
    name: String,
    description: Option<String>,
    unit: Option<String>,
    data_type: Option<ChannelDataType>,
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

    pub fn data_type(&self) -> Option<&ChannelDataType> {
        self.data_type.as_ref()
    }

    pub(crate) fn from_search(meta: SearchChannelMetadata) -> Self {
        Self {
            data_source_rid: rid_to_string(meta.data_source()),
            name: meta.name().to_string(),
            description: meta.description().map(str::to_string),
            unit: meta.unit().map(|u| u.symbol().to_string()),
            data_type: meta.data_type().map(ChannelDataType::from),
        }
    }

    pub(crate) fn from_stored(meta: StoredChannelMetadata) -> Self {
        let id = meta.channel_identifier();
        Self {
            data_source_rid: rid_to_string(id.data_source_rid()),
            name: id.channel_name().to_string(),
            description: meta.description().map(str::to_string),
            unit: meta.unit().map(|u| u.to_string()),
            data_type: meta.data_type().map(ChannelDataType::from),
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
}

/// A query for searching channels.
///
/// All fields are AND'd together. An empty query (the default) matches every
/// channel the caller is authorized to see.
#[derive(Debug, Default, Clone)]
pub struct ChannelQuery {
    fuzzy_text: String,
    data_source_rids: Vec<String>,
    exact_matches: Vec<String>,
    data_types: Vec<ChannelDataType>,
}

impl ChannelQuery {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fuzzy full-text match against channel name. Results are ranked by
    /// similarity to this text.
    #[must_use]
    pub fn fuzzy_text(mut self, text: impl Into<String>) -> Self {
        self.fuzzy_text = text.into();
        self
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
    /// Repeatable; all exact-match substrings must be present.
    #[must_use]
    pub fn exact_match(mut self, text: impl Into<String>) -> Self {
        self.exact_matches.push(text.into());
        self
    }

    /// Restrict the search to channels of the given data type. Repeatable.
    #[must_use]
    pub fn data_type(mut self, data_type: ChannelDataType) -> Self {
        self.data_types.push(data_type);
        self
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
        let exact_matches = self.exact_matches.into_iter().collect::<BTreeSet<_>>();
        Ok(ChannelSearchParts {
            fuzzy_text: self.fuzzy_text,
            data_source_rids,
            exact_matches,
            data_types,
        })
    }
}

pub(crate) struct ChannelSearchParts {
    pub fuzzy_text: String,
    pub data_source_rids: BTreeSet<DataSourceRid>,
    pub exact_matches: BTreeSet<String>,
    pub data_types: BTreeSet<ApiSeriesDataType>,
}

/// An update to a channel's metadata. Only fields that are set will change.
#[derive(Debug, Default, Clone)]
pub struct ChannelUpdate {
    description: Option<String>,
    unit_update: Option<UnitUpdate>,
}

#[derive(Debug, Clone)]
enum UnitUpdate {
    Set(String),
    Clear,
}

impl ChannelUpdate {
    pub fn new() -> Self {
        Self::default()
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

    pub(crate) fn into_request(
        self,
        data_source_rid: &str,
        name: &str,
    ) -> Result<UpdateChannelMetadataRequest> {
        let id = ChannelIdentifier::new(
            ApiChannel(name.to_string()),
            parse_rid::<DataSourceRid>(data_source_rid)?,
        );
        let mut b = UpdateChannelMetadataRequest::builder().channel_identifier(id);
        if let Some(d) = self.description {
            b = b.description(d);
        }
        if let Some(u) = self.unit_update {
            let update = match u {
                UnitUpdate::Set(symbol) => ApiUnitUpdate::Unit(ApiUnit(symbol)),
                UnitUpdate::Clear => ApiUnitUpdate::ClearUnit(Empty::new()),
            };
            b = b.unit_update(update);
        }
        Ok(b.build())
    }
}
