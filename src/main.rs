mod audio;
mod config;
mod daemon;
mod hotkey;
mod model;
mod output;
mod state;
mod transcribe;
mod ui;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "typingsucks", about = "Hold-to-talk voice transcription")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the background daemon (headless, no GUI)
    Daemon,
    /// Stop the running daemon
    Stop,
    /// Check if daemon is running
    Status,
    /// Print current config
    Config,
    /// Manage whisper models
    Model {
        #[command(subcommand)]
        action: ModelCommands,
    },
}

#[derive(Subcommand)]
enum ModelCommands {
    /// Download a model (tiny/base/small/medium/large-v3-turbo)
    Download {
        #[arg(default_value = "base")]
        size: String,
    },
    /// List available and downloaded models
    List,
    /// Show currently active model
    Active,
    /// Scan for existing whisper models on disk and import them
    Scan {
        /// Auto-import all found models without asking
        #[arg(long)]
        yes: bool,
    },
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "typingsucks=info".into()),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        // No subcommand → launch GUI
        None => {
            let cfg = config::Config::load()?;
            ui::settings::run_settings_gui(cfg)?;
        }
        Some(Commands::Daemon) => daemon::run()?,
        Some(Commands::Stop) => daemon::stop()?,
        Some(Commands::Status) => daemon::status()?,
        Some(Commands::Config) => {
            let cfg = config::Config::load()?;
            let path = config::config_path();
            println!("Config: {}", path.display());
            println!("{}", toml::to_string_pretty(&cfg)?);
        }
        Some(Commands::Model { action }) => match action {
            ModelCommands::Download { size } => model::download_model(&size)?,
            ModelCommands::List => model::list_models()?,
            ModelCommands::Active => {
                let cfg = config::Config::load()?;
                let path = model::resolve_model_path(&cfg)?;
                println!("Active model: {}", path.display());
            }
            ModelCommands::Scan { yes } => {
                let found = model::scan_for_models();
                if found.is_empty() {
                    println!("No new models found on disk.");
                } else {
                    println!("Found {} model(s):", found.len());
                    for m in &found {
                        println!("  {} ({}MB) — {}", m.name, m.size_mb, m.path.display());
                    }
                    if yes {
                        for m in &found {
                            match model::import_model(&m.path, &m.name) {
                                Ok(()) => println!("  ✓ Linked {}", m.name),
                                Err(e) => println!("  ✗ Failed {}: {}", m.name, e),
                            }
                        }
                    } else {
                        println!("\nRun with --yes to import all, or use the GUI.");
                    }
                }
            }
        },
    }

    Ok(())
}
