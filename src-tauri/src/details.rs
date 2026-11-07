//! Details of a place for the map: what it is, where exactly, when it is
//! open, a photo and a short description. OpenStreetMap (Overpass) knows the
//! place, Wikipedia (found through the `wikipedia` or `wikidata` tag) tells
//! its story.
//!
//!   <root>/.cache/details/<hash of "lat,lng,name,lang">.json
//!
//! Coordinates are rounded to 5 decimals (about a metre) in the key. Answers
//! are fresh for 30 days, including "nothing found" (stored as empty details,
//! shown as no details), so a lookup is not repeated on every click. If the
//! network fails, a stale answer is better than none; with no answer at all
//! nothing is cached and the next click tries again.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use orthros_core::hash;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::places::{percent_encode, throttle, USER_AGENT};

const TIMEOUT: Duration = Duration::from_secs(10);
const JSON_LIMIT: u64 = 2 * 1024 * 1024;
const IMAGE_LIMIT: u64 = 512 * 1024;
const TTL_SECS: u64 = 30 * 24 * 60 * 60;
/// Overpass search radius around the pin.
const SEARCH_RADIUS: u32 = 80;
/// Without a matching name, only a place this close to the pin counts.
const NEAR_METERS: f64 = 40.0;
const DESCRIPTION_MAX: usize = 400;

