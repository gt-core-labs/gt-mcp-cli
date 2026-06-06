//! `gt update` — self-update the installed binary, plus a passive startup notice.
//!
//! `update`/`update --check` query the latest GitHub Release of `gt-core-labs/gt` and (for
//! a plain `update`) download the matching platform asset and replace the executable in
//! place. Backed by `self_update` (sync/blocking reqwest), so it runs off the tokio reactor.
//!
//! [`maybe_notify`] is the auto path: a throttled (~once/day) background check that prints a
//! one-line "newer version available" hint to STDERR and never downloads. Throttled via a
//! cache stamp so it adds no latency to most invocations, time-boxed so a slow network never
//! delays the CLI, and silent on any error (an update hint must never break a command).
//!
//! Release assets are named `gt-<target-triple>.tar.gz` containing the `gt` binary — the
//! release workflow produces exactly that, and `self_update` selects the matching asset.

use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

const REPO_OWNER: &str = "gt-core-labs";
const REPO_NAME: &str = "gt";
const BIN_NAME: &str = "gt";

/// Min seconds between passive checks (~1/day).
const CHECK_INTERVAL_SECS: u64 = 86_400;
/// Hard cap on the passive check so a hung network never delays the CLI.
const CHECK_TIMEOUT: Duration = Duration::from_secs(2);

pub async fn run(check_only: bool) -> Result<()> {
    let current = env!("CARGO_PKG_VERSION").to_string();
    // self_update is blocking — keep it off the tokio reactor.
    tokio::task::spawn_blocking(move || do_update(&current, check_only))
        .await
        .context("self-update task panicked")?
}

fn do_update(current: &str, check_only: bool) -> Result<()> {
    let builder = self_update::backends::github::Update::configure();
    let mut builder = builder;
    builder
        .repo_owner(REPO_OWNER)
        .repo_name(REPO_NAME)
        .bin_name(BIN_NAME)
        .show_download_progress(true)
        .current_version(current);

    if check_only {
        let updater = builder.build().context("build self-update")?;
        let latest = updater
            .get_latest_release()
            .context("query the latest release")?;
        if self_update::version::bump_is_greater(current, &latest.version).unwrap_or(false) {
            println!(
                "update available: {current} → {} (run `gt update`)",
                latest.version
            );
        } else {
            println!("gt {current} is up to date");
        }
        return Ok(());
    }

    let status = builder
        .build()
        .context("build self-update")?
        .update()
        .context("download and apply the update")?;
    if status.updated() {
        println!("updated gt → {}", status.version());
    } else {
        println!("gt {current} is already the latest");
    }
    Ok(())
}

/// Passive, throttled update notice for interactive commands. Returns immediately when a
/// check ran within [`CHECK_INTERVAL_SECS`]; otherwise stamps the cache and runs a
/// time-boxed background query, printing a hint to stderr if a newer release exists. Every
/// failure path is silent — this must never interfere with the actual command.
///
/// NOT called for `gt mcp` (its stdout is the JSON-RPC channel and it is long-lived) or
/// `gt update` (which checks already).
pub fn maybe_notify() {
    if !due() {
        return;
    }
    stamp_now(); // throttle even when the check fails, so a flaky network can't busy-loop.

    let current = env!("CARGO_PKG_VERSION").to_string();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(latest_version());
    });
    if let Ok(Some(latest)) = rx.recv_timeout(CHECK_TIMEOUT) {
        if self_update::version::bump_is_greater(&current, &latest).unwrap_or(false) {
            eprintln!("gt: a newer version is available ({current} → {latest}); run `gt update`");
        }
    }
}

/// The latest stable release version, or `None` on any error (no release yet, offline, …).
fn latest_version() -> Option<String> {
    let release = self_update::backends::github::Update::configure()
        .repo_owner(REPO_OWNER)
        .repo_name(REPO_NAME)
        .bin_name(BIN_NAME)
        .current_version(env!("CARGO_PKG_VERSION"))
        .build()
        .ok()?
        .get_latest_release()
        .ok()?;
    Some(release.version)
}

/// `$XDG_CACHE_HOME/gt/last-update-check` (or `~/.cache/gt/...`).
fn stamp_path() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".cache")))?;
    Some(base.join("gt/last-update-check"))
}

/// True when no check has run within [`CHECK_INTERVAL_SECS`]. A missing/unreadable stamp ⇒ due.
fn due() -> bool {
    // An explicit opt-out for CI / scripted use.
    if std::env::var_os("GT_NO_UPDATE_CHECK").is_some() {
        return false;
    }
    let Some(path) = stamp_path() else {
        return false;
    };
    let last = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0);
    now_secs().saturating_sub(last) >= CHECK_INTERVAL_SECS
}

fn stamp_now() {
    if let Some(path) = stamp_path() {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&path, now_secs().to_string());
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
