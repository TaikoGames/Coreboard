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

pub fn agent() -> ureq::Agent {
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

/// What a pasted link turned out to be.
pub enum Fetched {
    /// A picture (the link itself, or the main picture of the page).
    Image(Vec<u8>),
    /// Something without a picture to show (a page, a PDF, a video): it goes on the board as a
    /// link card, with the page's title when there is one.
    Page { title: Option<String> },
}

/// Downloads the picture behind `url`: the picture itself, or the main picture of a page.
pub fn fetch_image(url: &str) -> Result<Fetched, String> {
    let agent = agent();
    let (bytes, kind) = get(&agent, url, MAX_IMAGE)?;
    if is_image(&bytes) {
        return Ok(Fetched::Image(bytes));
    }
    if !kind.contains("html") && !looks_like_html(&bytes) {
        return Ok(Fetched::Page { title: None });
    }
    let page = String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_PAGE as usize)]);
    let title = html_title(&page);
    let Some(found) = page_image(&page) else {
        return Ok(Fetched::Page { title });
    };
    let target = resolve(url, &found);
    match get(&agent, &target, MAX_IMAGE) {
        Ok((bytes, _)) if is_image(&bytes) => Ok(Fetched::Image(bytes)),
        _ => Ok(Fetched::Page { title }),
    }
}

/// What a link card shows: the page's title and its preview picture.
#[derive(Default)]
pub struct Preview {
    pub title: Option<String>,
    /// A small JPEG, ready to store in the board.
    pub image: Option<Vec<u8>>,
}

/// The longest side of a link card's stored preview picture.
const PREVIEW_SIDE: u32 = 480;

/// The video id of a YouTube link (watch, youtu.be, shorts, embed and live links).
pub fn youtube_id(url: &str) -> Option<String> {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = host.trim_start_matches("www.").trim_start_matches("m.");
    let id = match host {
        "youtu.be" => path.split(['?', '#', '/']).next()?.to_string(),
        "youtube.com" | "music.youtube.com" | "youtube-nocookie.com" => {
            if let Some(query) = path.strip_prefix("watch?") {
                query
                    .split('&')
                    .find_map(|kv| kv.strip_prefix("v="))?
                    .split('#')
                    .next()?
                    .to_string()
            } else {
                let mut parts = path.split('/');
                match parts.next()? {
                    "shorts" | "embed" | "live" | "v" => {
                        parts.next()?.split(['?', '#']).next()?.to_string()
                    }
                    _ => return None,
                }
            }
        }
        _ => return None,
    };
    let valid = id.len() == 11
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    valid.then_some(id)
}

/// Whether `url` is a video Core Board can play in its floating player.
pub fn is_video(url: &str) -> bool {
    youtube_id(url).is_some()
}

/// A YouTube video's title and thumbnail, from YouTube's public video info (which, unlike the
/// page, never shows a cookie-consent screen first).
fn youtube_preview(agent: &ureq::Agent, id: &str) -> Preview {
    let info = format!(
        "https://www.youtube.com/oembed?format=json&url=https://www.youtube.com/watch?v={id}"
    );
    let title = get(agent, &info, MAX_PAGE)
        .ok()
        .and_then(|(bytes, _)| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|json| json.get("title")?.as_str().map(str::to_string));
    let image = ["maxresdefault", "hqdefault"].iter().find_map(|name| {
        let (bytes, _) = get(
            agent,
            &format!("https://i.ytimg.com/vi/{id}/{name}.jpg"),
            MAX_IMAGE,
        )
        .ok()?;
        shrink(&bytes)
    });
    Preview { title, image }
}

/// The title and preview picture (the one chat apps show for links) of the page at `url`.
pub fn page_preview(url: &str) -> Preview {
    let agent = agent();
    if let Some(id) = youtube_id(url) {
        return youtube_preview(&agent, &id);
    }
    let Ok((bytes, kind)) = get(&agent, url, MAX_IMAGE) else {
        return Preview::default();
    };
    // A link straight to a picture is its own preview.
    if is_image(&bytes) {
        return Preview {
            title: None,
            image: shrink(&bytes),
        };
    }
    if !kind.contains("html") && !looks_like_html(&bytes) {
        return Preview::default();
    }
    let page = String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_PAGE as usize)]);
    let image = page_image(&page)
        .and_then(|found| get(&agent, &resolve(url, &found), MAX_IMAGE).ok())
        .and_then(|(bytes, _)| shrink(&bytes));
    Preview {
        title: html_title(&page),
        image,
    }
}