/// OSM keys whose value says what a place is, in order of preference.
const KIND_KEYS: [&str; 7] = [
    "tourism", "amenity", "shop", "historic", "leisure", "office", "craft",
];
/// `building=` values that are a place of worship even without `amenity`.
const WORSHIP_BUILDINGS: [&str; 7] = [
    "church",
    "chapel",
    "cathedral",
    "mosque",
    "synagogue",
    "temple",
    "shrine",
];

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Details {
    pub name: Option<String>,
    /// What it is, e.g. "Shinto shrine", "Cafe", "Museum".
    pub kind: Option<String>,
    /// One line, e.g. "Rua Augusta 24, 1100-053 Lisboa".
    pub address: Option<String>,
    /// The raw OSM `opening_hours` value, shown as it is.
    pub opening_hours: Option<String>,
    pub website: Option<String>,
    pub phone: Option<String>,
    /// The start of the Wikipedia article, cut at a sentence end.
    pub description: Option<String>,
    /// URL of the Wikipedia article.
    pub wikipedia: Option<String>,
    /// A `data:` URL, so the page never loads anything from the remote site.
    pub photo: Option<String>,
    /// `https://www.openstreetmap.org/<type>/<id>`
    pub osm: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Cached {
    /// Unix seconds.
    fetched: u64,
    details: Details,
}

type Tags = BTreeMap<String, String>;

/// An OSM object near the pin (Overpass) or the one Nominatim found.
#[derive(Debug, Clone, PartialEq)]
struct Element {
    osm_type: String,
    id: i64,
    lat: f64,
    lng: f64,
    tags: Tags,
}

/// The details of the place at `lat`,`lng` (`name` is what the link called
/// it, if anything), or None if nothing is known there or nothing could be
/// fetched and nothing is cached. `lang` picks names and the Wikipedia
/// language.
pub fn lookup(root: &Path, lat: f64, lng: f64, name: Option<&str>, lang: &str) -> Option<Details> {
    if !((-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lng)) {
        return None;
    }
    let name = name.map(str::trim).filter(|n| !n.is_empty());
    let cache = cache_path(root, lat, lng, name, lang);
    let cached = fs::read(&cache)
        .ok()
        .and_then(|b| serde_json::from_slice::<Cached>(&b).ok());
    if let Some(c) = cached.as_ref().filter(|c| fresh(c.fetched, unix_now())) {
        return non_empty(c.details.clone());
    }
    match fetch(lat, lng, name, lang) {
        Ok((details, complete)) => {
            // An answer that lacks the Wikipedia part only because that
            // request failed is shown, but not kept for 30 days.
            if complete {
                if let Some(dir) = cache.parent() {
                    // The cache is only an optimisation: failing to write it is fine.
                    let _ = fs::create_dir_all(dir).and_then(|_| {
                        let c = Cached {
                            fetched: unix_now(),
                            details: details.clone(),
                        };
                        fs::write(&cache, serde_json::to_vec(&c)?)
                    });
                }
            }
            non_empty(details)
        }
        Err(()) => cached.and_then(|c| non_empty(c.details)),
    }
}

/// Empty details are the cached form of "nothing found".
fn non_empty(d: Details) -> Option<Details> {
    (d != Details::default()).then_some(d)
}

fn cache_key(lat: f64, lng: f64, name: Option<&str>, lang: &str) -> String {
    format!("{lat:.5},{lng:.5},{},{lang}", name.unwrap_or(""))
}

fn cache_path(root: &Path, lat: f64, lng: f64, name: Option<&str>, lang: &str) -> PathBuf {
    root.join(".cache/details")
        .join(format!("{}.json", hash(&cache_key(lat, lng, name, lang))))
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// An entry from the future (the clock was set back) counts as stale.
fn fresh(fetched: u64, now: u64) -> bool {
    now >= fetched && now - fetched < TTL_SECS
}

/// The network part. Ok((details, complete)): empty details are a definite
/// "nothing here"; `complete` is false when a Wikipedia request failed, so
/// the answer should not be kept. Err: the primary lookup failed.
fn fetch(lat: f64, lng: f64, name: Option<&str>, lang: &str) -> Result<(Details, bool), ()> {
    // With redirects: Wikipedia answers redirect pages and Commons
    // `Special:FilePath` with a redirect to the file.
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .http_status_as_error(false)
        .build()
        .into();
    let elements = match overpass(&agent, lat, lng) {
        Ok(e) => e,
        // Overpass is often overloaded: Nominatim knows the same tags.
        Err(()) => reverse(&agent, lat, lng, lang)?.into_iter().collect(),
    };
    let Some(el) = choose(&elements, lat, lng, name, lang) else {
        return Ok((Details::default(), true));
    };
    let mut details = from_tags(&el.tags, lang, osm_url(el));
    let complete = enrich(&agent, &el.tags, lang, &mut details);
    Ok((details, complete))
}

/// GET `url`; the content type (lowercase, no parameters) and the body, which
/// must fit in `limit` bytes. Ok(None) is a 404, any other failure an Err.
fn get(agent: &ureq::Agent, url: &str, limit: u64) -> Result<Option<(String, Vec<u8>)>, ()> {
    let mut res = agent
        .get(url)
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(|_| ())?;
    if res.status().as_u16() == 404 {
        return Ok(None);
    }
    if !res.status().is_success() {
        return Err(());
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
        .map_err(|_| ())?;
    if body.len() as u64 > limit {
        return Err(());
    }
    Ok(Some((kind, body)))
}

fn get_json(agent: &ureq::Agent, url: &str) -> Result<Option<Value>, ()> {
    match get(agent, url, JSON_LIMIT)? {
        Some((_, body)) => serde_json::from_slice(&body).map(Some).map_err(|_| ()),
        None => Ok(None),
    }
}

// ----------------------------------------------------------------- overpass

fn overpass(agent: &ureq::Agent, lat: f64, lng: f64) -> Result<Vec<Element>, ()> {
    let query = format!(
        "[out:json][timeout:10];nwr(around:{SEARCH_RADIUS},{lat:.6},{lng:.6})[name];out tags center 30;"
    );
    let url = format!(
        "https://overpass-api.de/api/interpreter?data={}",
        percent_encode(&query)
    );
    let (_, body) = get(agent, &url, JSON_LIMIT)?.ok_or(())?;
    parse_overpass(&body).ok_or(())
}

/// The elements of an Overpass `out:json` answer. None for an answer that is
/// not one, or an empty one with a `remark` (Overpass reports a timeout or an
/// overload that way, with status 200).
fn parse_overpass(body: &[u8]) -> Option<Vec<Element>> {
    let v: Value = serde_json::from_slice(body).ok()?;
    let items = v.get("elements")?.as_array()?;
    if items.is_empty() && v.get("remark").is_some() {
        return None;
    }
    Some(items.iter().filter_map(element).collect())
}

/// Nodes carry lat/lon, ways and relations a `center` (from `out center`).
fn element(e: &Value) -> Option<Element> {
    let at = e.get("center").unwrap_or(e);
    let mut tags = Tags::new();
    collect_strings(e.get("tags"), &mut tags);
    Some(Element {
        osm_type: str_at(e, "type")?.to_string(),
        id: e.get("id")?.as_i64()?,
        lat: at.get("lat")?.as_f64()?,
        lng: at.get("lon")?.as_f64()?,
        tags,
    })
}

fn str_at<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key)?.as_str()
}

/// The string values of a JSON object, as tags.
fn collect_strings(v: Option<&Value>, tags: &mut Tags) {
    if let Some(o) = v.and_then(Value::as_object) {
        for (k, x) in o {
            if let Some(s) = x.as_str() {
                tags.insert(k.clone(), s.to_string());
            }
        }
    }
}

// ---------------------------------------------------------------- nominatim

/// The fallback when Overpass fails: Nominatim reverse geocoding at building
/// level, with the OSM tags. Ok(None): it found nothing.
fn reverse(agent: &ureq::Agent, lat: f64, lng: f64, lang: &str) -> Result<Option<Element>, ()> {
    throttle();
    let url = format!(
        "https://nominatim.openstreetmap.org/reverse?format=jsonv2&lat={lat:.6}&lon={lng:.6}\
         &zoom=18&extratags=1&namedetails=1&accept-language={}",
        percent_encode(lang)
    );
    let (_, body) = get(agent, &url, JSON_LIMIT)?.ok_or(())?;
    Ok(parse_reverse(&body))
}

/// A Nominatim `jsonv2` reverse answer as an element, with the tags the
/// Overpass path would have seen: `extratags`, the names, the main tag
/// (`category` = `type`) and the address parts as `addr:*`. None for
/// `{"error": ...}`, which is how Nominatim says "nothing here".
fn parse_reverse(body: &[u8]) -> Option<Element> {
    let v: Value = serde_json::from_slice(body).ok()?;
    let num = |k: &str| -> Option<f64> {
        let x = v.get(k)?;
        x.as_f64().or_else(|| x.as_str()?.trim().parse().ok())
    };
    let mut tags = Tags::new();
    collect_strings(v.get("extratags"), &mut tags);
    collect_strings(v.get("namedetails"), &mut tags);
    if let Some(n) = str_at(&v, "name").filter(|n| !n.is_empty()) {
        tags.entry("name".to_string())
            .or_insert_with(|| n.to_string());
    }
    let main = str_at(&v, "category").zip(str_at(&v, "type"));
    if let Some((c, t)) = main.filter(|(c, _)| KIND_KEYS.contains(c) || *c == "building") {
        tags.entry(c.to_string()).or_insert_with(|| t.to_string());
    }
    if let Some(a) = v.get("address") {
        let parts = [
            ("road", "addr:street"),
            ("house_number", "addr:housenumber"),
            ("postcode", "addr:postcode"),
        ];
        for (from, to) in parts {
            if let Some(s) = str_at(a, from) {
                tags.entry(to.to_string()).or_insert_with(|| s.to_string());
            }
        }
        let city = ["city", "town", "village", "municipality"]
            .iter()
            .find_map(|k| str_at(a, k));
        if let Some(c) = city {
            tags.entry("addr:city".to_string())
                .or_insert_with(|| c.to_string());
        }
    }
    Some(Element {
        osm_type: str_at(&v, "osm_type")?.to_string(),
        id: v.get("osm_id")?.as_i64()?,
        lat: num("lat")?,
        lng: num("lon")?,
        tags,
    })
}

// ---------------------------------------------------------------- selecting

/// Metres between two points on the globe.
fn haversine(lat1: f64, lng1: f64, lat2: f64, lng2: f64) -> f64 {
    const EARTH_RADIUS: f64 = 6_371_000.0;
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let dphi = (lat2 - lat1).to_radians();
    let dlambda = (lng2 - lng1).to_radians();
    let a = (dphi / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dlambda / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS * a.sqrt().asin()
}

fn norm(s: &str) -> String {
    s.trim().to_lowercase()
}

/// The primary language subtag as Wikipedia knows it ("pt-BR" is "pt"); "en"
/// for anything that does not look like one, since it ends up in a host name.
fn short_lang(lang: &str) -> String {
    let primary = lang
        .trim()
        .split(['-', '_'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if (2..=3).contains(&primary.len()) && primary.bytes().all(|b| b.is_ascii_lowercase()) {
        primary
    } else {
        "en".to_string()
    }
}

/// Every name an element goes by, lowercase.
fn names(tags: &Tags, lang: &str) -> Vec<String> {
    let l = short_lang(lang);
    let local = format!("name:{l}");
    ["name", "name:en", local.as_str(), "int_name"]
        .iter()
        .filter_map(|k| tag(tags, k))
        .map(norm)
        .collect()
}

/// 0 for the same name, 1 when one contains the other (a link says
/// "Fushimi Inari", OSM "Fushimi Inari Taisha"), None otherwise. Very short
/// names only match as a whole, or "a" would match everything.
fn name_rank(e: &Element, wanted: &str, lang: &str) -> Option<u8> {
    let names = names(&e.tags, lang);
    if names.iter().any(|n| n == wanted) {
        return Some(0);
    }
    names
        .iter()
        .any(|n| n.len().min(wanted.len()) >= 3 && (n.contains(wanted) || wanted.contains(n)))
        .then_some(1)
}

/// An element that is a place to visit, as opposed to a named street or
/// bus stop that just happens to be nearby.
fn is_place(e: &Element) -> bool {
    KIND_KEYS.iter().any(|k| tag(&e.tags, k).is_some())
        || tag(&e.tags, "building").is_some_and(|b| WORSHIP_BUILDINGS.contains(&b))
}

/// The element for the pin: the best name match (any distance within the
/// search radius, the closest on a tie), else the nearest place within
/// 40 m, else none.
fn choose<'a>(
    elements: &'a [Element],
    lat: f64,
    lng: f64,
    name: Option<&str>,
    lang: &str,
) -> Option<&'a Element> {
    let dist = |e: &Element| haversine(lat, lng, e.lat, e.lng);
    if let Some(wanted) = name.map(norm).filter(|n| !n.is_empty()) {
        let best = elements
            .iter()
            .filter_map(|e| name_rank(e, &wanted, lang).map(|r| (r, dist(e), e)))
            .min_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
        if let Some((_, _, e)) = best {
            return Some(e);
        }
    }
    elements
        .iter()
        .filter(|e| is_place(e))
        .map(|e| (dist(e), e))
        .filter(|(d, _)| *d <= NEAR_METERS)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, e)| e)
}

