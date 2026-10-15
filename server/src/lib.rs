//! orthros-server: the sync hub. The space is kept as the same plain files
//! the app writes (days/YYYY-MM-DD.md, later.md), plus `.index.json` with the
//! revision of every document. Protocol: see orthros_core::protocol.
//!
//!   orthros-server --data /var/lib/orthros --listen 127.0.0.1:8444 \
//!                  --token-file /run/secrets/orthros-token
//!
//! Meant to sit behind `tailscale serve` (HTTPS); every request needs
//! `Authorization: Bearer <token>`.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use axum::extract::{Path as UrlPath, Query, Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{get, put};
use axum::{Json, Router};
use orthros_core::protocol::{Changes, Put, PutResult, RemoteDoc};
use orthros_core::{doc_path, hash};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default)]
struct Index {
    rev: u64,
    /// Document name → revision it last changed at.
    docs: BTreeMap<String, u64>,
}

pub struct Space {
    root: PathBuf,
    index: Mutex<Index>,
}

impl Space {
    pub fn open(root: PathBuf) -> io::Result<Self> {
        fs::create_dir_all(root.join("days"))?;
        let index = match fs::read(root.join(".index.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(io::Error::other)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Self::scan(&root)?,
            Err(e) => return Err(e),
        };
        let space = Self { root, index: Mutex::new(index) };
        space.save(&space.index.lock().unwrap())?;
        Ok(space)
    }

    /// First start on an existing folder: every file gets a revision.
    fn scan(root: &Path) -> io::Result<Index> {
        let mut names = vec![];
        if root.join("later.md").exists() {
            names.push("later".to_string());
        }
        for e in fs::read_dir(root.join("days"))? {
            let name = e?.file_name().to_string_lossy().into_owned();
            if let Some(date) = name.strip_suffix(".md").filter(|d| orthros_core::is_date(d)) {
                names.push(format!("days/{date}"));
            }
        }
        let mut index = Index::default();
        for n in names {
            index.rev += 1;
            index.docs.insert(n, index.rev);
        }
        Ok(index)
    }

    fn save(&self, index: &Index) -> io::Result<()> {
        write_atomic(&self.root.join(".index.json"), &serde_json::to_vec_pretty(index).map_err(io::Error::other)?)
    }

    fn read(&self, name: &str) -> io::Result<String> {
        let path = doc_path(&self.root, name).ok_or_else(|| io::Error::other("bad name"))?;
        match fs::read_to_string(path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(String::new()),
            r => r,
        }
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(tmp, path)
}

pub type Shared = Arc<Space>;
type HttpResult<T> = Result<T, (StatusCode, String)>;

fn internal(e: io::Error) -> (StatusCode, String) {
    eprintln!("orthros-server: {e}");
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

#[derive(Deserialize)]
struct Since {
    #[serde(default)]
    since: u64,
}

async fn changes(State(space): State<Shared>, Query(q): Query<Since>) -> HttpResult<Json<Changes>> {
    let index = space.index.lock().unwrap();
    let mut docs = vec![];
    for (name, &rev) in index.docs.iter().filter(|(_, &r)| r > q.since) {
        docs.push(RemoteDoc { name: name.clone(), rev, content: space.read(name).map_err(internal)? });
    }
    Ok(Json(Changes { rev: index.rev, docs }))
}

async fn put_doc(State(space): State<Shared>, UrlPath(name): UrlPath<String>, Json(body): Json<Put>) -> HttpResult<Json<PutResult>> {
    let path = doc_path(&space.root, &name).ok_or((StatusCode::BAD_REQUEST, format!("invalid document name: {name}")))?;
    let mut index = space.index.lock().unwrap();
    let current = space.read(&name).map_err(internal)?;
    let doc_rev = index.docs.get(&name).copied().unwrap_or(0);

    if hash(&current) != body.base {
        return Ok(Json(PutResult::Conflict { rev: doc_rev, current }));
    }
    if body.content == current {
        return Ok(Json(PutResult::Ok { rev: doc_rev }));
    }
    if body.content.trim().is_empty() {
        match fs::remove_file(&path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(internal(e)),
            _ => {}
        }
    } else {
        write_atomic(&path, body.content.as_bytes()).map_err(internal)?;
    }
    index.rev += 1;
    let rev = index.rev;
    index.docs.insert(name, rev);
    space.save(&index).map_err(internal)?;
    Ok(Json(PutResult::Ok { rev }))
}

/// Constant-time comparison, so the token can't be guessed byte by byte.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

async fn auth(State(token): State<Arc<String>>, req: Request, next: Next) -> Result<Response, StatusCode> {
    let given = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    if same(given.as_bytes(), token.as_bytes()) {
        Ok(next.run(req).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

pub fn app(space: Shared, token: String) -> Router {
    Router::new()
        .route("/api/changes", get(changes))
        .route("/api/doc/{*name}", put(put_doc))
        .with_state(space)
        .layer(middleware::from_fn_with_state(Arc::new(token), auth))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    const TOKEN: &str = "0123456789abcdef";

    fn space() -> Shared {
        let dir = std::env::temp_dir().join(format!("orthros-server-test-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = fs::remove_dir_all(&dir);
        Arc::new(Space::open(dir).unwrap())
    }

    async fn call(app: &Router, method: &str, uri: &str, token: &str, body: Option<serde_json::Value>) -> (StatusCode, serde_json::Value) {
        let req = Request::builder()
            .method(method)
            .uri(uri)
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .body(body.map(|b| Body::from(b.to_string())).unwrap_or_default())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null))
    }

    #[tokio::test]
    async fn rejects_wrong_token() {
        let app = app(space(), TOKEN.into());
        assert_eq!(call(&app, "GET", "/api/changes", "nope", None).await.0, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn put_conflict_and_changes() {
        let app = app(space(), TOKEN.into());
        let empty = hash("");

        let (s, r) = call(&app, "PUT", "/api/doc/days/2026-10-15", TOKEN, Some(serde_json::json!({"base": empty, "content": "- [ ] a\n"}))).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(r["status"], "ok");
        assert_eq!(r["rev"], 1);

        // A second device still thinks the day is empty: conflict, gets current.
        let (_, r) = call(&app, "PUT", "/api/doc/days/2026-10-15", TOKEN, Some(serde_json::json!({"base": empty, "content": "- [ ] b\n"}))).await;
        assert_eq!(r["status"], "conflict");
        assert_eq!(r["current"], "- [ ] a\n");

        let (_, r) = call(&app, "GET", "/api/changes?since=0", TOKEN, None).await;
        assert_eq!(r["rev"], 1);
        assert_eq!(r["docs"][0]["name"], "days/2026-10-15");
        let (_, r) = call(&app, "GET", "/api/changes?since=1", TOKEN, None).await;
        assert_eq!(r["docs"].as_array().unwrap().len(), 0);

        // Delete: shows up as an empty document.
        let (_, r) = call(&app, "PUT", "/api/doc/days/2026-10-15", TOKEN, Some(serde_json::json!({"base": hash("- [ ] a\n"), "content": ""}))).await;
        assert_eq!(r["rev"], 2);
        let (_, r) = call(&app, "GET", "/api/changes?since=1", TOKEN, None).await;
        assert_eq!(r["docs"][0]["content"], "");
    }

    #[tokio::test]
    async fn rejects_bad_names() {
        let app = app(space(), TOKEN.into());
        let (s, _) = call(&app, "PUT", "/api/doc/../etc/passwd", TOKEN, Some(serde_json::json!({"base": hash(""), "content": "x"}))).await;
        assert_ne!(s, StatusCode::OK);
    }
}
