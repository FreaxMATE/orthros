//! Link previews for the cards under bare URLs: fetch the page, read its
//! Open Graph metadata, fetch the image, and cache the result on disk so a
//! card renders at once the next time (and offline).
//!
//!   <root>/.cache/links/<hash of url>.json
//!
//! A failed fetch is never cached, so it is retried the next time.

use std::fs;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

use orthros_core::hash;
use serde::{Deserialize, Serialize};

const PAGE_LIMIT: u64 = 1024 * 1024;
const IMAGE_LIMIT: u64 = 512 * 1024;
const TIMEOUT: Duration = Duration::from_secs(10);
const USER_AGENT: &str = "Mozilla/5.0 (compatible; Orthros link preview)";

#[derive(Serialize, Deserialize, Debug, Default, PartialEq)]
pub struct LinkPreview {
    pub url: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub site: Option<String>,
    /// A `data:` URL, so the card works offline and the page never loads
    /// anything from the remote site.
    pub image: Option<String>,
}

/// The cached or freshly fetched preview, or None if the page can't be
/// reached (or is not http/https).
pub fn preview(root: &Path, url: &str) -> Option<LinkPreview> {
    let url = url.trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return None;
    }
    let cache = root
        .join(".cache/links")
        .join(format!("{}.json", hash(url)));
    // A challenge cached by an older version is ignored, so it heals itself.
    if let Some(p) = fs::read(&cache)
        .ok()
        .and_then(|b| serde_json::from_slice::<LinkPreview>(&b).ok())
        .filter(|p| !is_challenge(p))
    {
        return Some(p);
    }
    let p = fetch(url)?;
    if let Some(dir) = cache.parent() {
        // The cache is only an optimisation: failing to write it is fine.
        let _ = fs::create_dir_all(dir).and_then(|_| fs::write(&cache, serde_json::to_vec(&p)?));
    }
    Some(p)
}

/// A bot check (Cloudflare, Anubis, Google...) instead of the page. Its title
/// is never shown: the card falls back to the site's name and icon.
fn is_challenge(p: &LinkPreview) -> bool {
    const SIGNS: [&str; 9] = [
        "just a moment",
        "attention required",
        "not a bot",
        "not a robot",
        "are you a robot",
        "are you human",
        "verify you are human",
        "captcha",
        "access denied",
    ];
    let title = p.title.as_deref().unwrap_or("").to_lowercase();
    SIGNS.iter().any(|s| title.contains(s))
}

/// The site's name and icon only, for a page that hides behind a bot check.
/// The icon may be behind it too; then the card has none.
fn site_card(agent: &ureq::Agent, url: &str) -> LinkPreview {
    let host = url.split("://").nth(1).and_then(|r| r.split(['/', '?', '#']).next()).unwrap_or(url);
    LinkPreview {
        url: url.to_string(),
        site: Some(host.trim_start_matches("www.").to_string()),
        image: resolve(url, "/favicon.ico").and_then(|icon| image_data_url(agent, &icon)),
        ..Default::default()
    }
}

fn fetch(url: &str) -> Option<LinkPreview> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .http_status_as_error(false)
        .build()
        .into();
    let (_, page) = get(&agent, url, PAGE_LIMIT, false)?;
    let meta = parse(&String::from_utf8_lossy(&page));
    if is_challenge(&LinkPreview { title: meta.title.clone(), ..Default::default() }) {
        return Some(site_card(&agent, url));
    }
    let image = meta
        .image
        .as_deref()
        .and_then(|href| resolve(url, href))
        .and_then(|img| image_data_url(&agent, &img));
    Some(LinkPreview {
        url: url.to_string(),
        title: meta.title,
        description: meta.description,
        site: meta.site,
        image,
    })
}

/// GET `url`; the content type (lowercase, no parameters) and at most
/// `limit` bytes of the body. With `exact`, a longer body is a failure
/// instead of being cut off.
fn get(agent: &ureq::Agent, url: &str, limit: u64, exact: bool) -> Option<(String, Vec<u8>)> {
    let mut res = agent
        .get(url)
        .header("User-Agent", USER_AGENT)
        .call()
        .ok()?;
    if !res.status().is_success() {
        return None;
    }
    let kind = res
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let kind = kind
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let mut body = Vec::new();
    res.body_mut()
        .as_reader()
        .take(limit + 1)
        .read_to_end(&mut body)
        .ok()?;
    if body.len() as u64 > limit {
        if exact {
            return None;
        }
        body.truncate(limit as usize);
    }
    Some((kind, body))
}

fn image_data_url(agent: &ureq::Agent, url: &str) -> Option<String> {
    let (kind, bytes) = get(agent, url, IMAGE_LIMIT, true)?;
    (kind.starts_with("image/") && !bytes.is_empty())
        .then(|| format!("data:{kind};base64,{}", base64(&bytes)))
}

