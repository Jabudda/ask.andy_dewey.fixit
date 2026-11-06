//! Gmail, read-only. Google sign-in runs in your own browser (OAuth with PKCE and a one-time
//! loopback address on this Mac). The app asks only for `gmail.readonly`, so it cannot send,
//! delete, label or mark anything as read. The OAuth client and refresh token live in the macOS
//! Keychain; access tokens are kept in memory for one sync and never stored.

use crate::error::{AppError, AppResult};
use crate::secrets;
use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

pub const SCOPE: &str = "https://www.googleapis.com/auth/gmail.readonly";
const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const REVOKE_URL: &str = "https://oauth2.googleapis.com/revoke";
const API: &str = "https://gmail.googleapis.com/gmail/v1/users/me";

pub const CLIENT_ID: &str = "gmail_client_id";
pub const CLIENT_SECRET: &str = "gmail_client_secret";
pub const REFRESH: &str = "gmail_refresh";

/// How long the app waits for you to finish signing in.
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(300);

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

pub fn decode_b64url(s: &str) -> Option<Vec<u8>> {
    let t: String = s.trim().trim_end_matches('=').chars().filter(|c| !c.is_whitespace()).collect();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(t).ok()
}

fn random_token() -> AppResult<String> {
    let mut buf = [0u8; 32];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut buf)?;
    Ok(b64(&buf))
}

/// PKCE S256 challenge for a verifier.
pub fn challenge(verifier: &str) -> String {
    b64(&Sha256::digest(verifier.as_bytes()))
}

fn form(pairs: &[(&str, &str)]) -> String {
    url::form_urlencoded::Serializer::new(String::new()).extend_pairs(pairs).finish()
}

pub fn auth_url(client_id: &str, redirect: &str, challenge: &str, state: &str, login_hint: Option<&str>) -> String {
    let mut pairs = vec![
        ("client_id", client_id),
        ("redirect_uri", redirect),
        ("response_type", "code"),
        ("scope", SCOPE),
        ("access_type", "offline"),
        ("prompt", "consent"),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
        ("state", state),
    ];
    if let Some(h) = login_hint.filter(|h| !h.is_empty()) {
        pairs.push(("login_hint", h));
    }
    format!("{AUTH_URL}?{}", form(&pairs))
}

/// Reads `code` (or `error`) and `state` from the browser's request line: `GET /?code=…&state=… HTTP/1.1`.
pub fn parse_callback(request_line: &str) -> Option<(Option<String>, Option<String>, Option<String>)> {
    let target = request_line.split_whitespace().nth(1)?;
    let u = url::Url::parse(&format!("http://127.0.0.1{target}")).ok()?;
    if u.path() != "/" {
        return None;
    }
    let get = |k: &str| u.query_pairs().find(|(n, _)| n == k).map(|(_, v)| v.into_owned());
    Some((get("code"), get("state"), get("error")))
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!("ProjectNextChapter/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(30))
        .build()
        .expect("http client")
}

async fn post_form(url: &str, body: String) -> AppResult<Value> {
    let resp = client()
        .post(url)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .map_err(|e| AppError::other(format!("Could not reach Google: {e}")))?;
    let status = resp.status();
    let v: Value = resp.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        let code = v["error"].as_str().unwrap_or("");
        let desc = v["error_description"].as_str().unwrap_or("");
        if code == "invalid_grant" {
            return Err(AppError::invalid(
                "Your Gmail sign-in has expired or was revoked. Click Reconnect Gmail. (While the Google project is in Testing mode, Google ends sign-ins after 7 days.)",
            ));
        }
        if code == "invalid_client" || code == "unauthorized_client" {
            return Err(AppError::invalid("Google did not accept the OAuth client ID or secret. Check them under Gmail setup."));
        }
        return Err(AppError::other(format!("Google refused the request ({status}): {code} {desc}")));
    }
    Ok(v)
}

