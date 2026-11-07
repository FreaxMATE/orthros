//! Map tiles, styles, sprites and glyphs for MapLibre. All map traffic goes
//! through here so the webview never talks to the network itself, and what
//! you have looked at is still there offline.
//!
//!   <root>/.cache/tiles/<hash of url>
//!
//! Only two hosts are allowed. OpenFreeMap answers are cached (fresh for 30
//! days, a stale copy is served when the network fails, the folder is kept
//! under 300 MB by deleting the oldest files). Esri World Imagery is never
//! written to disk: its terms do not allow offline use.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Once;
use std::time::{Duration, SystemTime};

use orthros_core::hash;

use crate::places::USER_AGENT;

const LIMIT: u64 = 8 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(15);
const MAX_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const CAP: u64 = 300 * 1024 * 1024;
/// How many cache writes between two eviction passes.
const EVICT_EVERY: usize = 500;

static WRITES: AtomicUsize = AtomicUsize::new(0);
static TMP: AtomicUsize = AtomicUsize::new(0);
static FIRST: Once = Once::new();

/// The bytes of `url`. An empty Vec means the tile does not exist (404 or
/// 204), which MapLibre treats as "nothing to draw".
pub fn fetch(root: &Path, url: &str) -> Result<Vec<u8>, String> {
    let url = url.trim();
    let cached = cacheable(url)?;
    let dir = root.join(".cache/tiles");
    let path = dir.join(hash(url));
    if cached {
        FIRST.call_once(|| evict(&dir));
        let modified = fs::metadata(&path).and_then(|m| m.modified());
        if modified.is_ok_and(|m| fresh(m, SystemTime::now())) {
            if let Ok(b) = fs::read(&path) {
                return Ok(b);
            }
        }
    }
    match download(url) {
        Ok(Some(bytes)) => {
            if cached {
                store(&dir, &path, &bytes);
            }
            Ok(bytes)
        }
        Ok(None) => Ok(Vec::new()),
        Err(e) => match cached.then(|| fs::read(&path)) {
            Some(Ok(stale)) => Ok(stale),
            _ => Err(e),
        },
    }
}

/// Whether `url` may be fetched at all (https and an allowed host), and
/// whether the answer may be cached.
fn cacheable(url: &str) -> Result<bool, String> {
    let rest = url
        .strip_prefix("https://")
        .ok_or("only https urls are allowed")?;
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    match host.to_ascii_lowercase().as_str() {
        "tiles.openfreemap.org" => Ok(true),
        "server.arcgisonline.com" => Ok(false),
        _ => Err(format!("host not allowed: {host}")),
    }
}

/// A fresh copy (None for a missing tile) or the reason it failed.
fn download(url: &str) -> Result<Option<Vec<u8>>, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .http_status_as_error(false)
        .build()
        .into();
    let mut res = agent
        .get(url)
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| e.to_string())?;
    let status = res.status();
    if status.as_u16() == 404 || status.as_u16() == 204 {
        return Ok(None);
    }
    if !status.is_success() {
        return Err(format!("http {}", status.as_u16()));
    }
    let mut body = Vec::new();
    res.body_mut()
        .as_reader()
        .take(LIMIT + 1)
        .read_to_end(&mut body)
        .map_err(|e| e.to_string())?;
    if body.len() as u64 > LIMIT {
        return Err("response too large".into());
    }
    Ok(Some(body))
}

/// Atomic (temp file, then rename), and best effort: the cache is only an
/// optimisation.
fn store(dir: &Path, path: &Path, bytes: &[u8]) {
    let tmp = dir.join(format!(
        "{}.{}.tmp",
        std::process::id(),
        TMP.fetch_add(1, Ordering::Relaxed)
    ));
    let ok = fs::create_dir_all(dir)
        .and_then(|_| fs::write(&tmp, bytes))
        .and_then(|_| fs::rename(&tmp, path))
        .is_ok();
    if !ok {
        let _ = fs::remove_file(&tmp);
        return;
    }
    if (WRITES.fetch_add(1, Ordering::Relaxed) + 1).is_multiple_of(EVICT_EVERY) {
        evict(dir);
    }
}

/// Within 30 days. A modification time in the future (clock changes) counts
/// as fresh.
fn fresh(modified: SystemTime, now: SystemTime) -> bool {
    match now.duration_since(modified) {
        Ok(age) => age < MAX_AGE,
        Err(_) => true,
    }
}

fn evict(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let files = entries
        .flatten()
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            m.is_file().then_some((e.path(), m.len(), m.modified().ok()?))
        })
        .collect();
    for p in evict_list(files, CAP) {
        let _ = fs::remove_file(p);
    }
}

/// The oldest files to delete so the rest fits in `cap` bytes.
fn evict_list(mut files: Vec<(PathBuf, u64, SystemTime)>, cap: u64) -> Vec<PathBuf> {
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    files.sort_by_key(|f| f.2);
    let mut out = Vec::new();
    for (path, size, _) in files {
        if total <= cap {
            break;
        }
        total -= size;
        out.push(path);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    #[test]
    fn allows_only_two_https_hosts() {
        assert_eq!(cacheable("https://tiles.openfreemap.org/planet/1/2/3.pbf"), Ok(true));
        assert_eq!(
            cacheable("https://tiles.openfreemap.org/styles/liberty"),
            Ok(true)
        );
        assert_eq!(
            cacheable("https://server.arcgisonline.com/ArcGIS/rest/services/x/MapServer/tile/1/2/3"),
            Ok(false)
        );
        for u in [
            "http://tiles.openfreemap.org/x",
            "https://example.com/x",
            "https://tiles.openfreemap.org.evil.com/x",
            "https://tiles.openfreemap.org@evil.com/x",
            "https://tiles.openfreemap.org:8443/x",
            "file:///etc/passwd",
            "ftp://tiles.openfreemap.org/x",
            "",
        ] {
            assert!(cacheable(u).is_err(), "{u}");
        }
    }

    #[test]
    fn freshness_is_thirty_days() {
        let day = 24 * 60 * 60;
        let now = at(100 * day);
        assert!(fresh(now, now));
        assert!(fresh(at(71 * day), now));
        assert!(!fresh(at(70 * day), now));
        assert!(!fresh(at(0), now));
        assert!(fresh(at(101 * day), now));
    }

    #[test]
    fn evicts_oldest_first_down_to_the_cap() {
        let f = |n: &str, size, t| (PathBuf::from(n), size, at(t));
        let files = vec![f("c", 40, 30), f("a", 40, 10), f("d", 40, 40), f("b", 40, 20)];
        assert_eq!(evict_list(files.clone(), 160), Vec::<PathBuf>::new());
        assert_eq!(evict_list(files.clone(), 100), vec![PathBuf::from("a"), PathBuf::from("b")]);
        assert_eq!(evict_list(files.clone(), 120), vec![PathBuf::from("a")]);
        assert_eq!(evict_list(files, 0).len(), 4);
        assert!(evict_list(Vec::new(), 0).is_empty());
    }

    #[test]
    fn serves_fresh_cache_and_refuses_other_hosts() {
        let root = std::env::temp_dir().join(format!(
            "orthros-tiles-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&root);
        assert!(fetch(&root, "https://example.com/t.pbf").is_err());
        assert!(!root.join(".cache").exists());

        let url = "https://tiles.openfreemap.org/planet/cached/1.pbf";
        let dir = root.join(".cache/tiles");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(hash(url)), b"tile").unwrap();
        assert_eq!(fetch(&root, url), Ok(b"tile".to_vec()));
        let _ = fs::remove_dir_all(&root);
    }
}
