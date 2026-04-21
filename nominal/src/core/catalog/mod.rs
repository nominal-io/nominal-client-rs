mod channel;
mod connection;
mod dataset;
mod video;

pub use channel::{Channel, ChannelDataType, ChannelQuery, ChannelUpdate};
pub use connection::{Connection, ConnectionUpdate};
pub use dataset::{Dataset, DatasetCreate, DatasetQuery, DatasetUpdate};
pub use video::{Video, VideoCreate, VideoQuery, VideoUpdate};

use std::sync::Arc;

use conjure_http::client::{AsyncService, ConjureRuntime};
use conjure_object::BearerToken;
use conjure_runtime::Client;
use futures::Stream;
use nominal_api::clients::scout::catalog::{AsyncCatalogService, AsyncCatalogServiceClient};
use nominal_api::clients::scout::datasource::connection::{
    AsyncConnectionService, AsyncConnectionServiceClient,
};
use nominal_api::clients::scout::datasource::{
    AsyncDataSourceService, AsyncDataSourceServiceClient,
};
use nominal_api::clients::scout::video::{AsyncVideoService, AsyncVideoServiceClient};
use nominal_api::clients::timeseries::channelmetadata::{
    AsyncChannelMetadataService, AsyncChannelMetadataServiceClient,
};
use nominal_api::objects::api::rids::{DataSourceRid, VideoRid};
use nominal_api::objects::datasource::api::{SearchChannelsRequest, SearchChannelsResponse};
use nominal_api::objects::scout::catalog::{
    GetDatasetsRequest, SearchDatasetsRequest, SearchDatasetsResponse,
    SortField as DatasetSortField, SortOptions as DatasetSortOptions,
};
use nominal_api::objects::scout::datasource::connection::api::{
    ConnectionRid, ListConnectionsResponse,
};
use nominal_api::objects::scout::video::api::{
    GetVideosRequest, SearchVideosRequest, SearchVideosResponse, SortField as VideoSortField,
    SortOptions as VideoSortOptions,
};
use nominal_api::objects::timeseries::channelmetadata::api::{
    ChannelIdentifier, GetChannelMetadataRequest,
};
use std::collections::{BTreeSet, HashMap};

use crate::core::rid::{parse_rid, rid_to_string};
use crate::core::utils::paginate_stream;
use crate::{Error, Result};
use futures::TryStreamExt;

/// Client for catalog operations: datasets, videos, connections, and channels.
pub struct CatalogClient {
    catalog_service: AsyncCatalogServiceClient<Client>,
    video_service: AsyncVideoServiceClient<Client>,
    connection_service: AsyncConnectionServiceClient<Client>,
    data_source_service: AsyncDataSourceServiceClient<Client>,
    channel_metadata_service: AsyncChannelMetadataServiceClient<Client>,
    token: BearerToken,
    workspace_rid: Option<String>,
    app_base_url: String,
}

impl CatalogClient {
    pub(crate) fn new(
        client: Client,
        runtime: &Arc<ConjureRuntime>,
        token: BearerToken,
        workspace_rid: Option<String>,
        app_base_url: String,
    ) -> Self {
        Self {
            catalog_service: AsyncCatalogServiceClient::new(client.clone(), runtime),
            video_service: AsyncVideoServiceClient::new(client.clone(), runtime),
            connection_service: AsyncConnectionServiceClient::new(client.clone(), runtime),
            data_source_service: AsyncDataSourceServiceClient::new(client.clone(), runtime),
            channel_metadata_service: AsyncChannelMetadataServiceClient::new(client, runtime),
            token,
            workspace_rid,
            app_base_url,
        }
    }

    // ── Dataset operations ───────────────────────────────────────────────────

    /// Create a new dataset.
    pub async fn create_dataset(&self, create: DatasetCreate) -> Result<Dataset> {
        let request = create.into_request(self.workspace_rid.as_deref())?;
        let response = self
            .catalog_service
            .create_dataset(&self.token, &request)
            .await
            .map_err(Error::from)?;
        Ok(Dataset::from_conjure(response, &self.app_base_url))
    }