fn creds() -> AppResult<(String, String)> {
    let id = secrets::get(CLIENT_ID)?.ok_or_else(|| AppError::invalid("Add your Google OAuth client ID first (Gmail setup)"))?;
    let secret = secrets::get(CLIENT_SECRET)?.ok_or_else(|| AppError::invalid("Add your Google OAuth client secret first (Gmail setup)"))?;
    Ok((id, secret))
}

/// Waits for the browser to come back to the loopback address. Blocking; run off the async runtime.
fn wait_for_code(listener: TcpListener, state: &str) -> AppResult<String> {
    listener.set_nonblocking(true)?;
    let deadline = Instant::now() + SIGN_IN_TIMEOUT;
    loop {
        if Instant::now() > deadline {
            return Err(AppError::invalid("Gmail sign-in timed out after 5 minutes. Click Connect Gmail to try again."));
        }
        let (mut stream, _) = match listener.accept() {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
            Err(e) => return Err(e.into()),
        };
        stream.set_nonblocking(false)?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        let mut buf = [0u8; 8192];
        let n = stream.read(&mut buf).unwrap_or(0);
        let req = String::from_utf8_lossy(&buf[..n]);
        let first = req.lines().next().unwrap_or("");
        let Some((code, got_state, error)) = parse_callback(first) else {
            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            continue;
        };
        let (ok, msg) = match (&code, &error) {
            (Some(_), _) if got_state.as_deref() == Some(state) => (true, "Gmail is connected. You can close this tab and go back to Next Chapter."),
            (_, Some(_)) => (false, "Gmail was not connected. You can close this tab; nothing was changed."),
            _ => (false, "That sign-in link did not match. Close this tab and click Connect Gmail again."),
        };
        let html = format!(
            "<!doctype html><meta charset=utf-8><title>Next Chapter</title><body style=\"font:16px -apple-system,sans-serif;padding:48px;max-width:560px\"><h2>Project Next Chapter</h2><p>{msg}</p></body>"
        );
        let _ = stream.write_all(
            format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}", html.len())
                .as_bytes(),
        );
        if ok {
            return Ok(code.unwrap_or_default());
        }
        return Err(match error.as_deref() {
            Some("access_denied") => AppError::invalid("You cancelled the Google sign-in. Nothing was connected."),
            Some(e) => AppError::invalid(format!("Google sign-in failed: {e}")),
            None => AppError::invalid("The sign-in response did not match this request. Try again."),
        });
    }
}

/// Full sign-in: opens Google's consent page in your browser, waits for the result, stores the
/// refresh token in the Keychain and returns the connected Gmail address.
pub async fn connect(login_hint: Option<String>) -> AppResult<String> {
    let (id, secret) = creds()?;
    let verifier = random_token()?;
    let state = random_token()?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let redirect = format!("http://127.0.0.1:{}", listener.local_addr()?.port());
    let url = auth_url(&id, &redirect, &challenge(&verifier), &state, login_hint.as_deref());
    std::process::Command::new("open").arg(&url).spawn().map_err(|e| AppError::other(format!("Could not open your browser: {e}")))?;
    let st = state.clone();
    let code = tauri::async_runtime::spawn_blocking(move || wait_for_code(listener, &st))
        .await
        .map_err(|e| AppError::other(format!("Sign-in was interrupted: {e}")))??;
    let v = post_form(
        TOKEN_URL,
        form(&[
            ("code", &code),
            ("client_id", &id),
            ("client_secret", &secret),
            ("redirect_uri", &redirect),
            ("grant_type", "authorization_code"),
            ("code_verifier", &verifier),
        ]),
    )
    .await?;
    let granted = v["scope"].as_str().unwrap_or("");
    if !granted.split(' ').any(|s| s == SCOPE) {
        return Err(AppError::invalid("Google did not grant read access to Gmail. Tick the Gmail box on the consent screen and try again."));
    }
    let refresh = v["refresh_token"].as_str().ok_or_else(|| {
        AppError::invalid("Google did not return a lasting sign-in. Remove “Next Chapter” at myaccount.google.com/permissions, then connect again.")
    })?;
    let access = v["access_token"].as_str().unwrap_or("").to_string();
    secrets::set(REFRESH, refresh)?;
    profile_address(&access).await
}

