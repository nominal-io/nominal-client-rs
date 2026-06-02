use chrono::{DateTime, Utc};
use nominal_api::objects::api::rids::WorkspaceRid;
use nominal_api::objects::api::{Label, Property, PropertyName, PropertyValue};
use nominal_api::objects::scout::catalog::{
    ChannelConfig, CreateDataset, DatasetOriginMetadata, SearchDatasetsQuery, UpdateDatasetMetadata,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::Result;
use crate::core::rid::{parse_rid, rid_to_string};

/// Represents a dataset in Nominal.
///
/// Datasets are time-series data sources that have been uploaded to Nominal,
/// typically from CSV or other file formats.
#[derive(Debug, Clone)]
pub struct Dataset {
    rid: String,
    name: String,
    description: Option<String>,
    properties: HashMap<String, String>,
    labels: Vec<String>,
    created_at: DateTime<Utc>,
    app_base_url: String,
}

impl Dataset {
    pub fn rid(&self) -> &str {
        &self.rid
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub fn properties(&self) -> &HashMap<String, String> {
        &self.properties
    }

    pub fn labels(&self) -> &[String] {
        &self.labels
    }

    pub fn created_at(&self) -> &DateTime<Utc> {
        &self.created_at
    }

    /// Get the URL to view this dataset in the Nominal web app.
    pub fn nominal_url(&self) -> String {
        format!("{}/data-sources/{}", self.app_base_url, self.rid)
    }

    pub(crate) fn from_conjure(
        dataset: nominal_api::objects::scout::catalog::EnrichedDataset,
        app_base_url: &str,
    ) -> Self {
        Self {
            rid: rid_to_string(dataset.rid()),
            name: dataset.name().to_string(),
            description: dataset
                .description()
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string()),
            properties: dataset
                .properties()
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            labels: dataset.labels().iter().map(|l| l.to_string()).collect(),
            created_at: dataset.ingest_date().to_utc(),
            app_base_url: app_base_url.to_string(),
        }
    }
}

/// Parameters for creating a new dataset.
#[derive(Debug, Clone)]
pub struct DatasetCreate {
    name: String,
    description: Option<String>,
    properties: Option<HashMap<String, String>>,
    labels: Option<Vec<String>>,
    channel_delimiter: Option<String>,
}

impl DatasetCreate {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            properties: None,
            labels: None,
            channel_delimiter: None,
        }
    }

    #[must_use]
    pub fn description(mut self, value: impl Into<String>) -> Self {
        self.description = Some(value.into());
        self
    }

    #[must_use]
    pub fn properties<I, K, V>(mut self, value: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.properties = Some(
            value
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        );
        self
    }

    #[must_use]
    pub fn labels<I>(mut self, value: I) -> Self
    where
        I: IntoIterator,
        I::Item: Into<String>,
    {
        self.labels = Some(value.into_iter().map(Into::into).collect());
        self
    }

    #[must_use]
    pub fn channel_delimiter(mut self, value: impl Into<String>) -> Self {
        self.channel_delimiter = Some(value.into());
        self
    }

    pub(crate) fn into_request(self, workspace_rid: &str) -> Result<CreateDataset> {
        let DatasetCreate {
            name,
            description,
            properties,
            labels,
            channel_delimiter,
        } = self;

        let mut origin = DatasetOriginMetadata::builder();
        if let Some(d) = channel_delimiter {
            origin =
                origin.channel_config(ChannelConfig::builder().prefix_tree_delimiter(d).build());
        }
        let mut b = CreateDataset::builder()
            .name(name)
            .origin_metadata(origin.build());

        if let Some(d) = description {
            b = b.description(d);
        }
        if let Some(p) = properties {
            b = b.properties(
                p.into_iter()
                    .map(|(k, v)| (PropertyName(k), PropertyValue(v)))
                    .collect::<BTreeMap<_, _>>(),
            );
        }
        if let Some(l) = labels {
            b = b.labels(l.into_iter().map(Label).collect::<BTreeSet<_>>());
        }
        b = b.workspace(parse_rid::<WorkspaceRid>(workspace_rid)?);

        Ok(b.build())
    }

    pub(crate) fn into_new_ingest_destination(
        self,
        workspace_rid: &str,
    ) -> Result<nominal_api::objects::ingest::api::NewDatasetIngestDestination> {
        let DatasetCreate {
            name,
            description,
            properties,
            labels,
            channel_delimiter,
        } = self;

        let mut b = nominal_api::objects::ingest::api::NewDatasetIngestDestination::builder()
            .dataset_name(name);
        if let Some(d) = channel_delimiter {
            b = b.channel_config(
                nominal_api::objects::ingest::api::ChannelConfig::builder()
                    .prefix_tree_delimiter(d)
                    .build(),
            );
        }
        if let Some(d) = description {
            b = b.dataset_description(d);
        }
        if let Some(p) = properties {
            b = b.properties(
                p.into_iter()
                    .map(|(k, v)| (PropertyName(k), PropertyValue(v)))
                    .collect::<BTreeMap<_, _>>(),
            );
        }
        if let Some(l) = labels {
            b = b.labels(l.into_iter().map(Label).collect::<BTreeSet<_>>());
        }
        b = b.workspace(parse_rid::<WorkspaceRid>(workspace_rid)?);

        Ok(b.build())
    }
}

