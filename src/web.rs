//! Downloads pictures from web links pasted onto the board.
//!
//! A link straight to a picture is downloaded as is. A link to a page (an art site, a museum
//! collection, a blog post) is read for the picture the page itself says represents it (the
//! `og:image` that link previews use), and that picture is downloaded instead.

use std::time::Duration;

use ureq::tls::{RootCerts, TlsConfig};

/// Bigger downloads are refused, so a wrong link can't fill the disk.
const MAX_IMAGE: u64 = 60 * 1024 * 1024;
const MAX_PAGE: u64 = 4 * 1024 * 1024;

/// The pasted text as a web link, if it is one (a single line starting with http:// or https://).
pub fn link(text: &str) -> Option<String> {
    let text = text.trim();
    let lower = text.to_ascii_lowercase();
    let is_web = lower.starts_with("http://") || lower.starts_with("https://");
    (is_web && !text.contains(char::is_whitespace)).then(|| text.to_string())
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        // The computer's own certificates, so it works wherever the browser does.
        .tls_config(
            TlsConfig::builder()
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        )
        .timeout_global(Some(Duration::from_secs(30)))
        .user_agent(concat!("CoreBoard/", env!("CARGO_PKG_VERSION")))
        .build()
        .new_agent()
}

/// Downloads the picture behind `url`: the picture itself, or the main picture of a page.
pub fn fetch_image(url: &str) -> Result<Vec<u8>, String> {
    let agent = agent();
    let (bytes, kind) = get(&agent, url, MAX_IMAGE)?;
    if is_image(&bytes) {
        return Ok(bytes);
    }
    if !kind.contains("html") && !looks_like_html(&bytes) {
        return Err("that link isn't a picture".into());
    }
    let page = String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_PAGE as usize)]);
    let found = page_image(&page).ok_or("no picture found on that page")?;
    let target = resolve(url, &found);
    let (bytes, _) = get(&agent, &target, MAX_IMAGE)?;
    if is_image(&bytes) {
        Ok(bytes)
    } else {
        Err("that page's picture couldn't be read".into())
    }
}

fn get(agent: &ureq::Agent, url: &str, limit: u64) -> Result<(Vec<u8>, String), String> {
    let mut response = agent.get(url).call().map_err(|err| match err {
        ureq::Error::StatusCode(code) => format!("the site answered {code}"),
        ureq::Error::HostNotFound => "couldn't reach that site (offline?)".into(),
        other => other.to_string(),
    })?;
    let kind = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let bytes = response
        .body_mut()
        .with_config()
        .limit(limit)
        .read_to_vec()
        .map_err(|err| err.to_string())?;
    Ok((bytes, kind))
}

fn is_image(bytes: &[u8]) -> bool {
    image::guess_format(bytes).is_ok()
}

fn looks_like_html(bytes: &[u8]) -> bool {
    let start = String::from_utf8_lossy(&bytes[..bytes.len().min(1024)]).to_ascii_lowercase();
    start.contains("<html") || start.contains("<!doctype html")
}

/// The picture a page names for link previews (og:image, then twitter:image, then
/// `<link rel="image_src">`), as written in the page.
fn page_image(html: &str) -> Option<String> {
    let tags: Vec<&str> = html
        .split('<')
        .filter(|t| {
            let t = t.get(..5).unwrap_or(t).to_ascii_lowercase();
            t.starts_with("meta") || t.starts_with("link")
        })
        .map(|t| t.split('>').next().unwrap_or(t))
        .collect();
    let wanted = [
        ("property", "og:image:secure_url", "content"),
        ("property", "og:image", "content"),
        ("name", "og:image", "content"),
        ("name", "twitter:image", "content"),
        ("property", "twitter:image", "content"),
        ("name", "twitter:image:src", "content"),
        ("rel", "image_src", "href"),
    ];
    wanted.iter().find_map(|(key, value, take)| {
        tags.iter().find_map(|tag| {
            let matches = attr(tag, key).is_some_and(|v| v.eq_ignore_ascii_case(value));
            matches
                .then(|| attr(tag, take))
                .flatten()
                .map(|v| unescape(&v))
                .filter(|v| !v.is_empty())
        })
    })
}