/// A short-lived access token from the stored refresh token.
pub async fn access_token() -> AppResult<String> {
    let (id, secret) = creds()?;
    let refresh = secrets::get(REFRESH)?.ok_or_else(|| AppError::invalid("Gmail is not connected"))?;
    let v = post_form(
        TOKEN_URL,
        form(&[("client_id", &id), ("client_secret", &secret), ("refresh_token", &refresh), ("grant_type", "refresh_token")]),
    )
    .await?;
    v["access_token"].as_str().map(String::from).ok_or_else(|| AppError::other("Google did not return an access token"))
}

/// Revokes the app's access at Google (best effort) and removes the token from the Keychain.
pub async fn disconnect() -> AppResult<()> {
    if let Ok(Some(t)) = secrets::get(REFRESH) {
        let _ = client().post(REVOKE_URL).header("content-type", "application/x-www-form-urlencoded").body(form(&[("token", &t)])).send().await;
    }
    secrets::delete(REFRESH)
}

async fn get(token: &str, url: &str) -> AppResult<Value> {
    let resp = client()
        .get(url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| AppError::other(format!("Could not reach Gmail: {e}")))?;
    let status = resp.status();
    let v: Value = resp.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        let msg = v["error"]["message"].as_str().unwrap_or("");
        if status.as_u16() == 403 && msg.to_lowercase().contains("has not been used") {
            return Err(AppError::invalid("The Gmail API is not turned on in your Google Cloud project. Enable it (see Gmail setup) and try again."));
        }
        return Err(AppError::other(format!("Gmail refused the request ({status}): {msg}")));
    }
    Ok(v)
}

pub async fn profile_address(token: &str) -> AppResult<String> {
    let v = get(token, &format!("{API}/profile")).await?;
    v["emailAddress"].as_str().map(String::from).ok_or_else(|| AppError::other("Gmail did not return the account address"))
}

/// Message ids matching a Gmail search, newest first, up to `cap`.
pub async fn list_ids(token: &str, query: &str, cap: usize) -> AppResult<Vec<String>> {
    let mut ids = vec![];
    let mut page: Option<String> = None;
    loop {
        let q: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
        let mut url = format!("{API}/messages?maxResults=100&q={q}");
        if let Some(p) = &page {
            url.push_str(&format!("&pageToken={p}"));
        }
        let v = get(token, &url).await?;
        for m in v["messages"].as_array().into_iter().flatten() {
            if let Some(id) = m["id"].as_str() {
                ids.push(id.to_string());
            }
        }
        page = v["nextPageToken"].as_str().map(String::from);
        if page.is_none() || ids.len() >= cap {
            break;
        }
    }
    ids.truncate(cap);
    Ok(ids)
}

pub async fn get_message(token: &str, id: &str) -> AppResult<Value> {
    get(token, &format!("{API}/messages/{id}?format=full")).await
}

// ───────────────────────────── message parsing ─────────────────────────────

/// The parts of a Gmail message the app uses.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawMessage {
    pub id: String,
    pub thread_id: String,
    pub labels: Vec<String>,
    pub from: String,
    pub to: String,
    pub cc: String,
    pub subject: String,
    pub date_ms: i64,
    pub snippet: String,
    pub html: String,
    pub text: String,
}

fn collect_bodies(part: &Value, html: &mut String, text: &mut String) {
    let mime = part["mimeType"].as_str().unwrap_or("");
    if let Some(data) = part["body"]["data"].as_str() {
        if let Some(bytes) = decode_b64url(data) {
            let s = String::from_utf8_lossy(&bytes).into_owned();
            match mime {
                "text/html" if html.is_empty() => *html = s,
                "text/plain" if text.is_empty() => *text = s,
                _ => {}
            }
        }
    }
    for p in part["parts"].as_array().into_iter().flatten() {
        collect_bodies(p, html, text);
    }
}

