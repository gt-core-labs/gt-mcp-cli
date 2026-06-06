//! `gt login` browser flow — authenticate through an OAuth provider in the system browser
//! (hq-gt-login-oauth.3), the way `claude login` / `gcloud auth login` do.
//!
//! The dance:
//!
//! 1. `GET {server}/auth/providers` — the public discovery list. 0 ⇒ a clear error; 1 ⇒ used
//!    automatically; N ⇒ a menu.
//! 2. Bind a loopback server on `127.0.0.1:<ephemeral>` and open the browser at
//!    `{server}/auth/providers/{id}/authorize?cli_redirect=http://127.0.0.1:<port>/callback`.
//! 3. The user authenticates with the provider; the backend (hq-gt-login-oauth.2) 302s a ONE-SHOT
//!    `code` back to the loopback — never the token, never in a fragment.
//! 4. `POST {server}/auth/cli/exchange {code}` redeems the code for the access + refresh pair.
//!
//! The loopback only ever sees a short-lived code, so nothing sensitive lands in shell history or a
//! proxy log. Returns the same [`Tokens`] the password path yields, so the rest of `gt init` (the
//! workspace/rig pick + config save) is unchanged.

use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use gt_mcp::Tokens;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// How long to wait for the browser round-trip before giving up.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);

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

    // Bind the loopback FIRST so we can put its real port in the authorize URL.
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .context("binding a loopback port for the login callback")?;
    let port = listener
        .local_addr()
        .context("reading the loopback port")?
        .port();
    let redirect = format!("http://127.0.0.1:{port}/callback");

    let authorize_url = format!(
        "{server}/auth/providers/{}/authorize?cli_redirect={}",
        provider.id,
        urlencode(&redirect),
    );

    eprintln!(
        "[gt login] opening your browser to authenticate with `{}` …",
        provider.id
    );
    eprintln!("[gt login] if it doesn't open, visit:\n    {authorize_url}");
    // Best-effort: a headless box has no browser, but the URL is printed above to open manually.
    let _ = open::that(&authorize_url);

    let code = wait_for_code(&listener).await?;

    eprintln!("[gt login] exchanging the authorization code …");
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

/// Accept one loopback connection, parse `?code=…` from the request line, reply with a small page,
/// and return the code. Times out after [`LOGIN_TIMEOUT`].
async fn wait_for_code(listener: &TcpListener) -> Result<String> {
    let accept = async {
        loop {
            let (mut sock, _) = listener
                .accept()
                .await
                .context("accepting the login callback")?;
            // Read the request head — the GET line is all we need; it arrives in the first packet.
            let mut buf = vec![0u8; 8192];
            let n = sock
                .read(&mut buf)
                .await
                .context("reading the callback request")?;
            let head = String::from_utf8_lossy(&buf[..n]);
            let target = head
                .lines()
                .next()
                .and_then(|l| l.split_whitespace().nth(1))
                .unwrap_or("");

            // Ignore noise (e.g. /favicon.ico) and wait for the real /callback?code=… .
            if let Some(code) = code_from_target(target) {
                let body = "<html><body style=\"font-family:sans-serif\">\
                    <h3>Login complete</h3>You can close this tab and return to the terminal.\
                    </body></html>";
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.flush().await;
                return Ok(code);
            }
            let resp = "HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n";
            let _ = sock.write_all(resp.as_bytes()).await;
        }
    };
    tokio::time::timeout(LOGIN_TIMEOUT, accept)
        .await
        .map_err(|_| anyhow!("timed out waiting for the browser login (5 min)"))?
}

/// Pull the `code` query value out of a request target like `/callback?code=abc&x=y`.
fn code_from_target(target: &str) -> Option<String> {
    let query = target.split('?').nth(1)?;
    query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == "code").then(|| urldecode(v))
    })
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
            "exchanging the login code failed ({}): the code may have expired — try again",
            resp.status()
        );
    }
    resp.json().await.context("decoding the token response")
}

/// Minimal percent-encoding for a query-string value (the loopback URL). Encodes everything that is
/// not RFC 3986 unreserved, which is more than enough for a `http://127.0.0.1:<port>/callback` URL.
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

/// Decode a percent-encoded query value (`+` → space, `%XX` → byte).
fn urldecode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(b);
                    i += 3;
                    continue;
                }
                out.push(b'%');
                i += 1;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_parsed_from_target() {
        assert_eq!(
            code_from_target("/callback?code=abc123").as_deref(),
            Some("abc123")
        );
        assert_eq!(
            code_from_target("/callback?state=x&code=zzz&y=1").as_deref(),
            Some("zzz")
        );
        assert_eq!(code_from_target("/callback").as_deref(), None);
        assert_eq!(code_from_target("/favicon.ico").as_deref(), None);
    }

    #[test]
    fn code_is_url_decoded() {
        assert_eq!(
            code_from_target("/cb?code=a%2Bb%3Dc").as_deref(),
            Some("a+b=c")
        );
    }

    #[test]
    fn urlencode_roundtrips_loopback() {
        let u = "http://127.0.0.1:8976/callback";
        assert_eq!(urldecode(&urlencode(u)), u);
        assert!(urlencode(u).contains("%3A") && urlencode(u).contains("%2F"));
    }
}
