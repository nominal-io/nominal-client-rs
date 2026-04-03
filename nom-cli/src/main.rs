use clap::{Parser, Subcommand};
mod commands;
use commands::asset::AssetCommands;
use commands::config::ConfigCommands;
use commands::user::UserCommands;

#[derive(Parser)]
#[command(name = "nom")]
#[command(about = "Interact with Nominal", long_about = None)]
struct Cli {
    /// Named profile to use from config
    #[arg(short, long, default_value = "default")]
    profile: String,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Asset {
        #[command(subcommand)]
        asset_command: AssetCommands,
    },
    /// Config management commands
    Config {
        #[command(subcommand)]
        config_command: ConfigCommands,
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
        err.exit();
    }
}

async fn run() -> Result<(), clap::Error> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Config { config_command } => commands::config::handle(config_command, None),
        Commands::Asset { asset_command } => {
            let client = commands::load_client(&cli.profile)?;
            commands::asset::handle(asset_command, client).await
        }
        Commands::User { user_command } => {
            let client = commands::load_client(&cli.profile)?;
            commands::user::handle(user_command, client).await
        }
    }
}
