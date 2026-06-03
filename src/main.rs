//! `gt` — the Gas Town operator CLI.
//!
//! A small, offline command set for driving a Gas Town deployment from the shell:
//!
//! - `gt prime` — report the active workspace/role/rig the shell carries.
//! - `gt workspace use <id>` — print an `export GT_WORKSPACE=<id>` line to eval.
//! - `gt compose up|down|destroy` — clone the `gt-app` deploy repo and drive `docker compose`.
//!
//! Every command runs locally: it inspects the environment or drives git + docker. The CLI
//! never opens a network/MCP session — agents talk to the orchestrator over MCP natively.

use anyhow::Result;
use clap::{Parser, Subcommand};

mod compose;
mod config;
mod prime;
mod workspace_cmd;

use compose::ComposeAction;
use workspace_cmd::WorkspaceAction;

#[derive(Parser)]
#[command(name = "gt", version, about = "Gas Town operator CLI")]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Report the active workspace/role/rig. Requires `GT_WORKSPACE` (aborts when unset unless
    /// `GT_WORKSPACE_DEFAULT_OPT_IN` opts into the legacy `default` fallback). Reads the
    /// environment only.
    Prime {
        /// Emit the context as a JSON object instead of the text report.
        #[arg(long)]
        json: bool,
    },
    /// Select a workspace for this shell: `use` prints an `export GT_WORKSPACE=<id>` line to eval.
    Workspace {
        #[command(subcommand)]
        action: WorkspaceAction,
    },
    /// Manage the gt-app deploy stack: `up` clones the deploy repo + `docker compose up -d`,
    /// `down` tears it down. Drives git + docker.
    Compose {
        #[command(subcommand)]
        action: ComposeAction,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let code = match &cli.cmd {
        Command::Prime { json } => prime::run(*json),
        Command::Workspace { action } => workspace_cmd::run(action),
        Command::Compose { action } => compose::run(action),
    };
    std::process::exit(code);
}
