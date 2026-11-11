//! Bounded, paced A/B runner. Native gRPC, no compression or retries in either mode.
//! Required: NOMINAL_TOKEN, NOMINAL_BASE_URL, BENCH_DATASET_RID, BENCH_MODE=unary|stream.
//! Defaults: 1M points/s, 100 channels x 500 points/chunk, 20 seconds, concurrency 16.
//! Use separate freshly-created datasets and the SAME BENCH_TIMESTAMP_BASE for both modes.
use futures::{StreamExt, TryStreamExt};
use nominal::{
    NominalClient,
    channel_writer::{Timestamp, WriteStreamOptions, WriteStreamPoolOptions, proto},
};
use prost::Message;
use std::{
    collections::BTreeMap,
    error::Error,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
use tonic::{
    metadata::MetadataValue,
    transport::{Channel, ClientTlsConfig},
};

// Exact double-column subset of Scout 51f4b6ca4e3 v1 nominal_channel_writer.proto.
// This is an example-only codec, not a second public SDK writing API.
#[derive(Clone, PartialEq, Message)]
struct UnaryRequest {
    #[prost(string, tag = "1")]
    dataset: String,
    #[prost(message, repeated, tag = "2")]
    batches: Vec<UnaryBatch>,
}
#[derive(Clone, PartialEq, Message)]
struct UnaryBatch {
    #[prost(string, tag = "1")]
    channel: String,
    #[prost(btree_map = "string, string", tag = "2")]
    tags: BTreeMap<String, String>,
    #[prost(message, repeated, tag = "3")]
    timestamps: Vec<Timestamp>,
    #[prost(message, optional, tag = "4")]
    values: Option<UnaryValues>,
}
#[derive(Clone, PartialEq, Message)]
struct UnaryValues {
    #[prost(message, optional, tag = "1")]
    doubles: Option<proto::Doubles>,
}
#[derive(Clone, PartialEq, Message)]
struct UnaryResponse {}

struct Shape {
    channels: u64,
    samples: u64,
    rate: u64,
    timestamp_base: i64,
}
impl Shape {
    fn tags() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("bench".into(), "bandwidth".into()),
            ("rack".into(), "rack-01".into()),
        ])
    }
    fn name(channel: u64) -> String {
        format!("bandwidth.channel.{channel:05}")
    }
    fn timestamps(&self, batch: u64) -> Vec<Timestamp> {
        let samples_per_second = self.rate / self.channels;
        (0..self.samples)
            .map(|i| {
                let sample = batch * self.samples + i;
                Timestamp {
                    seconds: self.timestamp_base + (sample / samples_per_second) as i64,
                    nanos: ((sample % samples_per_second) * 1_000_000_000 / samples_per_second)
                        as i32,
                }
            })
            .collect()
    }
    fn doubles(&self, batch: u64, channel: u64) -> proto::Doubles {
        proto::Doubles {
            values: (0..self.samples)
                .map(|i| ((batch * self.samples + i + channel * 31) % 10007) as f64 / 100.0)
                .collect(),
        }
    }
    fn schemas(&self, dataset: &str) -> Vec<proto::Schema> {
        (0..self.channels)
            .map(|c| proto::Schema {
                schema_id: c as u32 + 1,
                dataset_rid: dataset.into(),
                channel: Self::name(c),
                tags: Self::tags().into_iter().collect(),
                r#type: proto::ChannelType::Double as i32,
                ..Default::default()
            })
            .collect()
    }
    fn unary(&self, dataset: &str, batch: u64) -> UnaryRequest {
        let timestamps = self.timestamps(batch);
        UnaryRequest {
            dataset: dataset.into(),
            batches: (0..self.channels)
                .map(|c| UnaryBatch {
                    channel: Self::name(c),
                    tags: Self::tags(),
                    timestamps: timestamps.clone(),
                    values: Some(UnaryValues {
                        doubles: Some(self.doubles(batch, c)),
                    }),
                })
                .collect(),
        }
    }
    fn streaming(&self, batch: u64) -> Vec<proto::ChannelData> {
        let timestamps = self.timestamps(batch);
        (0..self.channels)
            .map(|c| proto::ChannelData {
                schema_id: c as u32 + 1,
                timestamps: timestamps.clone(),
                data: Some(proto::channel_data::Data::Values(proto::Values {
                    values: Some(proto::values::Values::Doubles(self.doubles(batch, c))),
                })),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_protocols_contain_identical_points_and_channel_metadata() {
        let shape = Shape {
            channels: 2,
            samples: 3,
            rate: 6,
            timestamp_base: 1234,
        };
        let unary = shape.unary("dataset", 2);
        let schemas = shape.schemas("dataset");
        let streamed = shape.streaming(2);
        for ((batch, schema), data) in unary.batches.iter().zip(&schemas).zip(&streamed) {
            assert_eq!(batch.channel, schema.channel);
            assert_eq!(batch.tags, schema.tags.clone().into_iter().collect());
            assert_eq!(schema.dataset_rid, unary.dataset);
            assert_eq!(schema.schema_id, data.schema_id);
            assert_eq!(batch.timestamps, data.timestamps);
            let Some(proto::channel_data::Data::Values(values)) = &data.data else {
                panic!("expected numeric values")
            };
            let Some(proto::values::Values::Doubles(doubles)) = &values.values else {
                panic!("expected doubles")
            };
            assert_eq!(
                batch.values.as_ref().unwrap().doubles.as_ref().unwrap(),
                doubles
            );
        }
        assert_eq!(
            UnaryRequest::decode(unary.encode_to_vec().as_slice()).unwrap(),
            unary
        );
    }

    #[test]
    fn timestamps_are_distinct_and_continue_across_batches() {
        let shape = Shape {
            channels: 100,
            samples: 500,
            rate: 1_000_000,
            timestamp_base: 1234,
        };
        let first = shape.timestamps(0);
        let next = shape.timestamps(1);
        assert_eq!(
            first[0],
            Timestamp {
                seconds: 1234,
                nanos: 0
            }
        );
        assert_eq!(
            first[499],
            Timestamp {
                seconds: 1234,
                nanos: 49_900_000
            }
        );
        assert_eq!(
            next[0],
            Timestamp {
                seconds: 1234,
                nanos: 50_000_000
            }
        );
    }
}

fn setting(name: &str, default: u64) -> Result<u64, Box<dyn Error>> {
    Ok(std::env::var(name)
        .ok()
        .map(|v| v.parse())
        .transpose()?
        .unwrap_or(default))
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let token = std::env::var("NOMINAL_TOKEN")?;
    let base = std::env::var("NOMINAL_BASE_URL")?;
    let dataset = std::env::var("BENCH_DATASET_RID")?;
    let mode = std::env::var("BENCH_MODE")?;
    if !matches!(mode.as_str(), "unary" | "stream") {
        return Err("BENCH_MODE must be unary or stream".into());
    }
    let seconds = setting("BENCH_SECONDS", 20)?;
    let rate = setting("BENCH_RATE", 1_000_000)?;
    let shape = Shape {
        channels: setting("BENCH_CHANNELS", 100)?,
        samples: setting("BENCH_SAMPLES", 500)?,
        rate,
        timestamp_base: setting(
            "BENCH_TIMESTAMP_BASE",
            (chrono::Utc::now().timestamp() - 300) as u64,
        )? as i64,
    };
    if seconds == 0
        || seconds > 60
        || shape.channels == 0
        || shape.channels > 100
        || shape.samples == 0
        || shape.samples > 50_000
        || rate == 0
        || rate % shape.channels != 0
    {
        return Err("invalid bounded benchmark shape/rate/duration".into());
    }
    let points_per_batch = shape.channels * shape.samples;
    let batches = rate * seconds / points_per_batch;
    if batches == 0 || rate * seconds % points_per_batch != 0 {
        return Err("duration must contain an integer number of batches".into());
    }
    let concurrency = 16;
    let client = NominalClient::builder(token.clone())
        .base_url(&base)
        .build()?;
    let mut endpoint = Channel::from_shared(
        base.trim_end_matches("/api")
            .trim_end_matches('/')
            .to_owned(),
    )?;
    if base.starts_with("https:") {
        endpoint = endpoint.tls_config(ClientTlsConfig::new().with_native_roots())?;
    }
    let channel = if mode == "unary" {
        endpoint.connect().await?
    } else {
        endpoint.connect_lazy()
    };
    let auth: MetadataValue<_> = format!("Bearer {token}").parse()?;
    let pool = if mode == "stream" {
        Some(
            client
                .channel_writer()
                .write_stream_pool(
                    shape.schemas(&dataset),
                    WriteStreamPoolOptions {
                        stream_count: 4,
                        max_pending_messages: concurrency,
                        max_pending_bytes: 64 * 1024 * 1024,
                        stream_options: WriteStreamOptions {
                            max_in_flight_messages: 4,
                            max_retries: 0,
                            response_timeout: Duration::from_secs(15),
                            ..Default::default()
                        },
                    },
                )
                .await?,
        )
    } else {
        None
    };
    let acknowledged = AtomicU64::new(0);
    let max_lag_us = AtomicU64::new(0);
    let wall_start = chrono::Utc::now().timestamp_millis();
    let start = tokio::time::Instant::now();
    let writes=futures::stream::iter(0..batches).map(|batch| {
        let (shape,pool,auth,channel,dataset,acknowledged,max_lag_us)=(&shape,&pool,auth.clone(),channel.clone(),&dataset,&acknowledged,&max_lag_us);
        async move {
            let due=start+Duration::from_secs_f64(batch as f64 * points_per_batch as f64 / rate as f64);
            tokio::time::sleep_until(due).await;
            max_lag_us.fetch_max(tokio::time::Instant::now().saturating_duration_since(due).as_micros() as u64,Ordering::Relaxed);
            if let Some(pool)=pool {
                pool.write_chunk(shape.streaming(batch)).await.map_err(|e|e.to_string())?;
            } else {
                let mut grpc=tonic::client::Grpc::new(channel);
                grpc.ready().await.map_err(|e|e.to_string())?;
                let mut request=tonic::Request::new(shape.unary(dataset,batch));
                request.metadata_mut().insert("authorization",auth);
                let _:tonic::Response<UnaryResponse>=grpc.unary(request,http::uri::PathAndQuery::from_static("/nominal.channel_writer.v1.NominalChannelWriterService/WriteColumnBatches"),tonic::codec::ProstCodec::default()).await.map_err(|e|e.to_string())?;
            }
            acknowledged.fetch_add(points_per_batch,Ordering::Relaxed);
            Ok::<(),String>(())
        }
    }).buffer_unordered(concurrency).try_collect::<Vec<_>>();
    let result = tokio::time::timeout(Duration::from_secs(seconds + 30), writes).await;
    let elapsed = start.elapsed().as_secs_f64();
    let error = match result {
        Ok(Ok(_)) => None,
        Ok(Err(e)) => Some(e),
        Err(_) => Some("benchmark hard deadline exceeded".into()),
    };
    let drain_error = if let Some(pool) = pool {
        pool.finish().await.err().map(|e| e.to_string())
    } else {
        None
    };
    let total = acknowledged.load(Ordering::Relaxed);
    println!(
        "{}",
        serde_json::json!({"mode":mode,"dataset":dataset,"target_points_per_second":rate,"scheduled_seconds":seconds,"channels":shape.channels,"samples_per_channel":shape.samples,"points_per_batch":points_per_batch,"batches":batches,"acknowledged_points":total,"elapsed_including_ack_drain_seconds":elapsed,"acknowledged_points_per_second":total as f64/elapsed,"max_scheduling_lag_ms":max_lag_us.load(Ordering::Relaxed) as f64/1000.0,"start_ms":wall_start,"end_ms":chrono::Utc::now().timestamp_millis(),"timestamp_base":shape.timestamp_base,"compression":"none","retries":0,"error":error,"drain_error":drain_error})
    );
    if error.is_some() || drain_error.is_some() {
        return Err("benchmark failed; see JSON result".into());
    }
    Ok(())
}
