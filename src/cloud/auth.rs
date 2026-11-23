//! Signing in to Google Drive, the way desktop apps do: the browser opens Google's sign-in
//! page, and Google sends the browser back to a one-time address on this computer
//! (127.0.0.1) that Core Board listens on. No password ever passes through Core Board.
//!
//! The permission asked for is `drive.file`: Core Board sees only the files it made itself,
//! never the rest of someone's Drive.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const SCOPE: &str = "https://www.googleapis.com/auth/drive.file";
/// How long to wait for the person to finish in the browser.
const WAIT: Duration = Duration::from_secs(300);

/// The app's Google sign-in client. Desktop apps can't keep a secret, so Google treats this
/// "secret" as public; the protection is PKCE and the one-time local address. Set when building
/// (or, for testing, when running) with COREBOARD_GOOGLE_CLIENT_ID / _SECRET.
pub fn client() -> Option<(String, String)> {
    let id = std::env::var("COREBOARD_GOOGLE_CLIENT_ID")
        .ok()
        .or(option_env!("COREBOARD_GOOGLE_CLIENT_ID").map(str::to_owned))?;
    let secret = std::env::var("COREBOARD_GOOGLE_CLIENT_SECRET")
        .ok()
        .or(option_env!("COREBOARD_GOOGLE_CLIENT_SECRET").map(str::to_owned))
        .unwrap_or_default();
    (!id.trim().is_empty()).then_some((id, secret))
}

/// A connected Google account, kept in the settings folder.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Account {
    pub email: String,
    pub refresh_token: String,
}

fn account_file() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "coreboard")
        .map(|d| d.config_dir().join("cloud-account.json"))
}

pub fn load_account() -> Option<Account> {
    serde_json::from_slice(&std::fs::read(account_file()?).ok()?).ok()
}

pub fn save_account(account: &Account) -> Result<(), String> {
    let file = account_file().ok_or("no settings folder")?;
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_vec_pretty(account).map_err(|e| e.to_string())?;
    std::fs::write(&file, json).map_err(|e| e.to_string())?;
    // Only this person's account can read it.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

pub fn forget_account() {
    if let Some(file) = account_file() {
        let _ = std::fs::remove_file(file);
    }
}

fn random_text(bytes: usize) -> Result<String, String> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(|e| e.to_string())?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf))
}

/// PKCE: the code challenge for a verifier.
fn challenge(verifier: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// Percent-encodes a URL query value.
pub fn encode(value: &str) -> String {
    let mut out = String::new();
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => match u8::from_str_radix(&value[i + 1..i + 3], 16) {
                Ok(b) => {
                    out.push(b);
                    i += 3;
                    continue;
                }
                Err(_) => out.push(b'%'),
            },
            b'+' => out.push(b' '),
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The value of `key` in a query string.
fn query_value(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then(|| decode(v))
    })
}

#[derive(Deserialize)]
struct TokenAnswer {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
}

