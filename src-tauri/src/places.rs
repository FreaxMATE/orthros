//! Places for the map: a link in a note is a place when we can get
//! coordinates for it. Most map links carry them in the URL itself (no
//! network); short links are followed by hand, hop by hop; a link with only a
//! name is geocoded with Nominatim (at most one request per second).
//!
//!   <root>/.cache/places/<hash of url>.json   {"place": {...}} or {"place": null}
//!
//! Answers that needed the network are cached forever, including "not a
//! place". A failed request is never cached, so it is retried next time.

use std::fs;
use std::io::Read;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use orthros_core::hash;
use serde::{Deserialize, Serialize};

pub const USER_AGENT: &str = concat!(
    "Orthros/",
    env!("CARGO_PKG_VERSION"),
    " (personal notes app; https://github.com/FreaxMATE/orthros)"
);
const TIMEOUT: Duration = Duration::from_secs(10);
const MAX_HOPS: usize = 5;
const BODY_LIMIT: u64 = 256 * 1024;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Place {
    pub lat: f64,
    pub lng: f64,
    pub name: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Cached {
    place: Option<Place>,
}

/// Where a map link points, or None if it isn't a map link, has no usable
/// location, or the network failed (then nothing is cached).
pub fn resolve(root: &Path, url: &str) -> Option<Place> {
    let url = url.trim();
    if !is_map_url(url) {
        return None;
    }
    if let Some(c) = parse_coords(url) {
        return Some(place(c, parse_name(url)));
    }
    let cache = root
        .join(".cache/places")
        .join(format!("{}.json", hash(url)));
    if let Some(c) = fs::read(&cache)
        .ok()
        .and_then(|b| serde_json::from_slice::<Cached>(&b).ok())
    {
        return c.place;
    }
    let found = lookup(url).ok()?;
    if let Some(dir) = cache.parent() {
        // The cache is only an optimisation: failing to write it is fine.
        let _ = fs::create_dir_all(dir).and_then(|_| {
            fs::write(&cache, serde_json::to_vec(&Cached { place: found.clone() })?)
        });
    }
    found
}

fn place((lat, lng): (f64, f64), name: Option<String>) -> Place {
    Place { lat, lng, name }
}

/// The network part: Ok(None) is a definite "not a place", Err a failure.
fn lookup(url: &str) -> Result<Option<Place>, ()> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .http_status_as_error(false)
        .max_redirects(0)
        .max_redirects_will_error(false)
        .build()
        .into();
    let mut cur = url.to_string();
    let mut name = parse_name(&cur);
    for _ in 0..MAX_HOPS {
        if let Some(c) = parse_coords(&cur) {
            return Ok(Some(place(c, name)));
        }
        if !is_short_link(&cur) {
            break;
        }
        let res = agent
            .get(&cur)
            .header("User-Agent", USER_AGENT)
            .call()
            .map_err(|_| ())?;
        if !res.status().is_redirection() {
            break;
        }
        let next = res
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .and_then(|l| join_location(&cur, l));
        // Stop at anything that isn't a map link (Google's consent page).
        match next {
            Some(n) if is_map_url(&n) => {
                name = name.or_else(|| parse_name(&n));
                cur = n;
            }
            _ => break,
        }
    }
    if let Some(c) = parse_coords(&cur) {
        return Ok(Some(place(c, name)));
    }
    match name {
        Some(n) => Ok(geocode(&agent, &n)?.map(|c| place(c, Some(n)))),
        None => Ok(None),
    }
}

static LAST_GEOCODE: Mutex<Option<Instant>> = Mutex::new(None);

/// Nominatim's usage policy: at most one request per second. The lock is held
/// while waiting, so concurrent callers queue up.
pub(crate) fn throttle() {
    let mut last = LAST_GEOCODE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(t) = *last {
        let due = t + Duration::from_secs(1);
        let now = Instant::now();
        if due > now {
            std::thread::sleep(due - now);
        }
    }
    *last = Some(Instant::now());
}

fn geocode(agent: &ureq::Agent, name: &str) -> Result<Option<(f64, f64)>, ()> {
    throttle();
    let url = format!(
        "https://nominatim.openstreetmap.org/search?format=json&limit=1&q={}",
        percent_encode(name)
    );
    let mut res = agent
        .get(&url)
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(|_| ())?;
    if !res.status().is_success() {
        return Err(());
    }
    let mut body = Vec::new();
    res.body_mut()
        .as_reader()
        .take(BODY_LIMIT)
        .read_to_end(&mut body)
        .map_err(|_| ())?;
    Ok(parse_nominatim(&body))
}