/// The value of attribute `name` in the inside of a tag, quoted or not.
fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(found) = lower[from..].find(name) {
        let at = from + found;
        from = at + name.len();
        // A whole attribute name, not the end of a longer one.
        let before = lower[..at].chars().last();
        if before.is_some_and(|c| !c.is_whitespace()) {
            continue;
        }
        let rest = tag[from..].trim_start();
        let Some(rest) = rest.strip_prefix('=') else {
            continue;
        };
        let rest = rest.trim_start();
        let value = match rest.chars().next() {
            Some(q @ ('"' | '\'')) => rest[1..].split(q).next().unwrap_or(""),
            // Unquoted: up to the next space, without a self-closing tag's slash.
            _ => {
                let word = rest.split(char::is_whitespace).next().unwrap_or("");
                if word.len() > 1 {
                    word.trim_end_matches('/')
                } else {
                    word
                }
            }
        };
        return Some(value.to_string());
    }
    None
}

fn unescape(s: &str) -> String {
    s.trim()
        .replace("&amp;", "&")
        .replace("&#38;", "&")
        .replace("&quot;", "\"")
        .replace("&#x2F;", "/")
        .replace("&#47;", "/")
}

/// Turns a link found on the page at `base` into a full address.
fn resolve(base: &str, link: &str) -> String {
    if link.contains("://") {
        return link.to_string();
    }
    let scheme = base.split("://").next().unwrap_or("https");
    if let Some(rest) = link.strip_prefix("//") {
        return format!("{scheme}://{rest}");
    }
    let after = &base[scheme.len() + 3..];
    let host_end = after.find('/').map_or(base.len(), |i| scheme.len() + 3 + i);
    let origin = &base[..host_end];
    if link.starts_with('/') {
        return format!("{origin}{link}");
    }
    // Relative to the page's folder.
    let path = base[host_end..].split(['?', '#']).next().unwrap_or("");
    let folder = path.rfind('/').map_or("/", |i| &path[..=i]);
    format!("{origin}{folder}{link}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spots_web_links() {
        assert_eq!(
            link("  https://example.com/a.jpg\n").as_deref(),
            Some("https://example.com/a.jpg")
        );
        assert!(link("HTTP://EXAMPLE.COM").is_some());
        assert!(link("/home/me/a.jpg").is_none());
        assert!(link("https://a.com/x.jpg https://b.com/y.jpg").is_none());
    }

    #[test]
    fn finds_the_page_picture() {
        let html = r#"<html><head><meta name="description" content="x">
            <meta content="https://cdn.site/big.jpg?w=1200&amp;q=80" property="og:image" />
            <meta name="twitter:image" content="https://cdn.site/small.jpg"></head>"#;
        assert_eq!(
            page_image(html).as_deref(),
            Some("https://cdn.site/big.jpg?w=1200&q=80")
        );
        let html = r#"<meta name=twitter:image content=/pics/a.png>"#;
        assert_eq!(page_image(html).as_deref(), Some("/pics/a.png"));
        assert_eq!(page_image("<p>no pictures</p>"), None);
    }

    #[test]
    fn resolves_relative_links() {
        let page = "https://site.org/gallery/item.html?id=3";
        assert_eq!(resolve(page, "/p/a.jpg"), "https://site.org/p/a.jpg");
        assert_eq!(resolve(page, "a.jpg"), "https://site.org/gallery/a.jpg");
        assert_eq!(resolve(page, "//cdn.org/a.jpg"), "https://cdn.org/a.jpg");
        assert_eq!(resolve(page, "http://x.org/a.jpg"), "http://x.org/a.jpg");
        assert_eq!(
            resolve("https://site.org", "a.jpg"),
            "https://site.org/a.jpg"
        );
    }
}