fn osm_url(e: &Element) -> String {
    format!("https://www.openstreetmap.org/{}/{}", e.osm_type, e.id)
}

// ------------------------------------------------------------------ mapping

/// A trimmed, non-empty tag value.
fn tag<'a>(tags: &'a Tags, key: &str) -> Option<&'a str> {
    tags.get(key)
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
}

fn from_tags(tags: &Tags, lang: &str, osm: String) -> Details {
    Details {
        name: name_of(tags, lang),
        kind: kind_of(tags),
        address: address_of(tags),
        opening_hours: tag(tags, "opening_hours").map(str::to_string),
        website: ["website", "contact:website", "url"]
            .iter()
            .find_map(|k| tag(tags, k).and_then(normalize_url)),
        phone: ["phone", "contact:phone"].iter().find_map(|k| {
            // Several numbers are separated by ';': the first is enough.
            let first = tag(tags, k)?.split(';').next()?.trim();
            (!first.is_empty()).then(|| first.to_string())
        }),
        osm: Some(osm),
        ..Details::default()
    }
}

/// The name in the user's language if OSM has one, else the local name.
fn name_of(tags: &Tags, lang: &str) -> Option<String> {
    tag(tags, &format!("name:{}", short_lang(lang)))
        .or_else(|| tag(tags, "name"))
        .map(str::to_string)
}

fn humanize(value: &str) -> String {
    let s = value.replace('_', " ");
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn worship_kind(religion: &str) -> Option<&'static str> {
    match religion.to_ascii_lowercase().as_str() {
        "shinto" => Some("Shinto shrine"),
        "buddhist" => Some("Buddhist temple"),
        "christian" => Some("Church"),
        "muslim" => Some("Mosque"),
        "jewish" => Some("Synagogue"),
        "hindu" => Some("Hindu temple"),
        _ => None,
    }
}

/// What the place is: the first of the kind keys that has a real value
/// (`shop=yes` says nothing), made readable.
fn kind_of(tags: &Tags) -> Option<String> {
    for key in KIND_KEYS {
        // A value can be a list: `cafe;restaurant`.
        let value = tag(tags, key)
            .and_then(|v| v.split(';').next())
            .map(str::trim)
            .filter(|v| !v.is_empty() && *v != "yes");
        let Some(value) = value else {
            continue;
        };
        if value == "place_of_worship" {
            if let Some(kind) = tag(tags, "religion").and_then(worship_kind) {
                return Some(kind.to_string());
            }
        }
        return Some(humanize(value));
    }
    tag(tags, "building")
        .filter(|b| WORSHIP_BUILDINGS.contains(b))
        .map(humanize)
}

/// "Rua Augusta 24, 1100-053 Lisboa": the street line, then postcode and city.
fn address_of(tags: &Tags) -> Option<String> {
    let street = tag(tags, "addr:street").or_else(|| tag(tags, "addr:place"));
    let line1 = match (street, tag(tags, "addr:housenumber")) {
        (Some(s), Some(n)) => Some(format!("{s} {n}")),
        (Some(s), None) => Some(s.to_string()),
        // A house number alone says nothing.
        (None, _) => None,
    };
    let line2 = match (tag(tags, "addr:postcode"), tag(tags, "addr:city")) {
        (Some(p), Some(c)) => Some(format!("{p} {c}")),
        (Some(x), None) | (None, Some(x)) => Some(x.to_string()),
        (None, None) => None,
    };
    let parts: Vec<String> = [line1, line2].into_iter().flatten().collect();
    (!parts.is_empty()).then(|| parts.join(", "))
}

/// A web address that is safe to open: http(s) as written, or a bare host
/// (`example.com/menu`) made https. Other schemes (`javascript:`, `mailto:`)
/// and anything with spaces are dropped.
fn normalize_url(raw: &str) -> Option<String> {
    let s = raw.trim();
    let lower = s.to_ascii_lowercase();
    if s.is_empty() || s.contains(char::is_whitespace) {
        return None;
    }
    if lower.starts_with("https://") || lower.starts_with("http://") {
        return (s.len() > lower.find("//")? + 2).then(|| s.to_string());
    }
    let schemeless = !s.contains("://")
        && !["javascript:", "mailto:", "tel:", "data:", "file:"]
            .iter()
            .any(|p| lower.starts_with(p));
    schemeless.then(|| format!("https://{s}"))
}

