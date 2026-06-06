//! `gt login` browser flow — authenticate through an OAuth provider in the system browser
//! (hq-gt-login-oauth.6), the way `claude login` does, with an out-of-band (OOB) paste step.
//!
//! The dance:
//!
//! 1. `GET {server}/auth/providers` — the public discovery list. 0 ⇒ a clear error; 1 ⇒ used
//!    automatically; N ⇒ a menu.
//! 2. Open the browser at
//!    `{server}/auth/providers/{id}/authorize?cli_redirect=urn:ietf:wg:oauth:2.0:oob`.
//! 3. The user authenticates with the provider; the backend (hq-gt-login-oauth.6) renders a page
//!    showing a ONE-SHOT `code` — never the token.
//! 4. The user pastes the code into the terminal; `POST {server}/auth/cli/exchange {code}` redeems
//!    it for the access + refresh pair.
//!
//! No loopback server: the only thing that crosses the boundary is a short-lived code the user
//! copies by hand, so nothing sensitive lands in shell history or a proxy log. Returns the same
//! [`Tokens`] the password path used to, so the rest of `gt init` is unchanged.

use std::time::Duration;

use anyhow::{bail, Context, Result};
use gt_mcp::Tokens;

/// The out-of-band `cli_redirect` sentinel the backend renders a code page for (RFC 6749 §3.1.2.1's
/// classic OOB value). Must match `CLI_REDIRECT_OOB` in gt-auth.
const CLI_REDIRECT_OOB: &str = "urn:ietf:wg:oauth:2.0:oob";

/// One login provider from the public `GET /auth/providers` discovery list.
#[derive(Debug, serde::Deserialize)]
struct PublicProvider {
    id: String,
    #[serde(default)]
    display_name: String,
}

/// The token pair returned by `POST /auth/cli/exchange`.
#[derive(Debug, serde::Deserialize)]
struct ExchangeResponse {
    access_token: String,
    refresh_token: String,
}

/// Run the browser login against `server` (already normalized, no trailing slash). Returns the
/// minted token pair, or an error with actionable context at each failure point.
pub async fn browser_login(server: &str) -> Result<Tokens> {
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .context("building the HTTP client")?;

    let provider = pick_provider(&http, server).await?;

    let authorize_url = format!(
        "{server}/auth/providers/{}/authorize?cli_redirect={}",
        provider.id,
        urlencode(CLI_REDIRECT_OOB),
    );

    eprintln!(
        "[gt login] opening your browser to authenticate with `{}` …",
        provider.id
    );
    eprintln!("[gt login] if it doesn't open, visit this URL:\n    {authorize_url}");
    // Best-effort: a headless box has no browser, but the URL is printed above to open manually.
    let _ = open::that(&authorize_url);

    // The user authenticates, the page shows a code, they paste it here.
    let code = prompt_code()?;

    eprintln!("[gt login] exchanging the code …");
    let tokens = exchange_code(&http, server, &code).await?;
    Ok(Tokens {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
    })
}

/// Fetch the discovery list and choose a provider: 0 ⇒ error, 1 ⇒ that one, N ⇒ a menu.
async fn pick_provider(http: &reqwest::Client, server: &str) -> Result<PublicProvider> {
    let url = format!("{server}/auth/providers");
    let resp = http
        .get(&url)
        .send()
        .await
        .with_context(|| format!("reaching {url}"))?;
    if !resp.status().is_success() {
        bail!("{url} returned HTTP {}", resp.status());
    }
    let mut providers: Vec<PublicProvider> =
        resp.json().await.context("decoding the providers list")?;

    match providers.len() {
        0 => bail!(
            "this server has no login providers configured — ask an admin to register one, \
             or log in with a token: `gt login --token gtpat_…`"
        ),
        1 => Ok(providers.remove(0)),
        _ => {
            let labels: Vec<String> = providers
                .iter()
                .map(|p| {
                    if p.display_name.is_empty() {
                        p.id.clone()
                    } else {
                        format!("{}  ({})", p.display_name, p.id)
                    }
                })
                .collect();
            let choice = inquire::Select::new("Login with:", labels.clone())
                .prompt()
                .context("selecting a provider")?;
            let idx = labels.iter().position(|l| l == &choice).unwrap_or(0);
            Ok(providers.remove(idx))
        }
    }
}

/// Prompt the user to paste the code the browser page showed. Trims surrounding whitespace.
fn prompt_code() -> Result<String> {
    let code = inquire::Text::new("Paste the code from your browser:")
        .prompt()
        .context("reading the pasted code")?;
    let code = code.trim().to_string();
    if code.is_empty() {
        bail!("no code entered");
    }
    Ok(code)
}

/// Redeem the one-shot code for the token pair.
async fn exchange_code(
    http: &reqwest::Client,
    server: &str,
    code: &str,
) -> Result<ExchangeResponse> {
    let url = format!("{server}/auth/cli/exchange");
    let resp = http
        .post(&url)
        .json(&serde_json::json!({ "code": code }))
        .send()
        .await
        .with_context(|| format!("reaching {url}"))?;
    if !resp.status().is_success() {
        bail!(
            "exchanging the code failed ({}): it may be wrong or expired — run `gt login` again",
            resp.status()
        );
    }
    resp.json().await.context("decoding the token response")
}

/// Minimal percent-encoding for a query-string value. Encodes everything that is not RFC 3986
/// unreserved — enough for the `urn:ietf:wg:oauth:2.0:oob` sentinel (its colons get encoded).
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urlencode_encodes_the_oob_sentinel() {
        let e = urlencode(CLI_REDIRECT_OOB);
        assert!(!e.contains(':'), "colons must be encoded: {e}");
        assert!(e.contains("%3A"));
        // Unreserved chars survive.
        assert!(e.starts_with("urn%3Aietf"));
    }
}
