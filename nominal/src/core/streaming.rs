use std::path::PathBuf;

use conjure_object::ResourceIdentifier;
use nominal_streaming::stream::NominalDatasetStreamBuilder;

use crate::{Error, Result};

pub use nominal_streaming::stream::{
    NominalDatasetStream as DatasetStream, NominalDoubleArrayWriter as DoubleArrayWriter,
    NominalDoubleWriter as DoubleWriter, NominalIntegerWriter as IntegerWriter,
    NominalStreamOpts as StreamOptions, NominalStringArrayWriter as StringArrayWriter,
    NominalStringWriter as StringWriter, NominalStructWriter as StructWriter,
    NominalUint64Writer as Uint64Writer,
};
pub use nominal_streaming::types::{ChannelDescriptor, IntoTimestamp};

/// Client for opening streaming sessions into Nominal datasets.
#[derive(Clone, Debug)]
pub struct StreamingClient {
    token: conjure_object::BearerToken,
    base_url: String,
}

impl StreamingClient {
    pub(crate) fn new(token: conjure_object::BearerToken, base_url: String) -> Self {
        Self { token, base_url }
    }

    /// Open a stream to a dataset with default streaming options.
    pub fn open(&self, dataset_rid: impl AsRef<str>) -> Result<DatasetStream> {
        self.open_with_options(dataset_rid, DatasetStreamOptions::new())
    }

    /// Open a stream to a dataset with explicit streaming options.
    pub fn open_with_options(
        &self,
        dataset_rid: impl AsRef<str>,
        options: DatasetStreamOptions,
    ) -> Result<DatasetStream> {
        let dataset = ResourceIdentifier::new(dataset_rid.as_ref()).map_err(|err| Error::Rid {
            rid: dataset_rid.as_ref().to_string(),
            reason: err.to_string(),
        })?;
        options.build_stream(self.token.clone(), dataset, &self.base_url)
    }
}

/// Options for a dataset streaming session.
#[derive(Clone, Debug, Default)]
pub struct DatasetStreamOptions {
    stream_to_file: Option<PathBuf>,
    file_fallback: Option<PathBuf>,
    runtime_handle: Option<tokio::runtime::Handle>,
    stream_options: StreamOptions,
}

impl DatasetStreamOptions {
    pub fn new() -> Self {
        Self::default()
    }

    /// Also write streamed points to an Avro file.
    #[must_use]
    pub fn stream_to_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.stream_to_file = Some(path.into());
        self
    }

    /// Write failed stream requests to an Avro file.
    #[must_use]
    pub fn with_file_fallback(mut self, path: impl Into<PathBuf>) -> Self {
        self.file_fallback = Some(path.into());
        self
    }

    /// Use a specific Tokio runtime handle for streaming background requests.
    #[must_use]
    pub fn runtime_handle(mut self, handle: tokio::runtime::Handle) -> Self {
        self.runtime_handle = Some(handle);
        self
    }

    /// Override the underlying streaming buffer and dispatch options.
    #[must_use]
    pub fn stream_options(mut self, options: StreamOptions) -> Self {
        self.stream_options = options;
        self
    }

    fn build_stream(
        self,
        token: conjure_object::BearerToken,
        dataset: ResourceIdentifier,
        base_url: &str,
    ) -> Result<DatasetStream> {
        if self.stream_to_file.is_some() && self.file_fallback.is_some() {
            return Err(Error::Streaming {
                details: "choose either stream_to_file or with_file_fallback, not both".into(),
            });
        }
        let handle = match self.runtime_handle {
            Some(handle) => handle,
            None => tokio::runtime::Handle::try_current().map_err(|err| Error::Streaming {
                details: format!("opening a streaming dataset requires a Tokio runtime: {err}"),
            })?,
        };

        let mut stream_options = self.stream_options;
        // overrides the default API URL
        stream_options.base_api_url = base_url.to_string();

        let mut builder = NominalDatasetStreamBuilder::new()
            .stream_to_core(token, dataset, handle)
            .with_options(stream_options);

        if let Some(path) = self.stream_to_file {
            builder = builder.stream_to_file(path);
        }
        if let Some(path) = self.file_fallback {
            builder = builder.with_file_fallback(path);
        }

        Ok(builder.build())
    }
}
