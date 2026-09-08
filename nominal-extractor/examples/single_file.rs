use nominal_extractor::{ExtractResult, SingleFileContext, run_single_file};

// This example copies an existing CSV. Replace the copy with your parser or conversion.
fn extract(ctx: &mut SingleFileContext) -> ExtractResult {
    // DATA is the input environment name registered with the image. Nominal
    // supplies the local path; the extractor does not download the file itself.
    let input = ctx.input("DATA")?;

    // Nominal collects files from this directory. Finish writing before declaring output.
    let output = ctx.output_dir().join("data.csv");
    std::fs::copy(input, &output)?;

    // Single-file mode requires exactly one output. Its format and timestamp
    // settings come from the image registration.
    ctx.set_output(output)?;
    Ok(())
}

fn main() -> nominal_extractor::Result<()> {
    // The runner reads the environment and checks the output after extract succeeds.
    // Returning an error from main gives the container a nonzero exit status.
    run_single_file(extract).map(|_| ())
}
