use clap::Args;

#[derive(Args, Default)]
pub struct OutputArgs {
    #[arg(long)]
    pub json: bool,
}
#[derive(Args, Default)]
pub struct WaitArgs {
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..), conflicts_with="no_wait")]
    pub timeout: Option<u64>,
    #[arg(long)]
    pub no_wait: bool,
}
impl WaitArgs {
    pub fn options(&self) -> nominal::core::WaitOptions {
        wait_options(self.timeout)
    }
}
pub fn wait_options(timeout: Option<u64>) -> nominal::core::WaitOptions {
    let options = nominal::core::WaitOptions::default();
    match timeout {
        Some(seconds) => options.timeout(std::time::Duration::from_secs(seconds)),
        None => options,
    }
}
