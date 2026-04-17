use clap::{Parser, Subcommand};
mod commands;
use commands::api::ApiArgs;
use commands::asset::AssetCommands;
use commands::channel::ChannelCommands;
use commands::config::ConfigCommands;
use commands::connection::ConnectionCommands;
use commands::dataset::DatasetCommands;
use commands::endpoint::EndpointCommands;
use commands::run::RunCommands;
use commands::user::UserCommands;
use commands::video::VideoCommands;

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
    /// Channel management commands
    Channel {
        #[command(subcommand)]
        channel_command: ChannelCommands,
    },
    /// Config management commands
    Config {
        #[command(subcommand)]
        config_command: ConfigCommands,
    },
    /// Connection management commands
    Connection {
        #[command(subcommand)]
        connection_command: ConnectionCommands,
    },
    /// Dataset management commands
    Dataset {
        #[command(subcommand)]
        dataset_command: DatasetCommands,
    },
    /// Endpoint introspection
    Endpoint {
        #[command(subcommand)]
        endpoint_command: EndpointCommands,
    },
    /// Run management commands
    Run {
        #[command(subcommand)]
        run_command: RunCommands,
    },
    /// User management commands
    User {
        #[command(subcommand)]
        user_command: UserCommands,
    },
    /// Video management commands
    Video {
        #[command(subcommand)]
        video_command: VideoCommands,
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
            commands::api::handle(args, profile.base_url(), profile.token()).await
        }
        Commands::Asset { asset_command } => {
            let client = commands::load_client(cli.profile.as_deref())?;
            commands::asset::handle(asset_command, client).await
        }
        Commands::Channel { channel_command } => {
            let client = commands::load_client(cli.profile.as_deref())?;
            commands::channel::handle(channel_command, client).await
        }
        Commands::Config { config_command } => commands::config::handle(config_command),
        Commands::Connection { connection_command } => {
            let client = commands::load_client(cli.profile.as_deref())?;
            commands::connection::handle(connection_command, client).await
        }
        Commands::Dataset { dataset_command } => {
            let client = commands::load_client(cli.profile.as_deref())?;
            commands::dataset::handle(dataset_command, client).await
        }
        Commands::Endpoint { endpoint_command } => commands::endpoint::handle(endpoint_command),
        Commands::Run { run_command } => {
            let client = commands::load_client(cli.profile.as_deref())?;
            commands::run::handle(run_command, client).await
        }
        Commands::User { user_command } => {
            let client = commands::load_client(cli.profile.as_deref())?;
            commands::user::handle(user_command, client).await
        }
        Commands::Video { video_command } => {
            let client = commands::load_client(cli.profile.as_deref())?;
            commands::video::handle(video_command, client).await
        }
    }
}