// ---------------------------------------------------------------- wikipedia

#[derive(Debug, Default, PartialEq)]
struct WikiData {
    /// (language, title) of the best article: the user's language, else English.
    article: Option<(String, String)>,
    /// P18, a file name on Wikimedia Commons.
    image: Option<String>,
}

#[derive(Debug, PartialEq)]
struct Summary {
    extract: Option<String>,
    page: Option<String>,
    image: Option<String>,
}

/// Add the Wikipedia description, article link and photo to `d`. True if
/// every request that was made worked (a 404 is an answer); a failed photo
/// download does not count, images are too often just too big.
fn enrich(agent: &ureq::Agent, tags: &Tags, lang: &str, d: &mut Details) -> bool {
    let l = short_lang(lang);
    let mut complete = true;
    let tag_ref = tag(tags, "wikipedia").and_then(parse_wikipedia_tag);
    let mut wd = WikiData::default();
    if let Some(q) = tag(tags, "wikidata").filter(|q| is_qid(q)) {
        let url = format!("https://www.wikidata.org/wiki/Special:EntityData/{q}.json");
        match get_json(agent, &url) {
            Ok(Some(v)) => wd = parse_wikidata(&v, q, &l),
            Ok(None) => {}
            Err(()) => complete = false,
        }
    }
    // The tag's article if it is in the user's language, else look for that
    // language through Wikidata (which falls back to English).
    let article = match tag_ref {
        Some((al, title)) if al == l => Some((al, title)),
        other => wd.article.or(other),
    };
    let mut thumb = None;
    if let Some((al, title)) = &article {
        let url = format!(
            "https://{al}.wikipedia.org/api/rest_v1/page/summary/{}",
            percent_encode(title)
        );
        match get_json(agent, &url) {
            Ok(Some(v)) => {
                if let Some(s) = parse_summary(&v) {
                    d.wikipedia = s.page.or_else(|| Some(article_url(al, title)));
                    d.description = s.extract;
                    thumb = s.image;
                }
            }
            Ok(None) => {}
            Err(()) => complete = false,
        }
    }
    let commons = wd.image.map(|f| {
        format!(
            "https://commons.wikimedia.org/wiki/Special:FilePath/{}?width=640",
            percent_encode(&f)
        )
    });
    d.photo = thumb
        .into_iter()
        .chain(commons)
        .find_map(|u| image_data_url(agent, &u));
    complete
}

/// "de:Rua Augusta" as (language, title); the `#section` is dropped.
fn parse_wikipedia_tag(t: &str) -> Option<(String, String)> {
    let (lang, title) = t.split_once(':')?;
    let lang = lang.trim().to_ascii_lowercase();
    let title = title.split('#').next().unwrap_or("").trim();
    // The language ends up in a host name.
    let ok = !lang.is_empty()
        && lang.len() <= 12
        && !lang.starts_with('-')
        && !lang.ends_with('-')
        && lang
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    (ok && !title.is_empty()).then(|| (lang, title.to_string()))
}

fn is_qid(q: &str) -> bool {
    q.strip_prefix('Q')
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// From a Wikidata `Special:EntityData` answer: the sitelink in `lang` (else
/// English) and the P18 image file name.
fn parse_wikidata(v: &Value, qid: &str, lang: &str) -> WikiData {
    let Some(entity) = v.get("entities").and_then(|e| e.get(qid)) else {
        return WikiData::default();
    };
    let sitelink = |code: &str| -> Option<String> {
        let key = format!("{code}wiki");
        let title = entity.get("sitelinks")?.get(&key)?.get("title")?.as_str()?;
        Some(title.to_string())
    };
    let article = sitelink(lang)
        .map(|t| (lang.to_string(), t))
        .or_else(|| sitelink("en").map(|t| ("en".to_string(), t)));
    let image = entity
        .get("claims")
        .and_then(|c| c.get("P18"))
        .and_then(|p| p.get(0))
        .and_then(|c| c.get("mainsnak")?.get("datavalue")?.get("value")?.as_str())
        .map(str::to_string);
    WikiData { article, image }
}

/// From a REST `page/summary` answer. None for a disambiguation page, or
/// when there is neither text nor a link.
fn parse_summary(v: &Value) -> Option<Summary> {
    if str_at(v, "type") == Some("disambiguation") {
        return None;
    }
    let extract = str_at(v, "extract")
        .map(|e| cut_description(e, DESCRIPTION_MAX))
        .filter(|e| !e.is_empty());
    let page = v
        .get("content_urls")
        .and_then(|c| c.get("desktop"))
        .and_then(|d| str_at(d, "page"))
        .map(str::to_string);
    let image = ["thumbnail", "originalimage"]
        .iter()
        .find_map(|k| v.get(*k).and_then(|i| str_at(i, "source")))
        .map(str::to_string);
    (extract.is_some() || page.is_some()).then_some(Summary {
        extract,
        page,
        image,
    })
}

fn article_url(lang: &str, title: &str) -> String {
    format!(
        "https://{lang}.wikipedia.org/wiki/{}",
        percent_encode(&title.replace(' ', "_"))
    )
}

/// At most `max` characters, cut after a sentence end so it reads as a whole.
/// Without one in range, at a word with an ellipsis.
fn cut_description(text: &str, max: usize) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max {
        return text;
    }
    let mut end = 0;
    for (i, c) in chars.iter().enumerate().take(max) {
        let western = matches!(c, '.' | '!' | '?')
            && !matches!(chars.get(i + 1), Some(n) if !n.is_whitespace());
        if western || matches!(c, '。' | '！' | '？') {
            end = i + 1;
        }
    }
    if end > 0 {
        return chars[..end].iter().collect();
    }
    let head: String = chars[..max].iter().collect();
    let head = head.rsplit_once(' ').map_or(head.as_str(), |(h, _)| h);
    format!("{}…", head.trim_end_matches([',', ';', ':']))
}

// ------------------------------------------------------------------- photos

