use std::sync::Arc;

use chrono::{DateTime, Utc};
use conjure_http::client::{AsyncService, ConjureRuntime};
use conjure_object::BearerToken;
use conjure_runtime::Client;
use nominal_api::clients::scout::{AsyncTemplateService, AsyncTemplateServiceClient};
use nominal_api::objects::scout::layout::api::WorkbookLayout;
use nominal_api::objects::scout::workbookcommon::api::WorkbookContent;

use crate::core::rid::{parse_rid, rid_to_string};
use crate::{Error, Result};

/// Represents a workbook template in Nominal.
///
/// Templates are versioned workbook definitions that can be applied to assets or runs
/// to create new workbooks.
#[derive(Debug, Clone)]
pub struct Template {
    rid: String,
    title: String,
    description: Option<String>,
    commit_id: String,
    created_at: DateTime<Utc>,
    layout: WorkbookLayout,
    content: WorkbookContent,
    app_base_url: String,
}

impl Template {
    pub fn rid(&self) -> &str {
        &self.rid
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// The commit ID identifying this template version.
    pub fn commit_id(&self) -> &str {
        &self.commit_id
    }

    pub fn created_at(&self) -> &DateTime<Utc> {
        &self.created_at
    }

    /// The template's layout as a JSON string.
    ///
    /// The schema is tied to the Nominal frontend and may evolve.
    pub fn layout_json(&self) -> String {
        serde_json::to_string(&self.layout).expect("WorkbookLayout always serializes to JSON")
    }

    /// The template's content as a JSON string.
    ///
    /// The schema is tied to the Nominal frontend and may evolve.
    pub fn content_json(&self) -> String {
        serde_json::to_string(&self.content).expect("WorkbookContent always serializes to JSON")
    }

    /// Raw layout for internal use when building a `CreateNotebookRequest`.
    /// These are crate-private so callers can't reach conjure types from the public API.
    pub(crate) fn layout(&self) -> &WorkbookLayout {
        &self.layout
    }

    /// Raw content for internal use when building a `CreateNotebookRequest`.
    /// These are crate-private so callers can't reach conjure types from the public API.
    pub(crate) fn content(&self) -> &WorkbookContent {
        &self.content
    }

    /// Get the URL to view this template in the Nominal web app.
    pub fn nominal_url(&self) -> String {
        format!("{}/workbooks/templates/{}", self.app_base_url, self.rid)
    }

    pub(crate) fn from_conjure(
        template: nominal_api::objects::scout::template::api::Template,
        app_base_url: &str,
    ) -> Self {
        let metadata = template.metadata();
        let description = if metadata.description().is_empty() {
            None
        } else {
            Some(metadata.description().to_string())
        };
        Self {
            rid: rid_to_string(template.rid()),
            title: metadata.title().to_string(),
            description,
            commit_id: template.commit().id().to_string(),
            created_at: metadata.created_at().to_utc(),
            layout: template.layout().clone(),
            content: template.content().clone(),
            app_base_url: app_base_url.to_string(),
        }
    }
}

/// Client for template operations (get).
pub struct TemplatesClient {
    service: AsyncTemplateServiceClient<Client>,
    token: BearerToken,
    app_base_url: String,
}

impl TemplatesClient {
    pub(crate) fn new(
        client: Client,
        runtime: &Arc<ConjureRuntime>,
        token: BearerToken,
        app_base_url: String,
    ) -> Self {
        Self {
            service: AsyncTemplateServiceClient::new(client, runtime),
            token,
            app_base_url,
        }
    }

    /// Get a template by RID. Returns the latest commit on the main branch.
    pub async fn get(&self, rid: &str) -> Result<Template> {
        let template_rid = parse_rid(rid)?;
        let response = self
            .service
            .get(&self.token, &template_rid, None, None)
            .await
            .map_err(Error::from)?;
        Ok(Template::from_conjure(response, &self.app_base_url))
    }
}