/// A picture made small enough to keep in a board as a card's thumbnail.
fn shrink(bytes: &[u8]) -> Option<Vec<u8>> {
    let image = image::load_from_memory(bytes).ok()?;
    let small = image.thumbnail(PREVIEW_SIDE, PREVIEW_SIDE).to_rgb8();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85)
        .encode_image(&small)
        .ok()?;
    Some(out)
}

/// A page's title: the one link previews use (og:title), else its `<title>`.
fn html_title(html: &str) -> Option<String> {
    let og = html
        .split('<')
        .filter(|t| t.get(..4).is_some_and(|t| t.eq_ignore_ascii_case("meta")))
        .map(|t| t.split('>').next().unwrap_or(t))
        .find(|tag| {
            ["property", "name"]
                .iter()
                .any(|key| attr(tag, key).is_some_and(|v| v.eq_ignore_ascii_case("og:title")))
        })
        .and_then(|tag| attr(tag, "content"));
    let title = og.or_else(|| {
        let lower = html.to_ascii_lowercase();
        let start = lower.find("<title")?;
        let open = start + lower[start..].find('>')? + 1;
        let close = open + lower[open..].find("</title")?;
        Some(html[open..close].to_string())
    })?;
    let title = unescape(&title)
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    (!title.is_empty()).then(|| title.chars().take(120).collect())
}

/// Turns what someone typed into a web address: adds https:// to "example.com/page".
pub fn normalize_link(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() || text.contains(char::is_whitespace) {
        return None;
    }
    if let Some(url) = link(text) {
        return Some(url);
    }
    let host = text.split(['/', '?', '#']).next().unwrap_or("");
    (host.contains('.') && !host.starts_with('.') && !host.ends_with('.'))
        .then(|| format!("https://{text}"))
}

/// The site's name for a card: "www.artstation.com/x" -> "artstation.com".
pub fn domain(url: &str) -> String {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    host.trim_start_matches("www.").to_string()
}

pub fn get(agent: &ureq::Agent, url: &str, limit: u64) -> Result<(Vec<u8>, String), String> {
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

    /// Needs the internet: run with `cargo test -- --ignored` (the player workflow does).
    #[test]
    #[ignore]
    fn fetches_a_youtube_title_and_thumbnail() {
        let preview = page_preview("https://www.youtube.com/watch?v=jNQXAC9IVRw");
        assert_eq!(preview.title.as_deref(), Some("Me at the zoo"));
        let image = image::load_from_memory(&preview.image.expect("a thumbnail")).unwrap();
        assert!(image.width() >= 300);
    }

    #[test]
    fn recognizes_youtube_links() {
        for url in [
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://youtube.com/watch?feature=share&v=dQw4w9WgXcQ&t=42",
            "https://m.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://youtu.be/dQw4w9WgXcQ?si=abc",
            "https://www.youtube.com/shorts/dQw4w9WgXcQ",
            "https://www.youtube.com/embed/dQw4w9WgXcQ",
        ] {
            assert_eq!(youtube_id(url).as_deref(), Some("dQw4w9WgXcQ"), "{url}");
        }
        for url in [
            "https://www.youtube.com/@someone",
            "https://www.youtube.com/watch?v=short",
            "https://example.com/watch?v=dQw4w9WgXcQ",
        ] {
            assert_eq!(youtube_id(url), None, "{url}");
        }
    }

    #[test]
    fn reads_page_titles() {
        let html = r#"<head><title>  Moss &amp; Stone
            | Gallery </title><meta property="og:title" content="Moss study"></head>"#;
        assert_eq!(html_title(html).as_deref(), Some("Moss study"));
        let html = "<html><TITLE>Rock &amp; Roll</TITLE></html>";
        assert_eq!(html_title(html).as_deref(), Some("Rock & Roll"));
        assert_eq!(html_title("<p>none</p>"), None);
    }

    #[test]
    fn turns_typing_into_links() {
        assert_eq!(
            normalize_link("artstation.com/x").as_deref(),
            Some("https://artstation.com/x")
        );
        assert_eq!(
            normalize_link(" https://a.org ").as_deref(),
            Some("https://a.org")
        );
        assert_eq!(normalize_link("hello"), None);
        assert_eq!(normalize_link("two words.com"), None);
        assert_eq!(
            domain("https://www.artstation.com/artwork/1"),
            "artstation.com"
        );
        assert_eq!(domain("http://localhost:8000/a"), "localhost:8000");
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
