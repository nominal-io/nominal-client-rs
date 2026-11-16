fn main() -> Result<(), Box<dyn std::error::Error>> {
    let protoc = protoc_bin_vendored::protoc_bin_path()?;
    let mut config = prost_build::Config::new();
    config.protoc_executable(protoc);
    tonic_build::configure().compile_protos_with_config(
        config,
        &["proto/channel_writer.proto"],
        &["proto"],
    )?;
    println!("cargo:rerun-if-changed=proto");
    Ok(())
}
