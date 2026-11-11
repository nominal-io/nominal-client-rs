//! Run with NOMINAL_TOKEN, NOMINAL_BASE_URL and NOMINAL_DATASET_RID set.
//! Sends eight chunks through two streams with a four-message GLOBAL window.
use std::error::Error;

use futures::{StreamExt, TryStreamExt};
use nominal::NominalClient;
use nominal::channel_writer::{
    Timestamp, WriteStreamError, WriteStreamOptions, WriteStreamPoolOptions, proto,
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let token = std::env::var("NOMINAL_TOKEN")?;
    let base_url = std::env::var("NOMINAL_BASE_URL")?;
    let dataset_rid = std::env::var("NOMINAL_DATASET_RID")?;
    let client = NominalClient::builder(token).base_url(base_url).build()?;
    let pool = client
        .channel_writer()
        .write_stream_pool(
            vec![proto::Schema {
                schema_id: 1,
                dataset_rid: dataset_rid.clone(),
                channel: "rust_write_stream_pool_smoke".into(),
                r#type: proto::ChannelType::Double as i32,
                ..Default::default()
            }],
            WriteStreamPoolOptions {
                stream_count: 2,
                max_pending_messages: 4,
                max_pending_bytes: 64 * 1024 * 1024,
                stream_options: WriteStreamOptions {
                    max_in_flight_messages: 2,
                    ..Default::default()
                },
            },
        )
        .await?;
    println!("Schemas acknowledged on two streams; global window=4; per-stream window=2");

    let seconds = chrono::Utc::now().timestamp();
    futures::stream::iter(0..8)
        .map(|chunk| {
            let pool = &pool;
            async move {
                pool.write_chunk(vec![proto::ChannelData {
                    schema_id: 1,
                    timestamps: (0..3)
                        .map(|offset| Timestamp {
                            seconds: seconds + chunk * 3 + offset,
                            nanos: 0,
                        })
                        .collect(),
                    data: Some(proto::channel_data::Data::Values(proto::Values {
                        values: Some(proto::values::Values::Doubles(proto::Doubles {
                            values: vec![42.125, 43.25, 44.5],
                        })),
                    })),
                }])
                .await?;
                println!("Acknowledged chunk {chunk}");
                Ok::<(), WriteStreamError>(())
            }
        })
        .buffer_unordered(4)
        .try_collect::<Vec<_>>()
        .await?;
    pool.finish().await?;
    println!(
        "Pool drained: dataset={dataset_rid}, channel=rust_write_stream_pool_smoke, seconds={seconds}, chunks=8, points=24"
    );
    Ok(())
}
