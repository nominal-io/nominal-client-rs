use clap::{Parser, Subcommand};
mod commands;
use commands::api::ApiArgs;
use commands::asset::AssetCommands;
use commands::config::ConfigCommands;
use commands::endpoint::EndpointCommands;
use commands::user::UserCommands;

#[derive(Parser)]
#[command(name = "nom")]
#[command(about = "Interact with Nominal", long_about = None)]
struct Cli {
    /// Named profile to use from config (overrides NOMINAL_PROFILE env var)
    #[arg(short, long)]
    profile: Option<String>,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Send a request to a REST or gRPC endpoint
    Api(ApiArgs),
    /// Asset management commands
    Asset {
        #[command(subcommand)]
        asset_command: AssetCommands,
    },
    /// Config management commands
    Config {
        #[command(subcommand)]
        config_command: ConfigCommands,
    },
    /// Endpoint introspection
    Endpoint {
        #[command(subcommand)]
        endpoint_command: EndpointCommands,
    },
    /// User management commands
    User {
        #[command(subcommand)]
        user_command: UserCommands,
    },
}

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

async fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Api(args) => {
            let profile = commands::load_profile(cli.profile.as_deref())?;
            commands::api::handle(args, profile).await
        }
        Commands::Asset { asset_command } => {
            let profile = commands::load_profile(cli.profile.as_deref())?;
            commands::asset::handle(asset_command, profile).await
        }
        Commands::Config { config_command } => commands::config::handle(config_command),
        Commands::Endpoint { endpoint_command } => commands::endpoint::handle(endpoint_command),
        Commands::User { user_command } => {
            let profile = commands::load_profile(cli.profile.as_deref())?;
            commands::user::handle(user_command, profile).await
        }
    }
}