/// The image as a `data:` URL: jpeg, png or webp and at most 512 KB, else None.
fn image_data_url(agent: &ureq::Agent, url: &str) -> Option<String> {
    let (kind, bytes) = get(agent, url, IMAGE_LIMIT).ok().flatten()?;
    (matches!(kind.as_str(), "image/jpeg" | "image/png" | "image/webp") && !bytes.is_empty())
        .then(|| format!("data:{kind};base64,{}", crate::links::base64(&bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(pairs: &[(&str, &str)]) -> Tags {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn el(osm_type: &str, id: i64, lat: f64, lng: f64, pairs: &[(&str, &str)]) -> Element {
        Element {
            osm_type: osm_type.to_string(),
            id,
            lat,
            lng,
            tags: tags(pairs),
        }
    }

    // 0.0001 degrees of latitude are about 11 m.
    const LAT: f64 = 35.0;
    const LNG: f64 = 135.0;

    fn shrine() -> Element {
        el(
            "way",
            1,
            LAT + 0.0005,
            LNG,
            &[
                ("name", "伏見稲荷大社"),
                ("name:en", "Fushimi Inari Taisha"),
                ("amenity", "place_of_worship"),
                ("religion", "shinto"),
            ],
        )
    }

    fn cafe() -> Element {
        el(
            "node",
            2,
            LAT + 0.0001,
            LNG,
            &[("name", "Inari Coffee"), ("amenity", "cafe")],
        )
    }

    #[test]
    fn haversine_metres() {
        assert_eq!(haversine(LAT, LNG, LAT, LNG), 0.0);
        let d = haversine(0.0, 0.0, 0.001, 0.0);
        assert!((111.0..111.4).contains(&d), "{d}");
        // Longitude shrinks with latitude: about 91 m per 0.001 degrees at 35 N.
        let d = haversine(35.0, 0.0, 35.0, 0.001);
        assert!((90.0..92.0).contains(&d), "{d}");
    }

    #[test]
    fn name_match_beats_distance() {
        let els = vec![cafe(), shrine()];
        let got = choose(&els, LAT, LNG, Some("fushimi inari taisha"), "en").unwrap();
        assert_eq!(got.id, 1);
        // The local name matches too, as does `name:en` for a local query.
        let got = choose(&els, LAT, LNG, Some("伏見稲荷大社"), "ja").unwrap();
        assert_eq!(got.id, 1);
    }

    #[test]
    fn name_match_exact_before_containment_then_distance() {
        // "Inari" is contained in both; the exact one wins even if farther.
        let near = el("node", 3, LAT + 0.0001, LNG, &[("name", "Inari Coffee")]);
        let exact = el("node", 4, LAT + 0.0003, LNG, &[("name", "Inari")]);
        let els = vec![near, exact];
        assert_eq!(choose(&els, LAT, LNG, Some("INARI"), "en").unwrap().id, 4);
        // Only containment: the closer one.
        let a = el("node", 5, LAT + 0.0003, LNG, &[("name", "Inari Shop")]);
        let b = el("node", 6, LAT + 0.0001, LNG, &[("name", "Inari Cafe")]);
        let els = vec![a, b];
        assert_eq!(choose(&els, LAT, LNG, Some("Inari"), "en").unwrap().id, 6);
        // The query may be the longer one.
        let c = el("node", 7, LAT, LNG, &[("name", "Inari")]);
        let els = vec![c];
        let got = choose(&els, LAT, LNG, Some("Fushimi Inari Taisha"), "en");
        assert_eq!(got.unwrap().id, 7);
    }

    #[test]
    fn short_names_only_match_whole() {
        let a = el("node", 8, LAT, LNG, &[("name", "A"), ("shop", "kiosk")]);
        let els = vec![a];
        // "a" is the whole name: a match. "Cafe A" only contains it: no
        // match, but the kiosk is a place right there, so it is the fallback.
        assert_eq!(choose(&els, LAT, LNG, Some("a"), "en").unwrap().id, 8);
        let b = el("node", 9, LAT, LNG, &[("name", "A")]);
        let els = vec![b];
        assert!(choose(&els, LAT, LNG, Some("Cafe A"), "en").is_none());
    }

    #[test]
    fn local_language_name_matches() {
        let e = el("node", 10, LAT, LNG, &[("name", "Rathaus"), ("name:pt", "Câmara")]);
        let els = vec![e];
        assert_eq!(
            choose(&els, LAT, LNG, Some("câmara"), "pt-BR").unwrap().id,
            10
        );
    }

    #[test]
    fn nearest_place_without_a_name_match() {
        let street = el(
            "way",
            11,
            LAT + 0.00005,
            LNG,
            &[("name", "Main Street"), ("highway", "residential")],
        );
        let far = el(
            "node",
            12,
            LAT + 0.001,
            LNG,
            &[("name", "Far Museum"), ("tourism", "museum")],
        );
        let els = vec![street, far, cafe(), shrine()];
        // No name given: the nearest place (the cafe, 11 m), not the street.
        assert_eq!(choose(&els, LAT, LNG, None, "en").unwrap().id, 2);
        // A name that matches nothing falls back the same way.
        assert_eq!(choose(&els, LAT, LNG, Some("Nowhere"), "en").unwrap().id, 2);
    }

    #[test]
    fn nothing_when_too_far_or_not_a_place() {
        let far = el(
            "node",
            13,
            LAT + 0.001,
            LNG,
            &[("name", "Far Museum"), ("tourism", "museum")],
        );
        let street = el("way", 14, LAT, LNG, &[("name", "Main Street")]);
        let els = vec![far, street];
        assert!(choose(&els, LAT, LNG, None, "en").is_none());
        assert!(choose(&els, LAT, LNG, Some("Nowhere"), "en").is_none());
        assert!(choose(&[], LAT, LNG, None, "en").is_none());
        // A church building counts as a place; a plain building does not.
        let church = el("way", 15, LAT, LNG, &[("name", "St Mary"), ("building", "church")]);
        let house = el("way", 16, LAT, LNG, &[("name", "Villa"), ("building", "yes")]);
        assert!(is_place(&church));
        assert!(!is_place(&house));
    }

    #[test]
    fn humanises_kinds() {
        let k = |p: &[(&str, &str)]| kind_of(&tags(p));
        assert_eq!(k(&[("tourism", "museum")]).as_deref(), Some("Museum"));
        assert_eq!(k(&[("amenity", "cafe")]).as_deref(), Some("Cafe"));
        assert_eq!(k(&[("shop", "department_store")]).as_deref(), Some("Department store"));
        assert_eq!(k(&[("historic", "castle")]).as_deref(), Some("Castle"));
        // Order of preference, and a list takes its first value.
        let both = [("amenity", "cafe"), ("tourism", "attraction")];
        assert_eq!(k(&both).as_deref(), Some("Attraction"));
        assert_eq!(k(&[("amenity", "cafe;restaurant")]).as_deref(), Some("Cafe"));
        // "yes" says nothing: the next key decides.
        let yes = [("shop", "yes"), ("craft", "brewery")];
        assert_eq!(k(&yes).as_deref(), Some("Brewery"));
        assert_eq!(k(&[("shop", "yes")]), None);
        assert_eq!(k(&[("highway", "residential")]), None);
        assert_eq!(k(&[("building", "yes")]), None);
        assert_eq!(k(&[("building", "chapel")]).as_deref(), Some("Chapel"));
    }

    #[test]
    fn worship_kinds_follow_the_religion() {
        let w = |r: Option<&str>| {
            let mut p = vec![("amenity", "place_of_worship")];
            if let Some(r) = r {
                p.push(("religion", r));
            }
            kind_of(&tags(&p))
        };
        assert_eq!(w(Some("shinto")).as_deref(), Some("Shinto shrine"));
        assert_eq!(w(Some("buddhist")).as_deref(), Some("Buddhist temple"));
        assert_eq!(w(Some("christian")).as_deref(), Some("Church"));
        assert_eq!(w(Some("muslim")).as_deref(), Some("Mosque"));
        assert_eq!(w(Some("pastafarian")).as_deref(), Some("Place of worship"));
        assert_eq!(w(None).as_deref(), Some("Place of worship"));
    }

    #[test]
    fn builds_addresses() {
        let a = |p: &[(&str, &str)]| address_of(&tags(p));
        let lisbon = [
            ("addr:street", "Rua Augusta"),
            ("addr:housenumber", "24"),
            ("addr:postcode", "1100-053"),
            ("addr:city", "Lisboa"),
        ];
        assert_eq!(a(&lisbon).as_deref(), Some("Rua Augusta 24, 1100-053 Lisboa"));
        let no_number = [("addr:street", "Rua Augusta"), ("addr:city", "Lisboa")];
        assert_eq!(a(&no_number).as_deref(), Some("Rua Augusta, Lisboa"));
        let place = [("addr:place", "Praca do Comercio"), ("addr:housenumber", "1")];
        assert_eq!(a(&place).as_deref(), Some("Praca do Comercio 1"));
        assert_eq!(a(&[("addr:postcode", "1100-053")]).as_deref(), Some("1100-053"));
        assert_eq!(a(&[("addr:housenumber", "24")]), None);
        assert_eq!(a(&[("name", "X")]), None);
    }

    #[test]
    fn normalises_websites() {
        assert_eq!(normalize_url("example.com").as_deref(), Some("https://example.com"));
        assert_eq!(
            normalize_url(" www.example.com/menu ").as_deref(),
            Some("https://www.example.com/menu")
        );
        assert_eq!(normalize_url("http://x.org/a").as_deref(), Some("http://x.org/a"));
        assert_eq!(normalize_url("HTTPS://X.org").as_deref(), Some("HTTPS://X.org"));
        assert_eq!(normalize_url("https://"), None);
        for bad in [
            "",
            "   ",
            "javascript:alert(1)",
            "JavaScript:alert(1)",
            "mailto:a@b.c",
            "ftp://x.org",
            "has space.com",
        ] {
            assert_eq!(normalize_url(bad), None, "{bad}");
        }
    }

    #[test]
    fn cuts_descriptions_at_a_sentence() {
        let short = "Fushimi Inari Taisha is a shrine.  It is old.";
        assert_eq!(cut_description(short, 400), "Fushimi Inari Taisha is a shrine. It is old.");

        let sentence = "The shrine sits at the base of a mountain and is known for its thousands of torii gates. ";
        let long = sentence.repeat(10);
        let cut = cut_description(&long, 400);
        assert!(cut.chars().count() <= 400);
        assert!(cut.ends_with("torii gates."), "{cut}");
        assert_eq!(cut, sentence.repeat(4).trim_end());

        // A dot inside a word is not a sentence end.
        let dotted = format!("{} v1.2 {}", "word ".repeat(70), "more ".repeat(60));
        let cut = cut_description(&dotted, 400);
        assert!(cut.ends_with('…') && cut.chars().count() <= 401, "{cut}");

        // No sentence end at all: cut at a word.
        let cut = cut_description(&"lorem ipsum, ".repeat(60), 100);
        assert!(cut.ends_with("lorem…"), "{cut}");
        // Japanese full stop.
        let jp = format!("{}。{}", "あ".repeat(300), "い".repeat(300));
        assert_eq!(cut_description(&jp, 400).chars().count(), 301);
    }

    const OVERPASS: &str = r#"{
  "version": 0.6,
  "generator": "Overpass API 0.7.62.1 084b4234",
  "osm3s": {"timestamp_osm_base": "2026-11-07T10:00:00Z"},
  "elements": [
    {"type": "node", "id": 4141412345, "lat": 38.7106, "lon": -9.1398,
     "tags": {"name": "Museu do Design", "tourism": "museum", "opening_hours": "Tu-Su 10:00-18:00"}},
    {"type": "way", "id": 24941402, "center": {"lat": 38.71065, "lon": -9.13985},
     "tags": {"name": "Rua Augusta", "highway": "pedestrian", "oneway": "no"}},
    {"type": "relation", "id": 99, "tags": {"name": "no position"}}
  ]
}"#;

    #[test]
    fn reads_overpass() {
        let els = parse_overpass(OVERPASS.as_bytes()).unwrap();
        // The relation without coordinates is skipped.
        assert_eq!(els.len(), 2);
        assert_eq!(els[0].osm_type, "node");
        assert_eq!(els[0].id, 4141412345);
        assert_eq!((els[0].lat, els[0].lng), (38.7106, -9.1398));
        assert_eq!(els[0].tags["tourism"], "museum");
        assert_eq!((els[1].lat, els[1].lng), (38.71065, -9.13985));
        assert_eq!(osm_url(&els[1]), "https://www.openstreetmap.org/way/24941402");

        let got = choose(&els, 38.7106, -9.1398, Some("Museu do Design"), "pt").unwrap();
        assert_eq!(got.id, 4141412345);

        assert_eq!(parse_overpass(br#"{"elements": []}"#), Some(vec![]));
        // A timeout is reported as a remark with no elements.
        let busy = br#"{"elements": [], "remark": "runtime error: Query timed out"}"#;
        assert_eq!(parse_overpass(busy), None);
        assert_eq!(parse_overpass(b"<html>Too many requests</html>"), None);
        assert_eq!(parse_overpass(br#"{"foo": 1}"#), None);
    }

    #[test]
    fn maps_tags_to_details() {
        let t = tags(&[
            ("name", "伏見稲荷大社"),
            ("name:en", "Fushimi Inari Taisha"),
            ("amenity", "place_of_worship"),
            ("religion", "shinto"),
            ("opening_hours", "24/7"),
            ("contact:website", "inari.example.jp"),
            ("phone", "+81 75-641-7331; +81 75-000-0000"),
            ("addr:street", "深草藪之内町"),
            ("addr:housenumber", "68"),
            ("addr:postcode", "612-0882"),
            ("addr:city", "京都市"),
            ("wikipedia", "ja:伏見稲荷大社"),
        ]);
        let url = "https://www.openstreetmap.org/way/1".to_string();
        let d = from_tags(&t, "en", url.clone());
        assert_eq!(
            d,
            Details {
                name: Some("Fushimi Inari Taisha".into()),
                kind: Some("Shinto shrine".into()),
                address: Some("深草藪之内町 68, 612-0882 京都市".into()),
                opening_hours: Some("24/7".into()),
                website: Some("https://inari.example.jp".into()),
                phone: Some("+81 75-641-7331".into()),
                osm: Some(url.clone()),
                ..Details::default()
            }
        );
        // No `name:ja`: the local name.
        assert_eq!(from_tags(&t, "ja", url).name.as_deref(), Some("伏見稲荷大社"));
        // `website` wins over `contact:website`; `url` is the last resort.
        let w = tags(&[("website", "https://a.org"), ("contact:website", "b.org"), ("url", "c.org")]);
        assert_eq!(from_tags(&w, "en", String::new()).website.as_deref(), Some("https://a.org"));
        let w = tags(&[("website", "javascript:x"), ("url", "c.org")]);
        assert_eq!(from_tags(&w, "en", String::new()).website.as_deref(), Some("https://c.org"));
        let p = tags(&[("contact:phone", "+351 21 000 0000")]);
        assert_eq!(from_tags(&p, "en", String::new()).phone.as_deref(), Some("+351 21 000 0000"));
    }

    const NOMINATIM: &str = r#"{
  "place_id": 301853121, "licence": "Data (c) OpenStreetMap contributors, ODbL 1.0.",
  "osm_type": "way", "osm_id": 24941402, "lat": "38.7106", "lon": "-9.1398",
  "category": "tourism", "type": "museum", "place_rank": 30, "importance": 0.00007,
  "addresstype": "tourism", "name": "Museu do Design",
  "display_name": "Museu do Design, Rua Augusta, Lisboa, Portugal",
  "address": {"tourism": "Museu do Design", "road": "Rua Augusta", "house_number": "24",
    "suburb": "Santa Maria Maior", "city": "Lisboa", "postcode": "1100-053",
    "country": "Portugal", "country_code": "pt"},
  "extratags": {"opening_hours": "Tu-Su 10:00-18:00", "website": "museu.example.pt",
    "wikidata": "Q123456", "wheelchair": "yes"},
  "namedetails": {"name": "Museu do Design", "name:en": "Design Museum"},
  "boundingbox": ["38.7105", "38.7107", "-9.1399", "-9.1397"]
}"#;

    #[test]
    fn maps_nominatim_reverse() {
        let e = parse_reverse(NOMINATIM.as_bytes()).unwrap();
        assert_eq!((e.osm_type.as_str(), e.id), ("way", 24941402));
        assert_eq!((e.lat, e.lng), (38.7106, -9.1398));
        assert_eq!(e.tags["wikidata"], "Q123456");
        let d = from_tags(&e.tags, "en", osm_url(&e));
        assert_eq!(d.name.as_deref(), Some("Design Museum"));
        assert_eq!(d.kind.as_deref(), Some("Museum"));
        assert_eq!(d.address.as_deref(), Some("Rua Augusta 24, 1100-053 Lisboa"));
        assert_eq!(d.opening_hours.as_deref(), Some("Tu-Su 10:00-18:00"));
        assert_eq!(d.website.as_deref(), Some("https://museu.example.pt"));
        assert_eq!(d.osm.as_deref(), Some("https://www.openstreetmap.org/way/24941402"));
        // It goes through the same selection as Overpass results.
        let els = vec![e];
        assert!(choose(&els, 38.7106, -9.1398, Some("museu do design"), "pt").is_some());
        assert!(choose(&els, 38.7106, -9.1398, None, "pt").is_some());
    }

    #[test]
    fn nominatim_reverse_odd_answers() {
        assert_eq!(parse_reverse(br#"{"error": "Unable to geocode"}"#), None);
        assert_eq!(parse_reverse(b"not json"), None);
        // A plain house: an address but no kind, and not a place to visit.
        let house = br#"{"osm_type": "node", "osm_id": 5, "lat": 1.5, "lon": 2.5,
            "category": "building", "type": "yes", "name": "",
            "address": {"road": "Hauptstrasse", "house_number": "3", "town": "Kleinstadt", "postcode": "12345"}}"#;
        let e = parse_reverse(house).unwrap();
        assert!(!is_place(&e));
        let d = from_tags(&e.tags, "de", osm_url(&e));
        assert_eq!(d.kind, None);
        assert_eq!(d.name, None);
        assert_eq!(d.address.as_deref(), Some("Hauptstrasse 3, 12345 Kleinstadt"));
    }

    const WIKIDATA: &str = r#"{
  "entities": {"Q123456": {
    "type": "item", "id": "Q123456",
    "claims": {"P18": [{"mainsnak": {"snaktype": "value", "property": "P18",
      "datavalue": {"value": "Museu do Design, Lisboa.jpg", "type": "string"}}, "rank": "normal"}],
      "P31": [{"mainsnak": {"snaktype": "value", "property": "P31"}}]},
    "sitelinks": {
      "enwiki": {"site": "enwiki", "title": "Design Museum Lisbon", "badges": []},
      "ptwiki": {"site": "ptwiki", "title": "Museu do Design e da Moda", "badges": []}}
  }}
}"#;

    #[test]
    fn reads_wikidata() {
        let v: Value = serde_json::from_str(WIKIDATA).unwrap();
        let pt = parse_wikidata(&v, "Q123456", "pt");
        assert_eq!(
            pt.article,
            Some(("pt".to_string(), "Museu do Design e da Moda".to_string()))
        );
        assert_eq!(pt.image.as_deref(), Some("Museu do Design, Lisboa.jpg"));
        // No article in French: English.
        let fr = parse_wikidata(&v, "Q123456", "fr");
        assert_eq!(fr.article, Some(("en".to_string(), "Design Museum Lisbon".to_string())));
        // Wrong id, or an entity without sitelinks and claims.
        assert_eq!(parse_wikidata(&v, "Q1", "en"), WikiData::default());
        let bare: Value = serde_json::from_str(r#"{"entities": {"Q7": {"id": "Q7"}}}"#).unwrap();
        assert_eq!(parse_wikidata(&bare, "Q7", "en"), WikiData::default());
    }

    #[test]
    fn reads_wikipedia_references() {
        assert_eq!(
            parse_wikipedia_tag("de:Rua Augusta#Geschichte"),
            Some(("de".to_string(), "Rua Augusta".to_string()))
        );
        assert_eq!(
            parse_wikipedia_tag("EN: Lisbon "),
            Some(("en".to_string(), "Lisbon".to_string()))
        );
        assert_eq!(parse_wikipedia_tag("Lisbon"), None);
        assert_eq!(parse_wikipedia_tag("de:"), None);
        // The language becomes a host name.
        assert_eq!(parse_wikipedia_tag("evil.com/x:Foo"), None);
        assert_eq!(parse_wikipedia_tag("-x:Foo"), None);
        assert!(is_qid("Q42") && !is_qid("Q") && !is_qid("42") && !is_qid("Q4x"));
        assert_eq!(short_lang("pt-BR"), "pt");
        assert_eq!(short_lang("de_AT"), "de");
        assert_eq!(short_lang("EN"), "en");
        assert_eq!(short_lang("../x"), "en");
        assert_eq!(short_lang(""), "en");
        assert_eq!(
            article_url("de", "Fushimi Inari-Taisha"),
            "https://de.wikipedia.org/wiki/Fushimi_Inari-Taisha"
        );
    }

    #[test]
    fn reads_summaries() {
        let v: Value = serde_json::from_str(
            r#"{"type": "standard", "title": "Fushimi Inari-taisha",
            "extract": "Fushimi Inari-taisha is the head shrine of the kami Inari.",
            "thumbnail": {"source": "https://upload.wikimedia.org/thumb/a/320px-a.jpg", "width": 320, "height": 240},
            "originalimage": {"source": "https://upload.wikimedia.org/a.jpg", "width": 4000, "height": 3000},
            "content_urls": {"desktop": {"page": "https://en.wikipedia.org/wiki/Fushimi_Inari-taisha"},
              "mobile": {"page": "https://en.m.wikipedia.org/wiki/Fushimi_Inari-taisha"}}}"#,
        )
        .unwrap();
        let s = parse_summary(&v).unwrap();
        assert_eq!(
            s.extract.as_deref(),
            Some("Fushimi Inari-taisha is the head shrine of the kami Inari.")
        );
        assert_eq!(s.page.as_deref(), Some("https://en.wikipedia.org/wiki/Fushimi_Inari-taisha"));
        assert_eq!(s.image.as_deref(), Some("https://upload.wikimedia.org/thumb/a/320px-a.jpg"));

        let v: Value = serde_json::from_str(
            r#"{"type": "standard", "extract": "Text.",
            "originalimage": {"source": "https://upload.wikimedia.org/a.png"}}"#,
        )
        .unwrap();
        let s = parse_summary(&v).unwrap();
        assert_eq!(s.page, None);
        assert_eq!(s.image.as_deref(), Some("https://upload.wikimedia.org/a.png"));

        let v: Value = serde_json::from_str(
            r#"{"type": "disambiguation", "extract": "Inari may refer to:"}"#,
        )
        .unwrap();
        assert_eq!(parse_summary(&v), None);
        assert_eq!(parse_summary(&serde_json::json!({"type": "standard"})), None);
    }

    #[test]
    fn cache_freshness_is_thirty_days() {
        let day = 24 * 60 * 60;
        assert!(fresh(1000, 1000));
        assert!(fresh(1000, 1000 + 30 * day - 1));
        assert!(!fresh(1000, 1000 + 30 * day));
        // Written in the future (clock set back): refetch.
        assert!(!fresh(2000, 1000));
    }

    #[test]
    fn cache_key_rounds_to_five_decimals() {
        let a = cache_key(38.710_601, -9.139_804, Some("X"), "en");
        let b = cache_key(38.710_604, -9.139_799, Some("X"), "en");
        assert_eq!(a, b);
        assert_eq!(a, "38.71060,-9.13980,X,en");
        assert_ne!(a, cache_key(38.710_61, -9.1398, Some("X"), "en"));
        assert_ne!(a, cache_key(38.710_601, -9.139_804, None, "en"));
        assert_ne!(a, cache_key(38.710_601, -9.139_804, Some("X"), "de"));
    }

    #[test]
    fn serves_fresh_cache_without_the_network() {
        let root = std::env::temp_dir().join(format!(
            "orthros-details-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&root);
        let write = |name: Option<&str>, details: Details| {
            let path = cache_path(&root, 38.7106, -9.1398, name, "en");
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            let c = Cached {
                fetched: unix_now(),
                details,
            };
            fs::write(path, serde_json::to_vec(&c).unwrap()).unwrap();
        };
        let d = Details {
            name: Some("Museu".into()),
            kind: Some("Museum".into()),
            ..Details::default()
        };
        write(Some("Museu"), d.clone());
        assert_eq!(lookup(&root, 38.7106, -9.1398, Some(" Museu "), "en"), Some(d));
        // "Nothing found" is cached as empty details and shown as None.
        write(None, Details::default());
        assert_eq!(lookup(&root, 38.7106, -9.1398, None, "en"), None);
        // Out of range never reaches the cache or the network.
        assert_eq!(lookup(&root, 91.0, 0.0, None, "en"), None);
        assert_eq!(lookup(&root, 0.0, f64::NAN, None, "en"), None);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn details_use_snake_case_names() {
        let d = Details {
            opening_hours: Some("24/7".into()),
            ..Details::default()
        };
        let v = serde_json::to_value(&d).unwrap();
        assert_eq!(v["opening_hours"], "24/7");
        assert!(v.get("photo").is_some_and(Value::is_null));
    }
}