    /// Get a dataset by RID.
    pub async fn get_dataset(&self, rid: &str) -> Result<Dataset> {
        let parsed = parse_rid(rid)?;
        let request = GetDatasetsRequest::builder()
            .extend_dataset_rids([parsed])
            .build();
        let response = self
            .catalog_service
            .get_enriched_datasets(&self.token, &request)
            .await
            .map_err(Error::from)?;

        response
            .into_iter()
            .next()
            .ok_or(Error::NotFound {
                resource: "dataset with given RID",
            })
            .map(|d| Dataset::from_conjure(d, &self.app_base_url))
    }

    /// Get multiple datasets by RID.
    ///
    /// Returns a map from RID string to Dataset. RIDs not found in Nominal are omitted.
    pub async fn get_dataset_batch<I, S>(&self, rids: I) -> Result<HashMap<String, Dataset>>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let rid_set = rids
            .into_iter()
            .map(|s| parse_rid(s.as_ref()).map_err(Error::from))
            .collect::<Result<BTreeSet<_>>>()?;
        let request = GetDatasetsRequest::builder().dataset_rids(rid_set).build();
        let response = self
            .catalog_service
            .get_enriched_datasets(&self.token, &request)
            .await
            .map_err(Error::from)?;
        Ok(response
            .into_iter()
            .map(|d| {
                let rid = rid_to_string(d.rid());
                (rid, Dataset::from_conjure(d, &self.app_base_url))
            })
            .collect())
    }

    fn search_datasets_stream(&self, query: DatasetQuery) -> impl Stream<Item = Result<Dataset>> {
        let conjure_query = query.into_conjure();
        let service = self.catalog_service.clone();
        let token = self.token.clone();
        let app_base_url = self.app_base_url.clone();
        paginate_stream(
            move |page_token| {
                SearchDatasetsRequest::builder()
                    .query(conjure_query.clone())
                    .sort_options(
                        DatasetSortOptions::builder()
                            .is_descending(true)
                            .field(DatasetSortField::IngestDate)
                            .build(),
                    )
                    .token(page_token)
                    .build()
            },
            move |req| {
                let service = service.clone();
                let token = token.clone();
                async move {
                    service
                        .search_datasets(&token, &req)
                        .await
                        .map_err(Error::from)
                }
            },
            |resp: &SearchDatasetsResponse| resp.next_page_token().cloned(),
            move |resp| {
                resp.results()
                    .iter()
                    .map(|d| Dataset::from_conjure(d.clone(), &app_base_url))
                    .collect()
            },
        )
    }

    /// List datasets, sorted by ingest date descending.
    pub async fn list_datasets(&self) -> Result<Vec<Dataset>> {
        self.search_datasets_stream(DatasetQuery::search_text(""))
            .try_collect()
            .await
    }

    /// Search datasets with a query, collecting all pages eagerly.
    ///
    /// # Example
    /// ```no_run
    /// # async fn example(client: nominal::NominalClient) -> nominal::Result<()> {
    /// use nominal::DatasetQuery;
    /// let datasets = client.catalog()
    ///     .search_datasets(DatasetQuery::and([
    ///         DatasetQuery::label("production"),
    ///         DatasetQuery::property("vehicle", "rocket"),
    ///     ]))
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub async fn search_datasets(&self, query: DatasetQuery) -> Result<Vec<Dataset>> {
        self.search_datasets_stream(query).try_collect().await
    }

    /// Update dataset metadata. Returns the updated dataset.
    ///
    /// Only fields set on the update will be changed; the rest remain untouched.
    pub async fn update_dataset(&self, rid: &str, update: DatasetUpdate) -> Result<Dataset> {
        let request = update.into_request();
        let dataset_rid = parse_rid(rid)?;
        let response = self
            .catalog_service
            .update_dataset_metadata(&self.token, &dataset_rid, &request)
            .await
            .map_err(Error::from)?;
        Ok(Dataset::from_conjure(response, &self.app_base_url))
    }

    /// Archive a dataset. Archived datasets are hidden from the UI but not deleted.
    pub async fn archive_dataset(&self, rid: &str) -> Result<()> {
        let dataset_rid = parse_rid(rid)?;
        self.catalog_service
            .archive_dataset(&self.token, &dataset_rid)
            .await
            .map_err(Error::from)?;
        Ok(())
    }

    /// Unarchive a dataset, restoring its visibility in the UI.
    pub async fn unarchive_dataset(&self, rid: &str) -> Result<()> {
        let dataset_rid = parse_rid(rid)?;
        self.catalog_service
            .unarchive_dataset(&self.token, &dataset_rid)
            .await
            .map_err(Error::from)?;
        Ok(())
    }

    // ── Video operations ─────────────────────────────────────────────────────

    /// Create a new video.
    pub async fn create_video(&self, create: VideoCreate) -> Result<Video> {
        let request = create.into_request(self.workspace_rid.as_deref())?;
        let response = self
            .video_service
            .create(&self.token, &request)
            .await
            .map_err(Error::from)?;
        Ok(Video::from_conjure(response, &self.app_base_url))
    }

    /// Get a video by RID.
    pub async fn get_video(&self, rid: &str) -> Result<Video> {
        let video_rid = parse_rid::<VideoRid>(rid)?;
        let response = self
            .video_service
            .get(&self.token, &video_rid)
            .await
            .map_err(Error::from)?;
        Ok(Video::from_conjure(response, &self.app_base_url))
    }

    /// Get multiple videos by RID.
    ///
    /// Returns a map from RID string to Video. RIDs not found in Nominal are omitted.
    pub async fn get_video_batch<I, S>(&self, rids: I) -> Result<HashMap<String, Video>>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let rid_set = rids
            .into_iter()
            .map(|s| parse_rid::<VideoRid>(s.as_ref()).map_err(Error::from))
            .collect::<Result<BTreeSet<_>>>()?;
        let request = GetVideosRequest::builder().video_rids(rid_set).build();
        let response = self
            .video_service
            .batch_get(&self.token, &request)
            .await
            .map_err(Error::from)?;
        Ok(response
            .responses()
            .iter()
            .map(|v| {
                let rid = rid_to_string(v.rid());
                (rid, Video::from_conjure(v.clone(), &self.app_base_url))
            })
            .collect())
    }

    fn search_videos_stream(&self, query: VideoQuery) -> impl Stream<Item = Result<Video>> {
        let conjure_query = query.into_conjure();
        let service = self.video_service.clone();
        let token = self.token.clone();
        let app_base_url = self.app_base_url.clone();
        paginate_stream(
            move |page_token| {
                SearchVideosRequest::builder()
                    .query(conjure_query.clone())
                    .sort_options(
                        VideoSortOptions::builder()
                            .is_descending(true)
                            .field(VideoSortField::CreatedAt)
                            .build(),
                    )
                    .token(page_token)
                    .build()
            },
            move |req| {
                let service = service.clone();
                let token = token.clone();
                async move { service.search(&token, &req).await.map_err(Error::from) }
            },
            |resp: &SearchVideosResponse| resp.next_page_token().cloned(),
            move |resp| {
                resp.results()
                    .iter()
                    .map(|v| Video::from_conjure(v.clone(), &app_base_url))
                    .collect()
            },
        )
    }

    /// List videos, sorted by creation date descending.
    pub async fn list_videos(&self) -> Result<Vec<Video>> {
        self.search_videos_stream(VideoQuery::search_text(""))
            .try_collect()
            .await
    }

    /// Search videos with a query, collecting all pages eagerly.
    ///
    /// # Example
    /// ```no_run
    /// # async fn example(client: nominal::NominalClient) -> nominal::Result<()> {
    /// use nominal::VideoQuery;
    /// let videos = client.catalog()
    ///     .search_videos(VideoQuery::and([
    ///         VideoQuery::label("flight"),
    ///         VideoQuery::property("vehicle", "rocket"),
    ///     ]))
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub async fn search_videos(&self, query: VideoQuery) -> Result<Vec<Video>> {
        self.search_videos_stream(query).try_collect().await
    }

    /// Update video metadata. Returns the updated video.
    ///
    /// Only fields set on the update will be changed; the rest remain untouched.
    pub async fn update_video(&self, rid: &str, update: VideoUpdate) -> Result<Video> {
        let request = update.into_request();
        let video_rid = parse_rid::<VideoRid>(rid)?;
        let response = self
            .video_service
            .update_metadata(&self.token, &video_rid, &request)
            .await
            .map_err(Error::from)?;
        Ok(Video::from_conjure(response, &self.app_base_url))
    }

    /// Archive a video. Archived videos are hidden from the UI but not deleted.
    pub async fn archive_video(&self, rid: &str) -> Result<()> {
        let video_rid = parse_rid::<VideoRid>(rid)?;
        self.video_service
            .archive(&self.token, &video_rid)
            .await
            .map_err(Error::from)?;
        Ok(())
    }

    /// Unarchive a video, restoring its visibility in the UI.
    pub async fn unarchive_video(&self, rid: &str) -> Result<()> {
        let video_rid = parse_rid::<VideoRid>(rid)?;
        self.video_service
            .unarchive(&self.token, &video_rid)
            .await
            .map_err(Error::from)?;
        Ok(())
    }

    // ── Connection operations ────────────────────────────────────────────────

    /// Get a connection by RID.
    pub async fn get_connection(&self, rid: &str) -> Result<Connection> {
        let connection_rid = parse_rid::<ConnectionRid>(rid)?;
        let response = self
            .connection_service
            .get_connection(&self.token, &connection_rid)
            .await
            .map_err(Error::from)?;
        Ok(Connection::from_conjure(response))
    }

    /// Get multiple connections by RID.
    ///
    /// Returns a map from RID string to Connection. RIDs not found in Nominal are omitted.
    pub async fn get_connection_batch<I, S>(&self, rids: I) -> Result<HashMap<String, Connection>>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let rid_set = rids
            .into_iter()
            .map(|s| parse_rid::<ConnectionRid>(s.as_ref()).map_err(Error::from))
            .collect::<Result<BTreeSet<_>>>()?;
        let response = self
            .connection_service
            .get_connections(&self.token, &rid_set)
            .await
            .map_err(Error::from)?;
        Ok(response
            .into_iter()
            .map(|c| {
                let rid = rid_to_string(c.rid());
                (rid, Connection::from_conjure(c))
            })
            .collect())
    }

    fn list_connections_stream(&self) -> impl Stream<Item = Result<Connection>> {
        let service = self.connection_service.clone();
        let token = self.token.clone();
        paginate_stream(
            |page_token| page_token,
            move |page_token| {
                let service = service.clone();
                let token = token.clone();
                async move {
                    service
                        .list_connections_v2(
                            &token,
                            None,
                            &BTreeSet::new(),
                            Some(100),
                            page_token.as_ref(),
                        )
                        .await
                        .map_err(Error::from)
                }
            },
            |resp: &ListConnectionsResponse| resp.next_page_token().cloned(),
            |resp| {
                resp.connections()
                    .iter()
                    .map(|c| Connection::from_conjure(c.clone()))
                    .collect()
            },
        )
    }

    /// List all connections.
    pub async fn list_connections(&self) -> Result<Vec<Connection>> {
        self.list_connections_stream().try_collect().await
    }

    /// Update connection metadata. Returns the updated connection.
    ///
    /// Only fields set on the update will be changed; the rest remain untouched.
    pub async fn update_connection(
        &self,
        rid: &str,
        update: ConnectionUpdate,
    ) -> Result<Connection> {
        let request = update.into_request();
        let connection_rid = parse_rid::<ConnectionRid>(rid)?;
        let response = self
            .connection_service
            .update_connection(&self.token, &connection_rid, &request)
            .await
            .map_err(Error::from)?;
        Ok(Connection::from_conjure(response))
    }

    /// Archive a connection. Archived connections are hidden from the UI but not deleted.
    pub async fn archive_connection(&self, rid: &str) -> Result<()> {
        let connection_rid = parse_rid::<ConnectionRid>(rid)?;
        self.connection_service
            .archive_connection(&self.token, &connection_rid)
            .await
            .map_err(Error::from)?;
        Ok(())
    }

    /// Unarchive a connection, restoring its visibility in the UI.
    pub async fn unarchive_connection(&self, rid: &str) -> Result<()> {
        let connection_rid = parse_rid::<ConnectionRid>(rid)?;
        self.connection_service
            .unarchive_connection(&self.token, &connection_rid)
            .await
            .map_err(Error::from)?;
        Ok(())
    }

    // ── Channel operations ───────────────────────────────────────────────────

    fn search_channels_stream(
        &self,
        query: ChannelQuery,
    ) -> Result<impl Stream<Item = Result<Channel>> + use<>> {
        let parts = query.into_parts()?;
        let service = self.data_source_service.clone();
        let token = self.token.clone();
        Ok(paginate_stream(
            move |page_token| {
                let mut b = SearchChannelsRequest::builder()
                    .fuzzy_search_text(parts.fuzzy_text.clone())
                    .data_sources(parts.data_source_rids.clone())
                    .data_types(parts.data_types.clone())
                    .exact_match(parts.exact_matches.clone());
                if let Some(t) = page_token {
                    b = b.next_page_token(t);
                }
                b.build()
            },
            move |req| {
                let service = service.clone();
                let token = token.clone();
                async move {
                    service
                        .search_channels(&token, &req)
                        .await
                        .map_err(Error::from)
                }
            },
            |resp: &SearchChannelsResponse| resp.next_page_token().cloned(),
            |resp| {
                resp.results()
                    .iter()
                    .cloned()
                    .map(Channel::from_search)
                    .collect()
            },
        ))
    }

    /// List every channel on a data source.
    ///
    /// `data_source_rid` can be any data source (dataset, video, connection, etc.).
    /// Paginates internally and returns all results.
    pub async fn list_channels(&self, data_source_rid: &str) -> Result<Vec<Channel>> {
        self.search_channels(ChannelQuery::new().data_source(data_source_rid))
            .await
    }

    /// Search channels with a query, collecting all pages eagerly.
    ///
    /// Accepts any data source RID — datasets, videos, connections.
    ///
    /// # Example
    /// ```no_run
    /// # async fn example(client: nominal::core::NominalClient) -> nominal::Result<()> {
    /// use nominal::core::ChannelQuery;
    /// let channels = client.catalog()
    ///     .search_channels(
    ///         ChannelQuery::new()
    ///             .fuzzy_text("temperature")
    ///             .data_source("ri.catalog.gov-staging.dataset.abc"),
    ///     )
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub async fn search_channels(&self, query: ChannelQuery) -> Result<Vec<Channel>> {
        self.search_channels_stream(query)?.try_collect().await
    }

    /// Get a single channel's metadata.
    pub async fn get_channel(&self, data_source_rid: &str, name: &str) -> Result<Channel> {
        let id = ChannelIdentifier::new(
            nominal_api::objects::api::Channel(name.to_string()),
            parse_rid::<DataSourceRid>(data_source_rid)?,
        );
        let request = GetChannelMetadataRequest::new(id);
        let response = self
            .channel_metadata_service
            .get_channel_metadata(&self.token, &request)
            .await
            .map_err(Error::from)?;
        Ok(Channel::from_stored(response))
    }

    /// Set a channel's metadata (description and/or unit). Only fields set
    /// on the update are written; the rest remain untouched. Returns the
    /// resulting channel.
    pub async fn set_channel_metadata(
        &self,
        data_source_rid: &str,
        name: &str,
        update: ChannelUpdate,
    ) -> Result<Channel> {
        let request = update.into_request(data_source_rid, name)?;
        let response = self
            .channel_metadata_service
            .update_channel_metadata(&self.token, &request)
            .await
            .map_err(Error::from)?;
        Ok(Channel::from_stored(response))
    }
}
