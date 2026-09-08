use nominal_extractor::{
    ExtractResult, ManifestContext, NumericTimeUnit, NumericTimestamp, TabularOutput, VideoOutput,
    VideoTiming, run_manifest,
};

fn extract(ctx: &mut ManifestContext) -> ExtractResult {
    // Register PREFIX as an optional parameter. An absent value leaves channel
    // names unchanged; a prefix keeps channels from different sources distinct.
    let prefix = ctx.optional_param::<String>("PREFIX")?.unwrap_or_default();

    // This example copies a CSV; replace the copy with your format conversion.
    // Write inside the output directory so Nominal can collect the result.
    let output = ctx.output_dir().join("data.csv");
    std::fs::copy(ctx.input("DATA")?, &output)?;

    // Declaring the file adds it to the manifest. Here the ts column contains
    // nanoseconds since the Unix epoch, overriding the image timestamp default.
    ctx.add_tabular(
        TabularOutput::new(output)
            .channel_prefix(prefix)
            .timestamp("ts", NumericTimestamp::Epoch(NumericTimeUnit::Nanoseconds)),
    )?;

    // VIDEO is an optional file input, not a scalar parameter. A missing input
    // leaves this run with CSV output only; other errors still fail the run.
    let video = match ctx.input("VIDEO") {
        Ok(input) => Some(input),
        Err(nominal_extractor::Error::Input(_)) => None,
        Err(error) => return Err(error.into()),
    };
    if let Some(input) = video {
        // The video needs a start time to align it with telemetry. Parse the
        // registered VIDEO_START parameter as an RFC3339 timestamp.
        let start = ctx.param::<chrono::DateTime<chrono::Utc>>("VIDEO_START")?;
        let output = ctx
            .output_dir()
            .join(input.file_name().ok_or("VIDEO has no file name")?);
        std::fs::copy(input, &output)?;

        // Start timing uses the video timing from the media file. No scaling
        // or per-frame timestamp file is needed for this example.
        ctx.add_video(VideoOutput::new(
            output,
            "camera",
            VideoTiming::Start {
                at: start,
                scale: None,
            },
        ))?;
    }
    Ok(())
}

fn main() -> nominal_extractor::Result<()> {
    // Write manifest.json only after all extraction steps succeed. Returning
    // errors from main lets Nominal detect a failed container run.
    run_manifest(extract).map(|_| ())
}
