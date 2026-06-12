pub(crate) mod asset;
pub(crate) mod catalog;
pub(crate) mod client;
pub(crate) mod datasource;
pub(crate) mod datetime;
pub(crate) mod ingest;
pub(crate) mod rid;
pub(crate) mod run;
pub(crate) mod template;
pub(crate) mod user;
pub(crate) mod utils;
pub(crate) mod workbook;
pub(crate) mod workspace;

pub use asset::{Asset, AssetCreate, AssetQuery, AssetUpdate, AssetsClient};
pub use catalog::{
    CatalogClient, Channel, ChannelDataType, ChannelQuery, ChannelUpdate, Connection,
    ConnectionUpdate, Dataset, DatasetCreate, DatasetQuery, DatasetUpdate, Video, VideoCreate,
    VideoQuery, VideoUpdate,
};
pub use client::{NominalClient, NominalClientBuilder};
pub use datasource::DataSource;
pub use ingest::{
    AvroStreamIngest, CsvIngest, DataflashIngest, DatasetTarget, FileType, IngestClient, IngestJob,
    IngestJobStatus, IngestType, JournalJsonIngest, McapIngest, ParquetIngest, ProgressCallback,
    TimeUnit, Timestamp, UploadEvent, UploadOptions, VideoIngest, VideoTarget,
};
// Re-exported from nominal_api so callers can name the types returned by
// `Template::layout` and `Template::content` without depending on nominal_api directly.
pub use nominal_api::objects::scout::layout::api::WorkbookLayout;
pub use nominal_api::objects::scout::workbookcommon::api::WorkbookContent;
pub use run::{Run, RunCreate, RunQuery, RunUpdate, RunsClient};
pub use template::{Template, TemplatesClient};
pub use user::{User, UsersClient};
pub use workbook::{Workbook, WorkbookCreate, WorkbookDataScope, WorkbookQuery, WorkbooksClient};
pub use workspace::WorkspacesClient;
