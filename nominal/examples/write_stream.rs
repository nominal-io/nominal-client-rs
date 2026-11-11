//! Run with NOMINAL_TOKEN, NOMINAL_BASE_URL and NOMINAL_DATASET_RID set.
//! Sends eight chunks through a four-message window on one native TLS gRPC stream.
use std::error::Error;

use nominal::NominalClient;
use nominal::channel_writer::{Timestamp, WriteStreamOptions, proto};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let token = std::env::var("NOMINAL_TOKEN")?;
    let base_url = std::env::var("NOMINAL_BASE_URL")?;
    let dataset_rid = std::env::var("NOMINAL_DATASET_RID")?;
    let client = NominalClient::builder(token).base_url(base_url).build()?;
    let stream = client
        .channel_writer()
        .write_stream_with_options(
            vec![proto::Schema {
                schema_id: 1,
                dataset_rid: dataset_rid.clone(),
                channel: "rust_write_stream_smoke".into(),
                r#type: proto::ChannelType::Double as i32,
                ..Default::default()
            }],
            WriteStreamOptions {
                max_in_flight_messages: 4,
                ..Default::default()
            },
        )
        .await?;
    println!("Schema acknowledged; max_in_flight_messages=4");

    // Admission waits for window capacity, not for this chunk's acknowledgment.
    let seconds = chrono::Utc::now().timestamp();
    let mut receipts = Vec::new();
    for chunk in 0..8 {
        let receipt = stream
            .enqueue_chunk(vec![proto::ChannelData {
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
        println!("Enqueued message {}", receipt.message_id());
        receipts.push(receipt);
    }
    // Responses are consumed in the background and may arrive out of order or batched.
    for receipt in receipts {
        let id = receipt.message_id();
        receipt.acknowledged().await?;
        println!("Acknowledged message {id}");
    }
    stream.finish().await?;
    println!(
        "Stream drained: dataset={dataset_rid}, channel=rust_write_stream_smoke, seconds={seconds}, chunks=8, points=24"
    );
    Ok(())
}
