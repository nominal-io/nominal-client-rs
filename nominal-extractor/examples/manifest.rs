use nominal_extractor::{
    ExtractResult, ManifestContext, NumericTimeUnit, NumericTimestamp, TabularOutput, VideoOutput,
    VideoTiming, run_manifest,
};
fn extract(ctx: &mut ManifestContext) -> ExtractResult {
    let prefix = ctx.optional_param::<String>("PREFIX")?.unwrap_or_default();
    let output = ctx.output_dir().join("data.csv");
    std::fs::copy(ctx.input("DATA")?, &output)?;
    ctx.add_tabular(
        TabularOutput::new(output)
            .channel_prefix(prefix)
            .timestamp("ts", NumericTimestamp::Epoch(NumericTimeUnit::Nanoseconds)),
    )?;
    let video = match ctx.input("VIDEO") {
        Ok(input) => Some(input),
        Err(nominal_extractor::Error::Input(_)) => None,
        Err(error) => return Err(error.into()),
    };
    if let Some(input) = video {
        let start = ctx.param::<chrono::DateTime<chrono::Utc>>("VIDEO_START")?;
        let output = ctx
            .output_dir()
            .join(input.file_name().ok_or("VIDEO has no file name")?);
        std::fs::copy(input, &output)?;
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
    run_manifest(extract).map(|_| ())
}
