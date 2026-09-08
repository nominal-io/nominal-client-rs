use nominal_extractor::{ExtractResult, SingleFileContext, run_single_file};
fn extract(ctx: &mut SingleFileContext) -> ExtractResult {
    let input = ctx.input("DATA")?;
    let output = ctx.output_dir().join("data.csv");
    std::fs::copy(input, &output)?;
    ctx.set_output(output)?;
    Ok(())
}
fn main() -> nominal_extractor::Result<()> {
    run_single_file(extract).map(|_| ())
}