pub fn parse_message(v: &Value) -> Option<RawMessage> {
    let id = v["id"].as_str()?.to_string();
    let headers = v["payload"]["headers"].as_array();
    let header = |name: &str| {
        headers
            .into_iter()
            .flatten()
            .find(|h| h["name"].as_str().map(|n| n.eq_ignore_ascii_case(name)).unwrap_or(false))
            .and_then(|h| h["value"].as_str())
            .unwrap_or("")
            .to_string()
    };
    let mut html = String::new();
    let mut text = String::new();
    collect_bodies(&v["payload"], &mut html, &mut text);
    Some(RawMessage {
        id,
        thread_id: v["threadId"].as_str().unwrap_or("").to_string(),
        labels: v["labelIds"].as_array().into_iter().flatten().filter_map(|l| l.as_str().map(String::from)).collect(),
        from: header("From"),
        to: header("To"),
        cc: header("Cc"),
        subject: header("Subject"),
        date_ms: v["internalDate"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0),
        snippet: crate::parse::decode_entities(v["snippet"].as_str().unwrap_or("")),
        html,
        text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pkce_challenge_matches_rfc_7636_example() {
        assert_eq!(challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn auth_url_asks_for_read_only_offline_access() {
        let u = auth_url("abc.apps.googleusercontent.com", "http://127.0.0.1:5555", "ch", "st", Some("someone@example.com"));
        assert!(u.starts_with(AUTH_URL));
        assert!(u.contains("scope=https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fgmail.readonly"));
        assert!(u.contains("access_type=offline") && u.contains("code_challenge_method=S256"));
        assert!(u.contains("login_hint=someone%40example.com"));
        assert!(!u.contains("gmail.send") && !u.contains("gmail.modify"));
    }

    #[test]
    fn reads_the_browser_callback() {
        let (code, state, err) = parse_callback("GET /?state=xyz&code=4%2F0Ab&scope=x HTTP/1.1").unwrap();
        assert_eq!(code.as_deref(), Some("4/0Ab"));
        assert_eq!(state.as_deref(), Some("xyz"));
        assert!(err.is_none());
        let (_, _, err) = parse_callback("GET /?error=access_denied&state=xyz HTTP/1.1").unwrap();
        assert_eq!(err.as_deref(), Some("access_denied"));
        assert!(parse_callback("GET /favicon.ico HTTP/1.1").is_none());
    }

    #[test]
    fn parses_a_gmail_message() {
        let body = b64("<p>Hello <b>there</b></p>".as_bytes());
        let plain = b64(b"Hello there");
        let v = json!({
            "id": "m1", "threadId": "t1", "labelIds": ["INBOX", "UNREAD"], "snippet": "Hello &amp; welcome",
            "internalDate": "1762300000000",
            "payload": {
                "mimeType": "multipart/alternative",
                "headers": [{"name": "From", "value": "\"Acme Care Recruiting\" <jobs@acmecare.example>"}, {"name": "subject", "value": "Your application"}],
                "parts": [
                    {"mimeType": "text/plain", "body": {"data": plain}},
                    {"mimeType": "text/html", "body": {"data": body}}
                ]
            }
        });
        let m = parse_message(&v).unwrap();
        assert_eq!(m.thread_id, "t1");
        assert_eq!(m.subject, "Your application");
        assert!(m.from.contains("jobs@acmecare.example"));
        assert_eq!(m.text, "Hello there");
        assert!(m.html.contains("<b>there</b>"));
        assert_eq!(m.snippet, "Hello & welcome");
        assert_eq!(m.date_ms, 1762300000000);
    }

    #[test]
    fn decodes_padded_and_unpadded_base64url() {
        assert_eq!(decode_b64url("aGk=").unwrap(), b"hi");
        assert_eq!(decode_b64url("aGk").unwrap(), b"hi");
    }
}
