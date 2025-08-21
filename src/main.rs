use clap::{Parser, Subcommand};
mod commands;
use commands::config::ConfigCommands;
use commands::user::UserCommands;
mod client;
mod config;

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
    /// User management commands
    User {
        #[command(subcommand)]
        user_command: UserCommands,
    },
    /// Config management commands
    Config {
        #[command(subcommand)]
        config_command: ConfigCommands,
    },
}

// UserCommands is now defined in commands/user.rs

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let config = config::Config::from_file(None).expect("Failed to load config");
    let profile = config.get_profile(&cli.profile).expect("Profile not found");
    let client =
        client::NominalClient::from_profile(profile).expect("Failed to create Nominal client");

    match cli.command {
        Commands::User { user_command } => {
            commands::user::handle(user_command, client).await;
        }
        Commands::Config { config_command } => {
            // You may want to pass a config path here
            commands::config::handle(config_command, None);
        }
    }
}
