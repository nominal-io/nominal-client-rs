use clap::{Parser, Subcommand};
mod commands;
use commands::asset::AssetCommands;
use commands::config::ConfigCommands;
use commands::user::UserCommands;

use nominal_client::{Config, NominalClient};

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
    let cli = Cli::parse();
    let config = Config::from_file(None).expect("Failed to load config");
    let profile = config.get_profile(&cli.profile).expect("Profile not found");
    let client = NominalClient::from_profile(profile).expect("Failed to create Nominal client");

    match cli.command {
        Commands::Asset { asset_command } => {
            commands::asset::handle(asset_command, client).await;
        }
        Commands::Config { config_command } => {
            // You may want to pass a config path here
            commands::config::handle(config_command, None);
        }
        Commands::User { user_command } => {
            commands::user::handle(user_command, client).await;
        }
    }
}