/// An update to dataset metadata. Only fields that are set will be changed.
#[derive(Debug, Default, Clone)]
pub struct DatasetUpdate {
    name: Option<String>,
    description: Option<String>,
    properties: Option<HashMap<String, String>>,
    labels: Option<Vec<String>>,
}

impl DatasetUpdate {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn name(mut self, value: impl Into<String>) -> Self {
        self.name = Some(value.into());
        self
    }

    #[must_use]
    pub fn description(mut self, value: impl Into<String>) -> Self {
        self.description = Some(value.into());
        self
    }

    #[must_use]
    pub fn properties<I, K, V>(mut self, value: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.properties = Some(
            value
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        );
        self
    }

    #[must_use]
    pub fn labels<I>(mut self, value: I) -> Self
    where
        I: IntoIterator,
        I::Item: Into<String>,
    {
        self.labels = Some(value.into_iter().map(Into::into).collect());
        self
    }

    pub(crate) fn into_request(self) -> UpdateDatasetMetadata {
        let DatasetUpdate {
            name,
            description,
            properties,
            labels,
        } = self;

        let mut b = UpdateDatasetMetadata::builder();
        if let Some(n) = name {
            b = b.name(n);
        }
        if let Some(d) = description {
            b = b.description(d);
        }
        if let Some(p) = properties {
            b = b.properties(Some(
                p.into_iter()
                    .map(|(k, v)| (PropertyName(k), PropertyValue(v)))
                    .collect::<BTreeMap<_, _>>(),
            ));
        }
        if let Some(l) = labels {
            b = b.labels(Some(l.into_iter().map(Label).collect::<BTreeSet<_>>()));
        }
        b.build()
    }
}

/// A query for searching datasets, composable with [`and`](DatasetQuery::and) and [`or`](DatasetQuery::or).
#[derive(Debug, Clone)]
pub enum DatasetQuery {
    /// Fuzzy full-text search against name and description.
    SearchText(String),
    /// Case-insensitive substring match against the dataset name.
    SubstringMatch(String),
    /// Filter by label.
    Label(String),
    /// Filter by property key and value.
    Property(String, String),
    /// All sub-queries must match.
    And(Vec<DatasetQuery>),
    /// At least one sub-query must match.
    Or(Vec<DatasetQuery>),
}

impl DatasetQuery {
    pub fn search_text(text: impl Into<String>) -> Self {
        Self::SearchText(text.into())
    }

    pub fn substring_match(text: impl Into<String>) -> Self {
        Self::SubstringMatch(text.into())
    }

    pub fn label(label: impl Into<String>) -> Self {
        Self::Label(label.into())
    }

    pub fn property(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self::Property(key.into(), value.into())
    }

    pub fn and(queries: impl IntoIterator<Item = DatasetQuery>) -> Self {
        Self::And(queries.into_iter().collect())
    }

    pub fn or(queries: impl IntoIterator<Item = DatasetQuery>) -> Self {
        Self::Or(queries.into_iter().collect())
    }

    pub(crate) fn collect_substring_matches(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.collect_substring_matches_into(&mut out);
        out
    }