/// The first hit of a Nominatim `format=json` answer (lat and lon are strings).
fn parse_nominatim(body: &[u8]) -> Option<(f64, f64)> {
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    let hit = v.as_array()?.first()?;
    let num = |k: &str| -> Option<f64> {
        let x = hit.get(k)?;
        x.as_f64().or_else(|| x.as_str()?.trim().parse().ok())
    };
    valid(num("lat")?, num("lon")?)
}

// ------------------------------------------------------------------ parsing

/// (host lowercase without port, path, query, fragment) of an http(s) URL.
fn split(url: &str) -> Option<(String, &str, &str, &str)> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let (rest, frag) = rest.split_once('#').unwrap_or((rest, ""));
    let (rest, query) = rest.split_once('?').unwrap_or((rest, ""));
    let (auth, path) = match rest.find('/') {
        Some(i) => rest.split_at(i),
        None => (rest, ""),
    };
    let host = auth.rsplit('@').next().unwrap_or(auth);
    let host = host.split(':').next().unwrap_or(host).to_ascii_lowercase();
    Some((host, path, query, frag))
}

/// Links worth looking at: Google Maps, goo.gl short links, OpenStreetMap,
/// Apple Maps and `geo:`. Everything else is not a place.
pub fn is_map_url(url: &str) -> bool {
    let url = url.trim();
    if geo_body(url).is_some() {
        return true;
    }
    let Some((host, path, _, _)) = split(url) else {
        return false;
    };
    let host = host.strip_prefix("www.").unwrap_or(&host);
    let in_maps = path == "/maps" || path.starts_with("/maps/");
    match host {
        "maps.app.goo.gl" | "openstreetmap.org" | "maps.apple.com" => true,
        "goo.gl" => in_maps,
        _ => {
            host.strip_prefix("maps.google.").is_some_and(is_google_tld)
                || (in_maps && host.strip_prefix("google.").is_some_and(is_google_tld))
        }
    }
}

/// The part after `geo:` (the scheme is case-insensitive).
fn geo_body(url: &str) -> Option<&str> {
    let head = url.get(..4)?;
    head.eq_ignore_ascii_case("geo:").then(|| &url[4..])
}

/// "com", "de", "co.uk", "com.au": a country domain, not `google.com.evil.net`.
fn is_google_tld(tld: &str) -> bool {
    let ok = |p: &str| (2..=3).contains(&p.len()) && p.bytes().all(|c| c.is_ascii_lowercase());
    match tld.split_once('.') {
        None => ok(tld),
        Some((a, b)) => matches!(a, "co" | "com") && ok(b) && b.len() == 2,
    }
}

fn is_short_link(url: &str) -> bool {
    match split(url) {
        Some((host, path, _, _)) => {
            host == "maps.app.goo.gl" || (host == "goo.gl" && path.starts_with("/maps"))
        }
        None => false,
    }
}

/// An absolute URL for a `Location` header value.
fn join_location(base: &str, loc: &str) -> Option<String> {
    let loc = loc.trim();
    if loc.starts_with("https://") || loc.starts_with("http://") {
        return Some(loc.to_string());
    }
    let scheme_end = base.find("://")? + 3;
    let origin_end = base[scheme_end..]
        .find(['/', '?', '#'])
        .map_or(base.len(), |i| scheme_end + i);
    if let Some(rest) = loc.strip_prefix("//") {
        return Some(format!("{}//{}", &base[..scheme_end - 2], rest));
    }
    loc.starts_with('/')
        .then(|| format!("{}{}", &base[..origin_end], loc))
}

fn valid(lat: f64, lng: f64) -> Option<(f64, f64)> {
    ((-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lng)).then_some((lat, lng))
}

/// "lat,lng" (spaces allowed, anything after a second comma ignored).
fn parse_pair(s: &str) -> Option<(f64, f64)> {
    let mut it = s.split(',');
    let lat = it.next()?.trim().parse::<f64>().ok()?;
    let lng = it.next()?.trim().parse::<f64>().ok()?;
    valid(lat, lng)
}

/// The leading run of characters that can be part of a decimal number.
fn number_prefix(s: &str) -> &str {
    let end = s
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
        .unwrap_or(s.len());
    &s[..end]
}

