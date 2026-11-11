//! Run with NOMINAL_TOKEN, NOMINAL_BASE_URL and NOMINAL_DATASET_RID set.
//! The base URL must route native gRPC; there is no HTTP/JSON or Twirp equivalent.
use std::error::Error;

use nominal::NominalClient;
use nominal::channel_writer::{Timestamp, proto};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let token = std::env::var("NOMINAL_TOKEN")?;
    let base_url = std::env::var("NOMINAL_BASE_URL")?;
    let dataset_rid = std::env::var("NOMINAL_DATASET_RID")?;
    let client = NominalClient::builder(token).base_url(base_url).build()?;

    // The constructor waits for SchemasDeclared before returning a usable stream.
    let mut stream = client
        .channel_writer()
        .write_stream(vec![proto::Schema {
            schema_id: 1,
            dataset_rid: dataset_rid.clone(),
            channel: "rust_write_stream_smoke".into(),
            r#type: proto::ChannelType::Double as i32,
            ..Default::default()
        }])
        .await?;
    println!("Schema acknowledged");

    let seconds = chrono::Utc::now().timestamp();
    stream
        .write_chunk(vec![proto::ChannelData {
            schema_id: 1,
            timestamps: (0..3)
                .map(|offset| Timestamp {
                    seconds: seconds + offset,
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
    println!(
        "Chunk acknowledged: dataset={dataset_rid}, channel=rust_write_stream_smoke, seconds={seconds}, values=[42.125, 43.25, 44.5]"
    );

    stream.finish().await?;
    println!("Stream drained and closed");
    Ok(())
}
