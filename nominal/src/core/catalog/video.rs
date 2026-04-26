use chrono::{DateTime, Utc};
use nominal_api::objects::api::rids::WorkspaceRid;
use nominal_api::objects::api::{Label, Property, PropertyName, PropertyValue};
use nominal_api::objects::scout::video::api::{
    CreateVideoRequest, SearchVideosQuery, UpdateVideoMetadataRequest, Video as ApiVideo,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::Result;
use crate::core::rid::{parse_rid, rid_to_string};

/// Represents a video in Nominal.
#[derive(Debug, Clone)]
pub struct Video {
    rid: String,
    name: String,
    description: Option<String>,
    properties: HashMap<String, String>,
    labels: Vec<String>,
    created_at: DateTime<Utc>,
    app_base_url: String,
}

impl Video {
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

    /// Get the URL to view this video in the Nominal web app.
    pub fn nominal_url(&self) -> String {
        format!("{}/videos/{}", self.app_base_url, self.rid)
    }

    pub(crate) fn from_conjure(video: ApiVideo, app_base_url: &str) -> Self {
        Self {
            rid: rid_to_string(video.rid()),
            name: video.title().to_string(),
            description: video
                .description()
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string()),
            properties: video
                .properties()
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            labels: video.labels().iter().map(|l| l.to_string()).collect(),
            created_at: video.created_at().to_utc(),
            app_base_url: app_base_url.to_string(),
        }
    }
}

/// Parameters for creating a new video.
#[derive(Debug, Clone)]
pub struct VideoCreate {
    name: String,
    description: Option<String>,
    properties: Option<HashMap<String, String>>,
    labels: Option<Vec<String>>,
}

impl VideoCreate {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            properties: None,
            labels: None,
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

    pub(crate) fn into_request(self, workspace_rid: Option<&str>) -> Result<CreateVideoRequest> {
        let VideoCreate {
            name,
            description,
            properties,
            labels,
        } = self;

        let mut b = CreateVideoRequest::builder().title(name);

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
        if let Some(wid) = workspace_rid {
            b = b.workspace(parse_rid::<WorkspaceRid>(wid)?);
        }

        Ok(b.build())
    }

    pub(crate) fn into_new_ingest_destination(
        self,
        workspace_rid: Option<&str>,
    ) -> Result<nominal_api::objects::ingest::api::NewVideoIngestDestination> {
        let VideoCreate {
            name,
            description,
            properties,
            labels,
        } = self;

        let mut b = nominal_api::objects::ingest::api::NewVideoIngestDestination::builder()
            .title(name);
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
        if let Some(wid) = workspace_rid {
            b = b.workspace(parse_rid::<WorkspaceRid>(wid)?);
        }

        Ok(b.build())
    }
}

/// An update to video metadata. Only fields that are set will be changed.
#[derive(Debug, Default, Clone)]
pub struct VideoUpdate {
    name: Option<String>,
    description: Option<String>,
    properties: Option<HashMap<String, String>>,
    labels: Option<Vec<String>>,
}

impl VideoUpdate {
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

    pub(crate) fn into_request(self) -> UpdateVideoMetadataRequest {
        let VideoUpdate {
            name,
            description,
            properties,
            labels,
        } = self;

        let mut b = UpdateVideoMetadataRequest::builder();
        if let Some(n) = name {
            b = b.title(n);
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

/// A query for searching videos, composable with [`and`](VideoQuery::and) and [`or`](VideoQuery::or).
#[derive(Debug, Clone)]
pub enum VideoQuery {
    /// Fuzzy full-text search against title and description.
    SearchText(String),
    /// Filter by label.
    Label(String),
    /// Filter by property key and value.
    Property(String, String),
    /// All sub-queries must match.
    And(Vec<VideoQuery>),
    /// At least one sub-query must match.
    Or(Vec<VideoQuery>),
}

impl VideoQuery {
    pub fn search_text(text: impl Into<String>) -> Self {
        Self::SearchText(text.into())
    }

    pub fn label(label: impl Into<String>) -> Self {
        Self::Label(label.into())
    }

    pub fn property(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self::Property(key.into(), value.into())
    }

    pub fn and(queries: impl IntoIterator<Item = VideoQuery>) -> Self {
        Self::And(queries.into_iter().collect())
    }

    pub fn or(queries: impl IntoIterator<Item = VideoQuery>) -> Self {
        Self::Or(queries.into_iter().collect())
    }

    pub(crate) fn into_conjure(self) -> SearchVideosQuery {
        match self {
            Self::SearchText(s) => SearchVideosQuery::SearchText(s),
            Self::Label(l) => SearchVideosQuery::Label(Label(l)),
            Self::Property(k, v) => {
                SearchVideosQuery::Property(Property::new(PropertyName(k), PropertyValue(v)))
            }
            Self::And(qs) => SearchVideosQuery::And(
                qs.into_iter()
                    .map(Self::into_conjure)
                    .collect::<BTreeSet<_>>(),
            ),
            Self::Or(qs) => SearchVideosQuery::Or(
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
        let q = VideoQuery::search_text("clip");
        assert_eq!(
            q.into_conjure(),
            SearchVideosQuery::SearchText("clip".into())
        );
    }

    #[test]
    fn query_label() {
        let q = VideoQuery::label("my-label");
        assert_eq!(
            q.into_conjure(),
            SearchVideosQuery::Label(nominal_api::objects::api::Label("my-label".into()))
        );
    }

    #[test]
    fn query_property() {
        let q = VideoQuery::property("cam", "front");
        let SearchVideosQuery::Property(p) = q.into_conjure() else {
            panic!("expected Property variant");
        };
        assert_eq!(
            p.name(),
            &nominal_api::objects::api::PropertyName("cam".into())
        );
        assert_eq!(
            p.value(),
            &nominal_api::objects::api::PropertyValue("front".into())
        );
    }

    #[test]
    fn query_and_or() {
        let q = VideoQuery::or([VideoQuery::label("a"), VideoQuery::label("b")]);
        let SearchVideosQuery::Or(children) = q.into_conjure() else {
            panic!("expected Or");
        };
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn update_empty() {
        let req = VideoUpdate::new().into_request();
        assert!(req.title().is_none());
        assert!(req.description().is_none());
        assert!(req.properties().is_none());
        assert!(req.labels().is_none());
    }

    #[test]
    fn update_all_fields() {
        let req = VideoUpdate::new()
            .name("vid")
            .description("desc")
            .properties([("k", "v")])
            .labels(["t"])
            .into_request();
        assert_eq!(req.title(), Some("vid"));
        assert_eq!(req.description(), Some("desc"));
        assert!(req.properties().is_some());
        assert!(req.labels().is_some());
    }
}