/// Coordinates written in the URL itself, or None.
pub fn parse_coords(url: &str) -> Option<(f64, f64)> {
    let url = url.trim();
    if !is_map_url(url) {
        return None;
    }
    if let Some(body) = geo_body(url) {
        let end = body.find([';', '?']).unwrap_or(body.len());
        return parse_pair(&body[..end]);
    }
    let (_, path, query, frag) = split(url)?;
    // Google: the place marker, which is more exact than the viewport.
    if let Some(i) = url.find("!3d") {
        let rest = &url[i + 3..];
        let lat = number_prefix(rest);
        if let Some(rest) = rest[lat.len()..].strip_prefix("!4d") {
            let lng = number_prefix(rest);
            if let Some(c) = lat
                .parse()
                .ok()
                .zip(lng.parse().ok())
                .and_then(|(a, b)| valid(a, b))
            {
                return Some(c);
            }
        }
    }
    // Google: the viewport centre, /@lat,lng,zoom.
    if let Some(i) = path.find("/@") {
        let seg = path[i + 2..].split('/').next().unwrap_or("");
        if let Some(c) = parse_pair(seg) {
            return Some(c);
        }
    }
    if let Some(c) = query_param(query, "mlat")
        .zip(query_param(query, "mlon"))
        .and_then(|(a, b)| valid(a.trim().parse().ok()?, b.trim().parse().ok()?))
    {
        return Some(c);
    }
    for key in ["q", "query", "ll", "center", "destination"] {
        if let Some(c) = query_param(query, key).and_then(|v| parse_pair(&v)) {
            return Some(c);
        }
    }
    // OpenStreetMap: #map=zoom/lat/lng
    let mut parts = frag.strip_prefix("map=")?.split('/');
    parts.next()?;
    let lat = parts.next()?.parse().ok()?;
    let lng = parts.next()?.parse().ok()?;
    valid(lat, lng)
}