pub(crate) fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = ((c[0] as u32) << 16)
            | ((*c.get(1).unwrap_or(&0) as u32) << 8)
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(A[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

// ------------------------------------------------------------------ parsing

#[derive(Debug, Default, PartialEq)]
struct Meta {
    title: Option<String>,
    description: Option<String>,
    site: Option<String>,
    image: Option<String>,
}

/// Pull the card fields out of an HTML page. Tolerant on purpose: real pages
/// are messy, so this scans for `<meta>` and `<title>` tags instead of
/// building a tree, and the first value for a key wins.
fn parse(html: &str) -> Meta {
    let lower = html.to_ascii_lowercase(); // same byte offsets as `html`
    let mut metas: Vec<(String, String)> = Vec::new(); // (property or name, content)
    let mut page_title = None;
    let mut i = 0;
    while let Some(off) = lower[i..].find('<') {
        let at = i + off + 1;
        let rest = &lower[at..];
        if rest.starts_with("!--") {
            i = rest.find("-->").map_or(lower.len(), |e| at + e + 3);
        } else if rest.starts_with("/head") || rest.starts_with("body") {
            break;
        } else if rest.starts_with("meta")
            && rest[4..].starts_with(|c: char| c.is_ascii_whitespace() || c == '/')
        {
            let (attrs, end) = attributes(html, at + 4);
            let get = |k: &str| attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
            if let (Some(key), Some(content)) = (get("property").or(get("name")), get("content")) {
                metas.push((
                    key.trim().to_ascii_lowercase(),
                    decode(content).trim().to_string(),
                ));
            }
            i = end;
        } else if page_title.is_none()
            && rest.starts_with("title")
            && rest[5..].starts_with(|c: char| c.is_ascii_whitespace() || c == '>')
        {
            let start = lower[at..].find('>').map_or(lower.len(), |e| at + e + 1);
            let end = lower[start..]
                .find("</title")
                .map_or(lower.len(), |e| start + e);
            page_title = Some(
                decode(&html[start..end])
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" "),
            );
            i = end;
        } else {
            i = at;
        }
    }
    let pick = |keys: &[&str]| {
        keys.iter().find_map(|k| {
            metas
                .iter()
                .find(|(n, v)| n.as_str() == *k && !v.is_empty())
                .map(|(_, v)| v.clone())
        })
    };
    Meta {
        title: pick(&["og:title", "twitter:title"])
            .or(page_title)
            .filter(|t| !t.is_empty()),
        description: pick(&["og:description", "twitter:description", "description"]),
        site: pick(&["og:site_name"]),
        image: pick(&["og:image", "og:image:url", "twitter:image"]),
    }
}

/// Attributes of the tag whose name ended at byte `from`: lowercase names,
/// raw values (single, double or no quotes, any order). Returns them and
/// the offset just past the closing `>`.
fn attributes(html: &str, from: usize) -> (Vec<(String, String)>, usize) {
    let b = html.as_bytes();
    let mut attrs = Vec::new();
    let mut i = from;
    loop {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'/') {
            i += 1;
        }
        if i >= b.len() || b[i] == b'>' {
            return (attrs, (i + 1).min(b.len()));
        }
        let start = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && !matches!(b[i], b'=' | b'>' | b'/') {
            i += 1;
        }
        let name = html[start..i].to_ascii_lowercase();
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut value = "";
        if i < b.len() && b[i] == b'=' {
            i += 1;
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
                let q = b[i];
                let start = i + 1;
                i = start;
                while i < b.len() && b[i] != q {
                    i += 1;
                }
                value = &html[start..i];
                i = (i + 1).min(b.len());
            } else {
                let start = i;
                while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'>' {
                    i += 1;
                }
                value = &html[start..i];
            }
        }
        if !name.is_empty() {
            attrs.push((name, value.to_string()));
        }
    }
}

