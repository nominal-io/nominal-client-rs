use super::super::filetype::TabularFormat;
use super::items::*;
use nominal_api::tonic::nominal::ingest::v2 as p;
use std::collections::BTreeMap;

fn source(upload: &PendingUpload, locations: &BTreeMap<usize, String>) -> p::IngestSource {
    p::IngestSource {
        source: Some(p::ingest_source::Source::S3(p::S3IngestSource {
            path: locations[&upload.id].clone(),
        })),
    }
}
pub(super) fn encode(item: &PendingItem, locations: &BTreeMap<usize, String>) -> p::IngestItem {
    use p::ingest_item::Item;
    let (item, tags) = match item {
        PendingItem::Tabular {
            file,
            options,
            format,
        } => {
            let wide = p::WideFormat {
                tag_columns: options.tag_columns.clone().into_iter().collect(),
                exclude_columns: vec![],
            };
            let ingest = match format {
                TabularFormat::Csv => p::file_ingest_options::Ingest::Csv(p::CsvIngestOptions {
                    format: Some(p::CsvFormat {
                        format: Some(p::csv_format::Format::Wide(wide)),
                    }),
                }),
                TabularFormat::Parquet { archive } => {
                    p::file_ingest_options::Ingest::Parquet(p::ParquetIngestOptions {
                        format: Some(p::ParquetFormat {
                            format: Some(p::parquet_format::Format::Wide(wide)),
                        }),
                        is_archive: *archive,
                    })
                }
            };
            (
                Item::File(p::FileIngestItem {
                    source: Some(source(file, locations)),
                    ingest: Some(p::FileIngestOptions {
                        timestamp_metadata: Some(options.timestamp.to_ingest_proto()),
                        units: options.units.clone().into_iter().collect(),
                        channel_prefix: options.channel_prefix.clone(),
                        channel_name_overrides: options
                            .channel_name_overrides
                            .clone()
                            .into_iter()
                            .collect(),
                        ingest: Some(ingest),
                    }),
                }),
                &options.tags,
            )
        }
        PendingItem::Avro { file, options } => (
            Item::File(p::FileIngestItem {
                source: Some(source(file, locations)),
                ingest: Some(p::FileIngestOptions {
                    timestamp_metadata: Some(options.timestamp.timestamp().to_ingest_proto()),
                    units: options.units.clone().into_iter().collect(),
                    channel_prefix: options.channel_prefix.clone(),
                    channel_name_overrides: Default::default(),
                    ingest: Some(p::file_ingest_options::Ingest::Avro(
                        p::AvroIngestOptions {},
                    )),
                }),
            }),
            &options.tags,
        ),
        PendingItem::Mcap { file, options } => {
            let selection = match &options.topics {
                Topics::All => None,
                Topics::Include(topics) => Some(
                    p::mcap_channel_selection::Selection::IncludeTopics(p::McapTopicNames {
                        topics: topics.clone(),
                    }),
                ),
                Topics::Exclude(topics) => Some(
                    p::mcap_channel_selection::Selection::ExcludeTopics(p::McapTopicNames {
                        topics: topics.clone(),
                    }),
                ),
            };
            (
                Item::Mcap(p::McapIngestItem {
                    source: Some(source(file, locations)),
                    channels: selection.map(|selection| p::McapChannelSelection {
                        selection: Some(selection),
                    }),
                    ignore_invalid_topics: options.ignore_invalid_topics,
                }),
                &options.tags,
            )
        }
        PendingItem::Journal { file, options } => (
            Item::Log(p::LogIngestItem {
                source: Some(source(file, locations)),
                channel: options.channel.clone(),
                timestamp_metadata: options.timestamp.as_ref().map(|t| t.to_ingest_proto()),
                message_field: None,
            }),
            &options.tags,
        ),
        PendingItem::Dataflash { file, options } => (
            Item::Dataflash(p::DataflashIngestItem {
                source: Some(source(file, locations)),
            }),
            &options.tags,
        ),
        PendingItem::Containerized { sources, options } => (
            Item::Containerized(p::ContainerizedIngestItem {
                extractor_rid: options.extractor_rid.clone(),
                sources: sources
                    .iter()
                    .map(|s| (s.name.clone(), source(s, locations)))
                    .collect(),
                arguments: options.arguments.clone().into_iter().collect(),
                timestamp_metadata: options.timestamp.as_ref().map(|t| t.to_ingest_proto()),
            }),
            &options.tags,
        ),
        PendingItem::Video {
            file,
            timing,
            channel,
            tags,
        } => {
            let manifest = match timing {
                PendingVideoTiming::Frames(sidecar) => {
                    p::video_timestamp_manifest::Manifest::TimestampManifestFiles(
                        p::TimestampManifestFiles {
                            sources: vec![source(sidecar, locations)],
                        },
                    )
                }
                PendingVideoTiming::Start(time) => {
                    p::video_timestamp_manifest::Manifest::NoManifest(p::NoTimestampManifest {
                        starting_timestamp: Some(nominal_api::tonic::google::protobuf::Timestamp {
                            seconds: time.timestamp(),
                            nanos: time.timestamp_subsec_nanos() as i32,
                        }),
                        scale_parameter: None,
                    })
                }
            };
            (
                Item::Video(p::VideoIngestItem {
                    source: Some(source(file, locations)),
                    ingest: Some(p::VideoIngestOptions {
                        channel: channel.clone(),
                        timestamp_manifest: Some(p::VideoTimestampManifest {
                            manifest: Some(manifest),
                        }),
                        overwrite_segments: None,
                    }),
                }),
                tags,
            )
        }
    };
    p::IngestItem {
        item: Some(item),
        tags: tags.clone().into_iter().collect(),
    }
}
