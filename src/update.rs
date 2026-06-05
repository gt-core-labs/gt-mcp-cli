//! `gt update` — self-update the installed binary (hq-gt-cli).
//!
//! Checks the latest GitHub Release of `gt-core-labs/gt-core`, and if it is newer than
//! the running binary, downloads the matching platform asset and replaces the executable
//! in place. Backed by the `self_update` crate (sync/blocking reqwest), so it runs on a
//! blocking thread off the async runtime.
//!
//! Release assets must be named `gt-<target-triple>.tar.gz` (e.g.
//! `gt-x86_64-unknown-linux-gnu.tar.gz`) containing the `gt` binary — the release
//! workflow produces exactly that, and `self_update` selects the asset matching the
//! current target.

use anyhow::{Context, Result};

const REPO_OWNER: &str = "gt-core-labs";
const REPO_NAME: &str = "gt";
const BIN_NAME: &str = "gt";

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
            println!("update available: {current} → {} (run `gt update`)", latest.version);
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
