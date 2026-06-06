use anyhow::Result;
use clap::Parser;
use clap::builder::styling::{AnsiColor, Effects, Styles};
use std::path::PathBuf;

mod config;
mod headless;
mod install_skill;
mod launcher;
mod prompt;
mod run;
mod run_dir;
mod tmux;

const STYLES: Styles = Styles::styled()
    .header(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .usage(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .literal(AnsiColor::Cyan.on_default().effects(Effects::BOLD))
    .placeholder(AnsiColor::Cyan.on_default());

#[derive(Parser)]
#[command(name = "agent-offload")]
#[command(version)]
#[command(about = "Launch coding agents and wait for completion")]
#[command(styles = STYLES)]
#[command(args_conflicts_with_subcommands = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    #[command(flatten)]
    run: RunArgs,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Launch a profile with a prompt and wait for its done file.
    Run(RunArgs),

    /// List configured profiles.
    Profiles(ConfigArgs),

    /// Install the bundled Claude Code skill.
    InstallSkill,
}

#[derive(clap::Args)]
struct RunArgs {
    /// Profile name from the user config.
    #[arg(short, long)]
    profile: Option<String>,

    /// Override the config file path.
    #[arg(long)]
    config: Option<PathBuf>,

    /// Run headlessly without tmux.
    #[arg(short = 'H', long)]
    headless: bool,

    /// Prompt text. If omitted, the prompt is read from stdin.
    #[arg(trailing_var_arg = true)]
    prompt: Vec<String>,
}

#[derive(clap::Args)]
struct ConfigArgs {
    /// Override the config file path.
    #[arg(long)]
    config: Option<PathBuf>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Run(args)) => run::run(args),
        Some(Commands::Profiles(args)) => run::profiles(args),
        Some(Commands::InstallSkill) => install_skill::run(),
        None => run::run(cli.run),
    }
}