/// The few entities that show up in titles: the named basics and numeric
/// ones. Anything else is left as it is.
fn decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let decoded = rest[1..].find(';').filter(|&e| e <= 10).and_then(|e| {
            let ent = &rest[1..1 + e];
            let ch = match ent {
                "amp" => '&',
                "lt" => '<',
                "gt" => '>',
                "quot" => '"',
                "apos" => '\'',
                "nbsp" => ' ',
                _ => {
                    let num = ent.strip_prefix('#')?;
                    let code = match num.strip_prefix(['x', 'X']) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                        None => num.parse().ok()?,
                    };
                    char::from_u32(code)?
                }
            };
            Some((ch, e + 2))
        });
        match decoded {
            Some((ch, len)) => {
                out.push(ch);
                rest = &rest[len..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// An image href made absolute against the page URL (no url crate needed
/// for the handful of forms pages use). None if it isn't http/https.
fn resolve(base: &str, href: &str) -> Option<String> {
    let href = href.trim();
    let scheme_end = base.find("://")?;
    let scheme = &base[..scheme_end];
    let after = scheme_end + 3;
    let auth_end = base[after..]
        .find(['/', '?', '#'])
        .map_or(base.len(), |i| after + i);
    let origin = &base[..auth_end];
    let abs = if href.starts_with("http://") || href.starts_with("https://") {
        href.to_string()
    } else if let Some(r) = href.strip_prefix("//") {
        format!("{scheme}://{r}")
    } else if href.starts_with('/') {
        format!("{origin}{href}")
    } else if href.is_empty()
        || href.starts_with('#')
        || href
            .find(':')
            .is_some_and(|c| href.find('/').is_none_or(|s| c < s))
    {
        return None; // empty, fragment, or another scheme (data:, javascript:)
    } else {
        let path = &base[auth_end..];
        let path = &path[..path.find(['?', '#']).unwrap_or(path.len())];
        let dir = &path[..path.rfind('/').map_or(0, |i| i + 1)];
        format!("{origin}{}{href}", if dir.is_empty() { "/" } else { dir })
    };
    Some(abs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_open_graph() {
        let html = r#"<!doctype html><html><head>
            <title>Fallback title</title>
            <meta name="description" content="plain description">
            <META PROPERTY='og:title' CONTENT='Tom &amp; Jerry&#39;s &quot;Best&quot; &#x41;'>
            <meta content="A site" property="og:site_name" />
            <meta property=og:description content="Long &lt;story&gt;">
            <meta property="og:image" content="/img/card.png">
            <meta property="og:title" content="Second title ignored">
            </head><body><meta property="og:site_name" content="in body"></body></html>"#;
        assert_eq!(
            parse(html),
            Meta {
                title: Some("Tom & Jerry's \"Best\" A".into()),
                description: Some("Long <story>".into()),
                site: Some("A site".into()),
                image: Some("/img/card.png".into()),
            }
        );
    }

    #[test]
    fn falls_back_to_title_and_description() {
        let html = "<html><head><title>\n  Hello\n   world </title><meta name=description content='just words'><meta property='og:title' content='  '></head>";
        let m = parse(html);
        assert_eq!(m.title.as_deref(), Some("Hello world"));
        assert_eq!(m.description.as_deref(), Some("just words"));
        assert_eq!((m.site, m.image), (None, None));
        assert_eq!(parse("not html at all"), Meta::default());
        assert_eq!(
            parse("<meta property=\"og:title\" content=\"unterminated"),
            Meta {
                title: Some("unterminated".into()),
                ..Default::default()
            }
        );
    }

    #[test]
    fn ignores_commented_out_tags_and_non_ascii() {
        let html = "<head><!-- <meta property=\"og:title\" content=\"old\"> --><title>Ünïcode İstanbul</title><meta name=\"twitter:image\" content=\"x.png\"></head>";
        let m = parse(html);
        assert_eq!(m.title.as_deref(), Some("Ünïcode İstanbul"));
        assert_eq!(m.image.as_deref(), Some("x.png"));
    }

    #[test]
    fn decodes_entities() {
        assert_eq!(
            decode("a &amp; b &lt;c&gt; &quot;d&quot; &#39;e&#39; &#65;&#x42;&nbsp;z"),
            "a & b <c> \"d\" 'e' AB z"
        );
        assert_eq!(
            decode("fish & chips &unknown; &#xZZ; &"),
            "fish & chips &unknown; &#xZZ; &"
        );
    }

    #[test]
    fn resolves_image_urls() {
        let base = "https://example.com/blog/post?id=1#top";
        assert_eq!(
            resolve(base, "https://cdn.x/a.png").as_deref(),
            Some("https://cdn.x/a.png")
        );
        assert_eq!(
            resolve(base, "//cdn.x/a.png").as_deref(),
            Some("https://cdn.x/a.png")
        );
        assert_eq!(
            resolve(base, "/a.png").as_deref(),
            Some("https://example.com/a.png")
        );
        assert_eq!(
            resolve(base, "a.png").as_deref(),
            Some("https://example.com/blog/a.png")
        );
        assert_eq!(
            resolve("https://example.com", "a.png").as_deref(),
            Some("https://example.com/a.png")
        );
        assert_eq!(
            resolve("https://example.com?q=1", "/a.png").as_deref(),
            Some("https://example.com/a.png")
        );
        assert_eq!(resolve(base, "data:image/png;base64,AAAA"), None);
        assert_eq!(resolve(base, ""), None);
    }

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(&[0xfb, 0xff]), "+/8=");
    }

    #[test]
    fn serves_from_cache_and_refuses_other_schemes() {
        let root = std::env::temp_dir().join(format!(
            "orthros-links-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&root);
        assert_eq!(preview(&root, "ftp://example.com/x"), None);
        assert_eq!(preview(&root, "file:///etc/passwd"), None);

        let url = "https://cached.invalid/page";
        let p = LinkPreview {
            url: url.into(),
            title: Some("T".into()),
            ..Default::default()
        };
        fs::create_dir_all(root.join(".cache/links")).unwrap();
        fs::write(
            root.join(".cache/links")
                .join(format!("{}.json", hash(url))),
            serde_json::to_vec(&p).unwrap(),
        )
        .unwrap();
        assert_eq!(preview(&root, url), Some(p));
    }

    #[test]
    fn bot_checks_are_not_previews() {
        let p = |t: &str| LinkPreview { title: Some(t.into()), ..Default::default() };
        assert!(is_challenge(&p("Just a moment...")));
        assert!(is_challenge(&p("Please make sure you are not a bot")));
        assert!(!is_challenge(&p("Visit Portugal")));
        assert!(!is_challenge(&LinkPreview::default()));
    }
}