    fn collect_substring_matches_into(&self, out: &mut Vec<String>) {
        match self {
            Self::SubstringMatch(s) => out.push(s.clone()),
            Self::And(qs) => qs
                .iter()
                .for_each(|q| q.collect_substring_matches_into(out)),
            _ => {}
        }
    }

    pub(crate) fn into_conjure(self) -> SearchDatasetsQuery {
        match self {
            Self::SearchText(s) => SearchDatasetsQuery::SearchText(s),
            Self::SubstringMatch(s) => SearchDatasetsQuery::ExactMatch(s),
            Self::Label(l) => SearchDatasetsQuery::Label(Label(l)),
            Self::Property(k, v) => {
                SearchDatasetsQuery::Properties(Property::new(PropertyName(k), PropertyValue(v)))
            }
            Self::And(qs) => SearchDatasetsQuery::And(
                qs.into_iter()
                    .map(Self::into_conjure)
                    .collect::<BTreeSet<_>>(),
            ),
            Self::Or(qs) => SearchDatasetsQuery::Or(
                qs.into_iter()
                    .map(Self::into_conjure)
                    .collect::<BTreeSet<_>>(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_search_text() {
        let q = DatasetQuery::search_text("hello");
        assert_eq!(
            q.into_conjure(),
            SearchDatasetsQuery::SearchText("hello".into())
        );
    }

    #[test]
    fn query_substring_match() {
        let q = DatasetQuery::substring_match("exact");
        assert_eq!(
            q.into_conjure(),
            SearchDatasetsQuery::ExactMatch("exact".into())
        );
    }

    #[test]
    fn query_label() {
        let q = DatasetQuery::label("my-label");
        assert_eq!(
            q.into_conjure(),
            SearchDatasetsQuery::Label(nominal_api::objects::api::Label("my-label".into()))
        );
    }

    #[test]
    fn query_property() {
        let q = DatasetQuery::property("key", "val");
        let SearchDatasetsQuery::Properties(p) = q.into_conjure() else {
            panic!("expected Properties variant");
        };
        assert_eq!(
            p.name(),
            &nominal_api::objects::api::PropertyName("key".into())
        );
        assert_eq!(
            p.value(),
            &nominal_api::objects::api::PropertyValue("val".into())
        );
    }

    #[test]
    fn query_and_children() {
        let q = DatasetQuery::and([
            DatasetQuery::search_text("a"),
            DatasetQuery::search_text("b"),
        ]);
        let SearchDatasetsQuery::And(children) = q.into_conjure() else {
            panic!("expected And variant");
        };
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn query_or_children() {
        let q = DatasetQuery::or([DatasetQuery::label("x"), DatasetQuery::label("y")]);
        let SearchDatasetsQuery::Or(children) = q.into_conjure() else {
            panic!("expected Or variant");
        };
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn query_nested() {
        let q = DatasetQuery::and([
            DatasetQuery::label("prod"),
            DatasetQuery::or([
                DatasetQuery::property("env", "us"),
                DatasetQuery::property("env", "eu"),
            ]),
        ]);
        let SearchDatasetsQuery::And(children) = q.into_conjure() else {
            panic!("expected And");
        };
        assert!(
            children
                .iter()
                .any(|c| matches!(c, SearchDatasetsQuery::Label(_)))
        );
        assert!(
            children
                .iter()
                .any(|c| matches!(c, SearchDatasetsQuery::Or(_)))
        );
    }

    #[test]
    fn update_empty() {
        let req = DatasetUpdate::new().into_request();
        assert!(req.name().is_none());
        assert!(req.description().is_none());
        assert!(req.properties().is_none());
        assert!(req.labels().is_none());
    }

    #[test]
    fn update_name_only() {
        let req = DatasetUpdate::new().name("New Name").into_request();
        assert_eq!(req.name(), Some("New Name"));
        assert!(req.description().is_none());
    }

    #[test]
    fn update_all_fields() {
        let req = DatasetUpdate::new()
            .name("name")
            .description("desc")
            .properties([("k", "v")])
            .labels(["t1", "t2", "t1"])
            .into_request();
        assert_eq!(req.name(), Some("name"));
        assert_eq!(req.description(), Some("desc"));
        assert_eq!(req.properties().unwrap().len(), 1);
        assert_eq!(req.labels().unwrap().len(), 2); // deduplicated
    }
}
