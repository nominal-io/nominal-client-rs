use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;
mod commands;
mod context;
mod output;
mod validate;
use commands::api::ApiArgs;
use commands::asset::AssetCommands;
use commands::channel::ChannelCommands;
use commands::config::ConfigCommands;
use commands::connection::ConnectionCommands;
use commands::dataset::DatasetCommands;
use commands::endpoint::EndpointCommands;
#[cfg(feature = "unstable")]
use commands::fs::FsCommands;
use commands::ingest::IngestCommands;
use commands::run::RunCommands;
use commands::user::UserCommands;
use commands::video::VideoCommands;

#[derive(Parser)]
#[command(name = "nomctl", version, about = "Nominal CLI")]
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
    /// File Store commands
    #[cfg(feature = "unstable")]
    Fs {
        #[command(subcommand)]
        fs_command: FsCommands,
    },
    /// Upload files and ingest them as datasets
    Ingest {
        #[command(subcommand)]
        ingest_command: Box<IngestCommands>,
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
    /// Print shell completions to stdout
    #[command(long_about = "\
Print a shell completion script for the given shell to stdout.

Pipe the output into the location your shell loads completions from:

  bash:
    nomctl completions bash | sudo tee /etc/bash_completion.d/nomctl

  zsh (ensure a writable dir is on $fpath, e.g. ~/.zfunc):
    nomctl completions zsh > ~/.zfunc/_nomctl
    # in ~/.zshrc:   fpath+=(~/.zfunc); autoload -U compinit && compinit

  fish:
    nomctl completions fish > ~/.config/fish/completions/nomctl.fish

  powershell:
    nomctl completions powershell >> $PROFILE

  elvish:
    nomctl completions elvish >> ~/.config/elvish/rc.elv
")]
    Completions {
        /// Shell to generate completions for
        shell: Shell,
    },
    /// Print full --help for every command and subcommand in one stream
    HelpAll,
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
        Commands::Config { config_command } => commands::config::handle(config_command).await,
        Commands::Connection { connection_command } => {
            let client = commands::load_client(cli.profile.as_deref())?;
            commands::connection::handle(connection_command, client).await
        }
        Commands::Dataset { dataset_command } => {
            let client = commands::load_client(cli.profile.as_deref())?;
            commands::dataset::handle(dataset_command, client).await
        }
        Commands::Endpoint { endpoint_command } => commands::endpoint::handle(endpoint_command),
        #[cfg(feature = "unstable")]
        Commands::Fs { fs_command } => commands::fs::handle(fs_command).await,
        Commands::Ingest { ingest_command } => {
            let client = commands::load_client(cli.profile.as_deref())?;
            commands::ingest::handle(*ingest_command, client).await
        }
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
        Commands::Completions { shell } => {
            let mut cmd = Cli::command();
            let name = cmd.get_name().to_string();
            clap_complete::generate(shell, &mut cmd, name, &mut std::io::stdout());
            Ok(())
        }
        Commands::HelpAll => {
            let mut cmd = Cli::command();
            let name = cmd.get_name().to_string();
            render_help_recursive(&mut cmd, &name, 1, &mut std::io::stdout())?;
            Ok(())
        }
    }
}

fn render_help_recursive<W: std::io::Write>(
    cmd: &mut clap::Command,
    path: &str,
    depth: usize,
    out: &mut W,
) -> std::io::Result<()> {
    writeln!(out, "{} {path}", "#".repeat(depth))?;
    writeln!(out)?;
    writeln!(out, "{}", cmd.render_long_help())?;
    let sub_names: Vec<String> = cmd
        .get_subcommands()
        .filter(|c| !c.is_hide_set() && c.get_name() != "help")
        .map(|c| c.get_name().to_string())
        .collect();
    for name in sub_names {
        let child_path = format!("{path} {name}");
        if let Some(child) = cmd.find_subcommand_mut(&name) {
            render_help_recursive(child, &child_path, depth + 1, out)?;
        }
    }
    Ok(())
}
