//! `gt compose` — manage the gt-app deploy stack.
//!
//! Clones/updates the `gt-app` deploy repo (compose +
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
        /// Directory to clone the deploy repo into (default: `~/gt-app`).
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
    /// Stop + remove the stack containers (`docker compose down`). Data volumes are
    /// KEPT — `compose up` later resumes with the same Dolt/PG/event-log data. To wipe
    /// the data too, use the separate `compose destroy` command.
    Down {
        /// Directory the deploy repo was cloned into (default: `~/gt-app`).
        #[arg(long, env = "GT_APP_DIR")]
        dir: Option<PathBuf>,
    },
    /// DESTROY the stack AND its data volumes (`docker compose down --volumes`). This
    /// permanently deletes the Dolt/PG/event-log data — separate from `down` on purpose
    /// so a routine teardown can never drop data. Requires `--yes` to proceed.
    Destroy {
        /// Directory the deploy repo was cloned into (default: `~/gt-app`).
        #[arg(long, env = "GT_APP_DIR")]
        dir: Option<PathBuf>,
        /// Confirm the irreversible data wipe. Without it the command aborts.
        #[arg(long)]
        yes: bool,
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
        ComposeAction::Down { dir } => down(dir.clone()),
        ComposeAction::Destroy { dir, yes } => destroy(dir.clone(), *yes),
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

fn down(dir: Option<PathBuf>) -> Result<()> {
    let dir = resolve_dir(dir)?;
    require_compose_file(&dir)?;
    eprintln!("==> docker compose down (data volumes kept)");
    compose(&dir, &["down"])?;
    Ok(())
}

fn destroy(dir: Option<PathBuf>, yes: bool) -> Result<()> {
    let dir = resolve_dir(dir)?;
    require_compose_file(&dir)?;
    if !yes {
        bail!(
            "refusing to destroy data volumes without --yes \
             (this permanently deletes the Dolt/PG/event-log data)"
        );
    }
    eprintln!("==> docker compose down --volumes (DESTROYING data volumes)");
    compose(&dir, &["down", "--volumes"])?;
    Ok(())
}

fn require_compose_file(dir: &PathBuf) -> Result<()> {
    if !dir.join("docker-compose.yml").is_file() {
        bail!("no docker-compose.yml in {} — run `compose up` first", dir.display());
    }
    Ok(())
}

/// Default deploy dir: an explicit `--dir`/`$GT_APP_DIR` wins; else `~/gt-app`
/// (user home root — a path docker compose can always traverse).
fn resolve_dir(dir: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(d) = dir {
        return Ok(d);
    }
    let home = std::env::var_os("HOME").ok_or_else(|| anyhow!("HOME not set"))?;
    Ok(PathBuf::from(home).join("gt-app"))
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
