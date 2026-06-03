//! `gt-mcp-cli compose` — manage the gt-app deploy stack.
//!
//! Offline (no MCP session): clones/updates the `gt-app` deploy repo (compose +
//! `.env.example` + `mcp-scope.toml`) and drives `docker compose` against it.
//! `up` brings the stack up (pull image from DockerHub, then `up -d`); `down`
//! tears it down, optionally dropping the data volumes.

use anyhow::{anyhow, bail, Context, Result};
use clap::Subcommand;
use std::path::PathBuf;
use std::process::Command;

/// Default deploy repo (public). Override with `--repo` / `GT_APP_REPO`.
const DEFAULT_REPO: &str = "https://github.com/gt-core-labs/gt-app.git";

#[derive(Subcommand)]
pub enum ComposeAction {
    /// Clone/update the gt-app deploy repo and bring the stack up (`docker compose up -d`).
    Up {
        /// Directory to clone the deploy repo into (default: `~/.local/share/gt-app`).
        #[arg(long, env = "GT_APP_DIR")]
        dir: Option<PathBuf>,
        /// Deploy repo URL.
        #[arg(long, env = "GT_APP_REPO", default_value = DEFAULT_REPO)]
        repo: String,
        /// Git branch / ref to check out.
        #[arg(long, default_value = "main")]
        branch: String,
        /// Skip the `docker compose pull` before `up` (use the locally cached image).
        #[arg(long)]
        no_pull: bool,
    },
    /// Tear the stack down (`docker compose down`).
    Down {
        /// Directory the deploy repo was cloned into (default: `~/.local/share/gt-app`).
        #[arg(long, env = "GT_APP_DIR")]
        dir: Option<PathBuf>,
        /// Also remove the named volumes — DESTROYS the Dolt/PG/event-log data.
        #[arg(long)]
        volumes: bool,
    },
}

/// Run a compose action. Returns the process exit code (0 = ok).
pub fn run(action: &ComposeAction) -> i32 {
    let result = match action {
        ComposeAction::Up {
            dir,
            repo,
            branch,
            no_pull,
        } => up(dir.clone(), repo, branch, *no_pull),
        ComposeAction::Down { dir, volumes } => down(dir.clone(), *volumes),
    };
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("error: {e:#}");
            1
        }
    }
}

fn up(dir: Option<PathBuf>, repo: &str, branch: &str, no_pull: bool) -> Result<()> {
    let dir = resolve_dir(dir)?;
    sync_repo(&dir, repo, branch)?;

    if !no_pull {
        eprintln!("==> docker compose pull");
        compose(&dir, &["pull"])?;
    }
    eprintln!("==> docker compose up -d");
    compose(&dir, &["up", "-d"])?;
    eprintln!("stack up — MCP at http://127.0.0.1:8765/mcp ({})", dir.display());
    Ok(())
}

fn down(dir: Option<PathBuf>, volumes: bool) -> Result<()> {
    let dir = resolve_dir(dir)?;
    if !dir.join("docker-compose.yml").is_file() {
        bail!("no docker-compose.yml in {} — run `compose up` first", dir.display());
    }
    let mut args = vec!["down"];
    if volumes {
        args.push("--volumes");
    }
    eprintln!("==> docker compose {}", args.join(" "));
    compose(&dir, &args)?;
    Ok(())
}

/// Default deploy dir: `$GT_APP_DIR` (handled by clap) else `~/.local/share/gt-app`.
fn resolve_dir(dir: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(d) = dir {
        return Ok(d);
    }
    let home = std::env::var_os("HOME").ok_or_else(|| anyhow!("HOME not set"))?;
    Ok(PathBuf::from(home).join(".local/share/gt-app"))
}

/// Clone the repo if absent, else fast-forward the existing checkout.
fn sync_repo(dir: &PathBuf, repo: &str, branch: &str) -> Result<()> {
    if dir.join(".git").is_dir() {
        eprintln!("==> git pull ({})", dir.display());
        run_cmd(
            Command::new("git")
                .current_dir(dir)
                .args(["pull", "--ff-only", "origin", branch]),
            "git pull",
        )
    } else {
        eprintln!("==> git clone {repo} -> {}", dir.display());
        run_cmd(
            Command::new("git").args(["clone", "--branch", branch, repo, &dir.to_string_lossy()]),
            "git clone",
        )
    }
}

/// `docker compose <args...>` in the deploy dir. Tries `docker compose`, falling
/// back to the legacy `docker-compose` binary.
fn compose(dir: &PathBuf, args: &[&str]) -> Result<()> {
    let mut cmd = Command::new("docker");
    cmd.current_dir(dir).arg("compose").args(args);
    match cmd.status() {
        Ok(s) if s.success() => return Ok(()),
        Ok(s) => {
            // `docker compose` ran but the subcommand failed — surface that, don't fall back.
            bail!("docker compose {} exited with {}", args.join(" "), s);
        }
        Err(_) => {} // `docker` missing — try the standalone binary.
    }
    run_cmd(
        Command::new("docker-compose").current_dir(dir).args(args),
        "docker-compose",
    )
}

/// Run a command, mapping spawn failure + non-zero exit to an error.
fn run_cmd(cmd: &mut Command, label: &str) -> Result<()> {
    let status = cmd.status().with_context(|| format!("spawn {label}"))?;
    if !status.success() {
        bail!("{label} exited with {status}");
    }
    Ok(())
}