fn post_form(pairs: &[(&str, &str)]) -> Result<TokenAnswer, String> {
    let body = pairs
        .iter()
        .map(|(k, v)| format!("{k}={}", encode(v)))
        .collect::<Vec<_>>()
        .join("&");
    let mut response = crate::web::agent()
        .post(TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .send(body.as_bytes())
        .map_err(|err| match err {
            ureq::Error::StatusCode(400 | 401) => {
                crate::i18n::tr("Google said no (signed out, or access was removed)").to_string()
            }
            other => other.to_string(),
        })?;
    let bytes = response
        .body_mut()
        .read_to_vec()
        .map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

/// A fresh access token (good for about an hour) for a connected account.
pub fn access_token(account: &Account) -> Result<String, String> {
    let (id, secret) = client().ok_or("cloud sync isn't set up in this build")?;
    Ok(post_form(&[
        ("client_id", &id),
        ("client_secret", &secret),
        ("refresh_token", &account.refresh_token),
        ("grant_type", "refresh_token"),
    ])?
    .access_token)
}

/// Opens Google's sign-in page in the browser and waits for the person to finish there.
/// Returns the connected account.
pub fn sign_in() -> Result<Account, String> {
    let (id, secret) = client().ok_or("cloud sync isn't set up in this build")?;
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let redirect = format!(
        "http://127.0.0.1:{}",
        listener.local_addr().map_err(|e| e.to_string())?.port()
    );
    let verifier = random_text(48)?;
    let state = random_text(16)?;
    let url = format!(
        "{AUTH_URL}?client_id={}&redirect_uri={}&response_type=code&scope={}&code_challenge={}\
         &code_challenge_method=S256&state={}&access_type=offline&prompt=consent",
        encode(&id),
        encode(&redirect),
        encode(SCOPE),
        challenge(&verifier),
        encode(&state),
    );
    opener::open_browser(&url).map_err(|e| e.to_string())?;

    // Wait for Google to send the browser back here.
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let started = Instant::now();
    let code = loop {
        if started.elapsed() > WAIT {
            return Err(crate::i18n::tr("sign-in took too long, try again").into());
        }
        let mut stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            Err(err) => return Err(err.to_string()),
        };
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut line = String::new();
        let _ = BufReader::new(&stream).read_line(&mut line);
        // "GET /?code=…&state=… HTTP/1.1"
        let query = line
            .split_whitespace()
            .nth(1)
            .and_then(|path| path.split_once('?'))
            .map(|(_, q)| q.to_string())
            .unwrap_or_default();
        let (ok, message) = if query_value(&query, "state").as_deref() != Some(state.as_str()) {
            (None, "")
        } else if let Some(code) = query_value(&query, "code") {
            (
                Some(code),
                crate::i18n::tr("Connected. You can close this tab and go back to Core Board."),
            )
        } else {
            (
                Some(String::new()),
                crate::i18n::tr("Not connected. You can close this tab and go back to Core Board."),
            )
        };
        if let Some(code) = ok {
            let page = format!(
                "<!doctype html><meta charset=utf-8><title>Core Board</title>\
                 <body style=\"font:18px system-ui;background:#0d0f0c;color:#f3f6ee;\
                 display:grid;place-items:center;height:90vh\"><p>{message}</p>"
            );
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{page}",
                page.len()
            );
            if code.is_empty() {
                return Err(crate::i18n::tr("sign-in was cancelled").into());
            }
            break code;
        }
        // Something else knocked (a browser's favicon request): ignore it.
        let _ = write!(
            stream,
            "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n"
        );
    };

    let answer = post_form(&[
        ("client_id", &id),
        ("client_secret", &secret),
        ("code", &code),
        ("code_verifier", &verifier),
        ("redirect_uri", &redirect),
        ("grant_type", "authorization_code"),
    ])?;
    let refresh_token = answer
        .refresh_token
        .ok_or("Google didn't give Core Board lasting access; try again")?;
    let email = super::drive::Drive::new(answer.access_token)
        .email()
        .unwrap_or_default();
    let account = Account {
        email,
        refresh_token,
    };
    save_account(&account)?;
    Ok(account)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_matches_the_standard_example() {
        // RFC 7636, appendix B.
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn reads_the_answer_from_google() {
        let q = "state=abc&code=4%2F0Ab_x-y&scope=https%3A%2F%2Fwww.googleapis.com";
        assert_eq!(query_value(q, "code").as_deref(), Some("4/0Ab_x-y"));
        assert_eq!(query_value(q, "state").as_deref(), Some("abc"));
        assert_eq!(query_value(q, "error"), None);
        assert_eq!(encode("a b/é"), "a%20b%2F%C3%A9");
        assert_eq!(decode(&encode("a b/é&=")), "a b/é&=");
        assert_eq!(random_text(16).unwrap().len(), 22);
    }
}