/// A place name written in the URL: Google's `/place/<Name>/` or
/// `/search/<Name>`, or a `q=` / `query=` that is not a coordinate pair.
pub fn parse_name(url: &str) -> Option<String> {
    let url = url.trim();
    if !is_map_url(url) {
        return None;
    }
    let (_, path, query, _) = split(url)?;
    for marker in ["/place/", "/search/"] {
        if let Some(i) = path.find(marker) {
            let seg = path[i + marker.len()..].split('/').next().unwrap_or("");
            let name = percent_decode(seg);
            let name = name.trim();
            if !name.is_empty() && !name.starts_with('@') && parse_pair(name).is_none() {
                return Some(name.to_string());
            }
        }
    }
    for key in ["q", "query"] {
        if let Some(v) = query_param(query, key) {
            let v = v.trim();
            if !v.is_empty() && parse_pair(v).is_none() {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// The decoded value of the first `key=` in a query string.
fn query_param(query: &str, key: &str) -> Option<String> {
    query
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .find(|(k, _)| *k == key)
        .map(|(_, v)| percent_decode(v))
}

/// `%XX` escapes and `+` as space; bad escapes stay as they are.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let escaped = match (b[i], b.get(i + 1).and_then(|&c| hex(c)), b.get(i + 2).and_then(|&c| hex(c))) {
            (b'%', Some(h), Some(l)) => Some(h * 16 + l),
            _ => None,
        };
        match (escaped, b[i]) {
            (Some(byte), _) => {
                out.push(byte);
                i += 3;
                continue;
            }
            (None, b'+') => out.push(b' '),
            (None, c) => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub(crate) fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for &c in s.as_bytes() {
        if c.is_ascii_alphanumeric() || b"-._~".contains(&c) {
            out.push(c as char);
        } else {
            out.push_str(&format!("%{c:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const FUSHIMI: &str = "https://www.google.com/maps/place/Fushimi+Inari+Taisha/@34.9676945,135.7791876,17z/data=!3m1!4b1!4m6!3m5!1s0x60010f153d2e6d21:0x7b1aca1c753ae2e9!8m2!3d34.9676945!4d135.7791876!16zL20vMDJ0dDZ6";

    #[test]
    fn recognises_map_urls() {
        for u in [
            FUSHIMI,
            "https://maps.google.com/?q=48.8584,2.2945",
            "https://www.google.de/maps/@1,2,3z",
            "https://google.com/maps",
            "https://maps.app.goo.gl/AbCdEf",
            "https://goo.gl/maps/xyz",
            "https://www.openstreetmap.org/#map=15/38.7223/-9.1393",
            "https://openstreetmap.org/",
            "https://maps.apple.com/?ll=1,2",
            "geo:37.786971,-122.399677",
            "GEO:1,2",
        ] {
            assert!(is_map_url(u), "{u}");
        }
        for u in [
            "https://example.com/@1,2",
            "https://www.google.com/search?q=1,2",
            "https://goo.gl/abc",
            "https://google.com.evil.com/maps/@1,2",
            "https://notopenstreetmap.org/",
            "ftp://maps.google.com/",
            "file:///maps/",
            "mailto:a@b.c",
            "",
        ] {
            assert!(!is_map_url(u), "{u}");
        }
    }

    #[test]
    fn google_place_prefers_the_marker() {
        assert_eq!(parse_coords(FUSHIMI), Some((34.9676945, 135.7791876)));
        assert_eq!(parse_name(FUSHIMI).as_deref(), Some("Fushimi Inari Taisha"));
        // Without the marker the viewport centre is used.
        let u = "https://www.google.com/maps/place/Foo/@1.5,-2.5,17z";
        assert_eq!(parse_coords(u), Some((1.5, -2.5)));
    }

    #[test]
    fn google_query_forms() {
        assert_eq!(
            parse_coords("https://maps.google.com/?q=48.8584,2.2945"),
            Some((48.8584, 2.2945))
        );
        assert_eq!(parse_name("https://maps.google.com/?q=48.8584,2.2945"), None);
        let u = "https://www.google.com/maps/search/?api=1&query=47.5951518%2C-122.3316393";
        assert_eq!(parse_coords(u), Some((47.5951518, -122.3316393)));
        assert_eq!(parse_name(u), None);
        let u = "https://www.google.com/maps/dir/?api=1&destination=34.059808,-118.368152";
        assert_eq!(parse_coords(u), Some((34.059808, -118.368152)));
        let u = "https://www.google.com/maps?center=10,20&zoom=5";
        assert_eq!(parse_coords(u), Some((10.0, 20.0)));
        let u = "https://www.google.com/maps/@-33.8,151.2,12z";
        assert_eq!(parse_coords(u), Some((-33.8, 151.2)));
    }

    #[test]
    fn openstreetmap_forms() {
        let u = "https://www.openstreetmap.org/?mlat=59.9139&mlon=10.7522#map=12/59.9139/10.7522";
        assert_eq!(parse_coords(u), Some((59.9139, 10.7522)));
        let u = "https://www.openstreetmap.org/?mlat=1&mlon=2#map=12/50/60";
        assert_eq!(parse_coords(u), Some((1.0, 2.0)));
        let u = "https://www.openstreetmap.org/#map=15/38.7223/-9.1393";
        assert_eq!(parse_coords(u), Some((38.7223, -9.1393)));
        assert_eq!(parse_name(u), None);
        assert_eq!(parse_coords("https://www.openstreetmap.org/#map=15/x/y"), None);
        assert_eq!(parse_coords("https://www.openstreetmap.org/"), None);
    }

    #[test]
    fn geo_and_apple() {
        assert_eq!(
            parse_coords("geo:37.786971,-122.399677"),
            Some((37.786971, -122.399677))
        );
        assert_eq!(
            parse_coords("geo:48.2,16.3?z=11"),
            Some((48.2, 16.3))
        );
        assert_eq!(parse_coords("geo:1,2;u=35"), Some((1.0, 2.0)));
        assert_eq!(parse_coords("geo:abc"), None);
        let u = "https://maps.apple.com/?ll=50.894967,4.341626&q=Atomium";
        assert_eq!(parse_coords(u), Some((50.894967, 4.341626)));
        assert_eq!(parse_name(u).as_deref(), Some("Atomium"));
    }

    #[test]
    fn name_only_and_non_places() {
        let u = "https://www.google.com/maps/search/Kyoto+Station";
        assert!(is_map_url(u));
        assert_eq!(parse_coords(u), None);
        assert_eq!(parse_name(u).as_deref(), Some("Kyoto Station"));
        let u = "https://www.google.com/maps/search/Caf%C3%A9+de+Flore,+Paris/";
        assert_eq!(parse_name(u).as_deref(), Some("Café de Flore, Paris"));
        let u = "https://maps.google.com/?q=Eiffel%20Tower";
        assert_eq!(parse_name(u).as_deref(), Some("Eiffel Tower"));

        let u = "https://maps.app.goo.gl/AbCdEf";
        assert!(is_map_url(u));
        assert_eq!(parse_coords(u), None);
        assert_eq!(parse_name(u), None);

        let u = "https://example.com/@1,2";
        assert!(!is_map_url(u));
        assert_eq!(parse_coords(u), None);
        assert_eq!(parse_name(u), None);
    }

    #[test]
    fn rejects_out_of_range() {
        assert_eq!(parse_coords("geo:91,0"), None);
        assert_eq!(parse_coords("geo:0,181"), None);
        assert_eq!(parse_coords("geo:-90,-180"), Some((-90.0, -180.0)));
        assert_eq!(parse_coords("geo:NaN,0"), None);
        assert_eq!(parse_coords("https://maps.google.com/?q=100,200"), None);
        // An out-of-range marker falls back to the next form.
        let u = "https://www.google.com/maps/@1,2,3z/data=!3d99!4d0";
        assert_eq!(parse_coords(u), Some((1.0, 2.0)));
    }

    #[test]
    fn decodes_percent_and_plus() {
        assert_eq!(percent_decode("a+b%20c%2Cd"), "a b c,d");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz%4"), "%zz%4");
        assert_eq!(percent_decode("%E6%9D%B1%E4%BA%AC"), "東京");
        assert_eq!(percent_encode("Café de Flore"), "Caf%C3%A9%20de%20Flore");
    }

    #[test]
    fn redirect_walk_parses_every_hop() {
        // What a short link's Location headers look like, parse only.
        let hop1 = join_location(
            "https://maps.app.goo.gl/AbCdEf",
            "https://www.google.com/maps/place/Foo/@1,2,17z/data=!3d3!4d4",
        )
        .unwrap();
        assert!(is_map_url(&hop1));
        assert_eq!(parse_coords(&hop1), Some((3.0, 4.0)));
        assert_eq!(parse_name(&hop1).as_deref(), Some("Foo"));
        assert!(is_short_link("https://maps.app.goo.gl/AbCdEf"));
        assert!(is_short_link("https://goo.gl/maps/xyz"));
        assert!(!is_short_link(&hop1));
        // A consent page is not a map link, so the walk stops there.
        let consent = "https://consent.google.com/m?continue=https://www.google.com/maps/@1,2,3z";
        assert!(!is_map_url(consent));
    }

    #[test]
    fn joins_locations() {
        let base = "https://goo.gl/maps/xyz?a=1";
        assert_eq!(
            join_location(base, "/maps/place/X").as_deref(),
            Some("https://goo.gl/maps/place/X")
        );
        assert_eq!(
            join_location(base, "//www.google.com/maps").as_deref(),
            Some("https://www.google.com/maps")
        );
        assert_eq!(
            join_location(base, "http://x.test/y").as_deref(),
            Some("http://x.test/y")
        );
        assert_eq!(join_location(base, "relative/path"), None);
        assert_eq!(
            join_location("https://goo.gl", "/maps").as_deref(),
            Some("https://goo.gl/maps")
        );
    }

    #[test]
    fn reads_nominatim() {
        let body = br#"[{"place_id":1,"lat":"34.985","lon":"135.758","display_name":"Kyoto"}]"#;
        assert_eq!(parse_nominatim(body), Some((34.985, 135.758)));
        assert_eq!(parse_nominatim(b"[]"), None);
        assert_eq!(parse_nominatim(b"not json"), None);
        assert_eq!(parse_nominatim(br#"[{"lat":"x","lon":"1"}]"#), None);
        assert_eq!(parse_nominatim(br#"[{"lat":"95","lon":"1"}]"#), None);
    }

    #[test]
    fn resolves_pure_urls_and_serves_cache() {
        let root = std::env::temp_dir().join(format!(
            "orthros-places-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&root);
        assert_eq!(resolve(&root, "https://example.com/@1,2"), None);
        assert_eq!(
            resolve(&root, " geo:1,2 "),
            Some(Place { lat: 1.0, lng: 2.0, name: None })
        );
        // Neither touched the disk.
        assert!(!root.join(".cache").exists());

        let dir = root.join(".cache/places");
        fs::create_dir_all(&dir).unwrap();
        let hit = "https://maps.app.goo.gl/cached";
        let p = Place { lat: 5.0, lng: 6.0, name: Some("X".into()) };
        fs::write(
            dir.join(format!("{}.json", hash(hit))),
            serde_json::to_vec(&Cached { place: Some(p.clone()) }).unwrap(),
        )
        .unwrap();
        assert_eq!(resolve(&root, hit), Some(p));
        let miss = "https://maps.app.goo.gl/nothing";
        fs::write(dir.join(format!("{}.json", hash(miss))), br#"{"place": null}"#).unwrap();
        assert_eq!(resolve(&root, miss), None);
        let _ = fs::remove_dir_all(&root);
    }
}
