//! Background sync with an orthros-server (protocol: orthros_core::protocol).
//!
//! Per document the store keeps a *base*: the content last agreed on with
//! the server (.sync/base/...). A cycle:
//!
//!   pull  fetch everything changed since our cursor; where we also changed
//!         the document, three-way merge (base, ours, theirs)
//!   push  upload every document that differs from its base; if the server
//!         moved on meanwhile it answers with its copy, we merge and retry
//!
//! Runs half a second after local edits, as soon as the server reports
//! another device's edit (a long poll kept open on /api/wait), on request,
//! and every minute. Offline is normal: changes wait and go out once the
//! server is reachable again.
//!
//! Configured by `sync.json` in the config dir (~/.config/orthros/sync.json,
//! macOS: ~/Library/Application Support/orthros/sync.json):
//!   { "server": "https://freaxhp.example.ts.net:8444", "token_file": "~/.config/orthros/token" }
//! (or the token inline as "token"). Without it sync is off.

use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use orthros_core::protocol::{Changes, Put, PutResult, Wait};
use orthros_core::{hash, merge3};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::store::Store;
use crate::DocChanged;

/// A full cycle at least this often, even if the long poll says nothing.
const INTERVAL: Duration = Duration::from_secs(60);
/// After a local edit, wait this long so a burst of typing syncs once.
const SETTLE: Duration = Duration::from_millis(500);

#[derive(Deserialize)]
struct Config {
    server: String,
    #[serde(default)]
    token: String,
    /// Read the token from this file instead (`~/` allowed), so sync.json
    /// can live in a Nix store while the secret doesn't.
    #[serde(default)]
    token_file: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
struct State {
    server: String,
    cursor: u64,
}

#[derive(Serialize, Clone, Default)]
pub struct Status {
    /// "off" | "syncing" | "synced" | "offline" | "error"
    pub state: &'static str,
    pub message: Option<String>,
    /// Documents with local changes not yet on the server.
    pub pending: usize,
    /// Unix seconds of the last successful sync.
    pub last: Option<u64>,
}

enum Failure {
    Offline(String),
    Error(String),
}

impl From<std::io::Error> for Failure {
    fn from(e: std::io::Error) -> Self {
        Failure::Error(e.to_string())
    }
}

pub fn config_path() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("orthros").join("sync.json")
}

fn load_config() -> Result<Option<Config>, String> {
    let mut cfg: Config = match fs::read(config_path()) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", config_path().display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    if let Some(file) = &cfg.token_file {
        let path = match file.strip_prefix("~/") {
            Some(rest) => dirs::home_dir().unwrap_or_default().join(rest),
            None => PathBuf::from(file),
        };
        cfg.token = fs::read_to_string(&path).map_err(|e| format!("token file {}: {e}", path.display()))?.trim().to_string();
    }
    if cfg.token.is_empty() {
        return Err("no sync token (set \"token\" or \"token_file\")".into());
    }
    Ok(Some(cfg))
}

struct Client {
    agent: ureq::Agent,
    server: String,
    auth: String,
}

impl Client {
    fn new(c: &Config, timeout: Duration) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .http_status_as_error(false)
            .build()
            .into();
        Self { agent, server: c.server.trim_end_matches('/').to_string(), auth: format!("Bearer {}", c.token.trim()) }
    }

    fn check(status: u16) -> Result<(), Failure> {
        match status {
            200 => Ok(()),
            401 => Err(Failure::Error("the server rejected the sync token".into())),
            s => Err(Failure::Error(format!("server answered HTTP {s}"))),
        }
    }

    fn changes(&self, since: u64) -> Result<Changes, Failure> {
        let mut res = self.agent.get(format!("{}/api/changes?since={since}", self.server)).header("Authorization", &self.auth).call().map_err(net)?;
        Self::check(res.status().as_u16())?;
        res.body_mut().read_json().map_err(net)
    }

    /// Long poll: returns the server's revision once it passes `since`.
    fn wait(&self, since: u64) -> Result<u64, Failure> {
        let mut res = self.agent.get(format!("{}/api/wait?since={since}", self.server)).header("Authorization", &self.auth).call().map_err(net)?;
        Self::check(res.status().as_u16())?;
        res.body_mut().read_json::<Wait>().map(|w| w.rev).map_err(net)
    }

    fn put(&self, name: &str, put: &Put) -> Result<PutResult, Failure> {
        let mut res = self.agent.put(format!("{}/api/doc/{name}", self.server)).header("Authorization", &self.auth).send_json(put).map_err(net)?;
        Self::check(res.status().as_u16())?;
        res.body_mut().read_json().map_err(net)
    }
}

fn net(e: ureq::Error) -> Failure {
    match e {
        ureq::Error::Json(e) => Failure::Error(format!("unexpected answer from the server: {e}")),
        e => Failure::Offline(e.to_string()),
    }
}

type Notify = Arc<dyn Fn(&str, &str) + Send + std::marker::Sync>;

struct Engine {
    /// Called with (name, content) when sync changed a local document.
    notify: Notify,
    store: Arc<Store>,
    /// The revision we have pulled up to, shared with the listener.
    cursor: Arc<AtomicU64>,
}

impl Engine {
    fn notify(&self, name: &str, content: &str) {
        (self.notify)(name, content);
    }

    fn state_path(&self) -> PathBuf {
        self.store.root().join(".sync/state.json")
    }

    fn load_state(&self) -> State {
        fs::read(self.state_path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    fn save_state(&self, s: &State) -> std::io::Result<()> {
        let path = self.state_path();
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(path, serde_json::to_vec(s).map_err(std::io::Error::other)?)
    }

    /// Merge `theirs` (the server's copy) into the local document; it
    /// becomes the new base.
    fn take_remote(&self, name: &str, theirs: &str) -> std::io::Result<()> {
        let changed = self.store.locked(|| {
            let local = self.store.read(name)?;
            let base = self.store.read_base(name)?;
            let merged = if local == base { theirs.to_string() } else { merge3(&base, &local, theirs) };
            if merged != local {
                self.store.write_raw(name, &merged)?;
            }
            self.store.write_base(name, theirs)?;
            Ok((merged != local).then_some(merged))
        })?;
        if let Some(content) = changed {
            self.notify(name, &content);
        }
        Ok(())
    }

    fn dirty(&self) -> std::io::Result<Vec<(String, String, String)>> {
        let mut out = vec![];
        for name in self.store.all_names()? {
            let (local, base) = self.store.locked(|| Ok((self.store.read(&name)?, self.store.read_base(&name)?)))?;
            if local != base {
                out.push((name, local, base));
            }
        }
        Ok(out)
    }

    fn cycle(&self, cfg: &Config) -> Result<usize, Failure> {
        let client = Client::new(cfg, Duration::from_secs(20));
        let mut state = self.load_state();
        if state.server != client.server {
            // A different server: start over, merging everything as new.
            self.store.clear_bases()?;
            state = State { server: client.server.clone(), cursor: 0 };
        }

        // Pull.
        let changes = client.changes(state.cursor)?;
        for doc in &changes.docs {
            self.take_remote(&doc.name, &doc.content)?;
        }
        state.cursor = changes.rev;
        self.save_state(&state)?;
        self.cursor.store(changes.rev, Ordering::Relaxed);

        // Push.
        for (name, mut local, mut base) in self.dirty()? {
            for _attempt in 0..5 {
                match client.put(&name, &Put { base: hash(&base), content: local.clone() })? {
                    PutResult::Ok { .. } => {
                        self.store.locked(|| self.store.write_base(&name, &local))?;
                        break;
                    }
                    PutResult::Conflict { current, .. } => {
                        self.take_remote(&name, &current)?;
                        local = self.store.read(&name)?;
                        base = current;
                    }
                }
            }
        }
        Ok(self.dirty()?.len())
    }
}

enum Msg {
    /// A local document changed.
    Changed,
    /// The server has something new (or came back).
    Remote,
    /// The user asked (click on the dot, window focus, tray).
    Now,
    /// The server can't be reached.
    Unreachable(String),
}

/// Handle to the background sync threads.
pub struct Sync {
    tx: Sender<Msg>,
}

impl Sync {
    /// A local document changed: shown as pending at once, synced shortly.
    pub fn changed(&self) {
        let _ = self.tx.send(Msg::Changed);
    }

    pub fn now(&self) {
        let _ = self.tx.send(Msg::Now);
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Keep one long poll open on the server so other devices' edits arrive
/// within a second, and notice at once when the server becomes unreachable.
fn listen(cursor: Arc<AtomicU64>, tx: Sender<Msg>) {
    let mut seen = 0u64;
    let mut failing = false;
    let mut backoff = Duration::from_secs(2);
    loop {
        let cfg = match load_config() {
            Ok(Some(cfg)) => cfg,
            _ => {
                std::thread::sleep(Duration::from_secs(10));
                continue;
            }
        };
        let since = cursor.load(Ordering::Relaxed).max(seen);
        let sent = match Client::new(&cfg, Duration::from_secs(80)).wait(since) {
            Ok(rev) => {
                backoff = Duration::from_secs(2);
                seen = rev;
                let msg = (rev > since || failing).then_some(Msg::Remote);
                failing = false;
                msg.map(|m| tx.send(m))
            }
            Err(Failure::Offline(m)) => {
                let sent = (!failing).then(|| tx.send(Msg::Unreachable(m)));
                failing = true;
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(Duration::from_secs(30));
                sent
            }
            // A token problem or an old server: the regular cycle reports it.
            Err(Failure::Error(_)) => {
                std::thread::sleep(Duration::from_secs(30));
                None
            }
        };
        if let Some(Err(_)) = sent {
            return; // the sync thread is gone
        }
    }
}

pub fn start(app: AppHandle, store: Arc<Store>) -> Sync {
    let (tx, rx) = mpsc::channel::<Msg>();
    let cursor = Arc::new(AtomicU64::new(0));
    {
        let (cursor, tx) = (cursor.clone(), tx.clone());
        std::thread::spawn(move || listen(cursor, tx));
    }
    std::thread::spawn(move || {
        let emitter = app.clone();
        let notify: Notify = Arc::new(move |name: &str, content: &str| {
            let _ = emitter.emit("doc-changed", DocChanged { name: name.into(), content: content.into() });
        });
        let engine = Engine { notify, store, cursor };
        let mut current = Status { state: "off", ..Default::default() };
        fn emit(app: &AppHandle, current: &mut Status, s: Status) {
            let _ = app.emit("sync-status", s.clone());
            *crate::LAST_STATUS.lock().unwrap() = s.clone();
            *current = s;
        }
        let pending = |e: &Engine| e.dirty().map(|d| d.len()).unwrap_or(0);
        let mut last: Option<u64> = None;
        let mut run = true;
        loop {
            if run {
                match load_config() {
                    Ok(None) => emit(&app, &mut current, Status { state: "off", ..Default::default() }),
                    Err(e) => emit(&app, &mut current, Status { state: "error", message: Some(format!("bad sync config: {e}")), ..Default::default() }),
                    Ok(Some(cfg)) => {
                        if current.state != "offline" {
                            emit(&app, &mut current, Status { state: "syncing", pending: pending(&engine), last, message: None });
                        }
                        let status = match engine.cycle(&cfg) {
                            Ok(pending) => {
                                last = Some(now());
                                Status { state: "synced", pending, last, message: None }
                            }
                            Err(Failure::Offline(m)) => Status { state: "offline", message: Some(m), pending: pending(&engine), last },
                            Err(Failure::Error(m)) => Status { state: "error", message: Some(m), pending: pending(&engine), last },
                        };
                        emit(&app, &mut current, status);
                    }
                }
            }
            run = match rx.recv_timeout(INTERVAL) {
                Err(RecvTimeoutError::Timeout) => true,
                Err(RecvTimeoutError::Disconnected) => break,
                Ok(Msg::Unreachable(m)) => {
                    if current.state != "offline" && current.state != "off" {
                        emit(&app, &mut current, Status { state: "offline", message: Some(m), pending: pending(&engine), last });
                    }
                    false
                }
                Ok(msg) => {
                    if let Msg::Changed = msg {
                        // Show the edit as on its way (or waiting, if offline).
                        let state = if current.state == "offline" { "offline" } else { "syncing" };
                        if current.state != "off" {
                            let message = current.message.clone();
                            emit(&app, &mut current, Status { state, pending: pending(&engine), message, last });
                        }
                        std::thread::sleep(SETTLE);
                    }
                    while let Ok(m) = rx.try_recv() {
                        if let Msg::Unreachable(_) = m {
                            continue;
                        }
                    }
                    true
                }
            };
        }
    });
    Sync { tx }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "test-token-0123456789";

    /// A real orthros-server on a free local port, in a background runtime.
    fn server() -> String {
        let dir = std::env::temp_dir().join(format!("orthros-sync-server-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = fs::remove_dir_all(&dir);
        let space = Arc::new(orthros_server::Space::open(dir).unwrap());
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            rt.block_on(async move {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                tx.send(listener.local_addr().unwrap().port()).unwrap();
                axum::serve(listener, orthros_server::app(space, TOKEN.into())).await.unwrap();
            });
        });
        format!("http://127.0.0.1:{}", rx.recv().unwrap())
    }

    fn device(tag: &str) -> Engine {
        let dir = std::env::temp_dir().join(format!("orthros-sync-{tag}-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = fs::remove_dir_all(&dir);
        Engine { notify: Arc::new(|_: &str, _: &str| {}), store: Arc::new(Store::new(dir).unwrap()), cursor: Arc::new(AtomicU64::new(0)) }
    }

    fn cfg(server: &str) -> Config {
        Config { server: server.into(), token: TOKEN.into(), token_file: None }
    }

    fn sync(e: &Engine, c: &Config) -> usize {
        match e.cycle(c) {
            Ok(pending) => pending,
            Err(Failure::Offline(m)) | Err(Failure::Error(m)) => panic!("sync failed: {m}"),
        }
    }

    #[test]
    fn two_devices() {
        let c = cfg(&server());
        let (laptop, mac) = (device("laptop"), device("mac"));
        let day = "days/2026-10-15";

        // Laptop plans the day; the Mac receives it.
        laptop.store.save(day, "- [ ] a\n- [ ] b\n- [ ] c\n", "").unwrap();
        assert_eq!(sync(&laptop, &c), 0);
        sync(&mac, &c);
        assert_eq!(mac.store.read(day).unwrap(), "- [ ] a\n- [ ] b\n- [ ] c\n");

        // Offline edits on both: different lines merge, the same line keeps both.
        laptop.store.save(day, "- [x] a\n- [ ] b (laptop)\n- [ ] c\n", "- [ ] a\n- [ ] b\n- [ ] c\n").unwrap();
        mac.store.save(day, "- [ ] a\n- [ ] b (mac)\n- [x] c\n", "- [ ] a\n- [ ] b\n- [ ] c\n").unwrap();
        sync(&laptop, &c);
        assert_eq!(sync(&mac, &c), 0);
        sync(&laptop, &c);
        let want = "- [x] a\n- [ ] b (mac)\n- [ ] b (laptop)\n- [x] c\n";
        assert_eq!(mac.store.read(day).unwrap(), want);
        assert_eq!(laptop.store.read(day).unwrap(), want);

        // A new device with its own notes merges instead of overwriting.
        mac.store.save("later", "- from the mac\n", "").unwrap();
        let fresh = device("fresh");
        fresh.store.save("later", "- from the fresh one\n", "").unwrap();
        sync(&mac, &c);
        sync(&fresh, &c);
        sync(&mac, &c);
        assert_eq!(mac.store.read("later").unwrap(), "- from the fresh one\n- from the mac\n");

        // Deleting a day on one device deletes it everywhere.
        laptop.store.save(day, "", want).unwrap();
        sync(&laptop, &c);
        sync(&mac, &c);
        assert_eq!(mac.store.read(day).unwrap(), "");
        assert!(!mac.store.days().unwrap().contains(&"2026-10-15".to_string()));
    }

    #[test]
    fn unreachable_server_is_offline() {
        let e = device("offline");
        e.store.save("later", "- x\n", "").unwrap();
        assert!(matches!(e.cycle(&cfg("http://127.0.0.1:9")), Err(Failure::Offline(_))));
        assert_eq!(e.dirty().unwrap().len(), 1);
    }

    #[test]
    fn wrong_token_is_an_error() {
        let server = server();
        let e = device("token");
        let bad = Config { server, token: "wrong".into(), token_file: None };
        assert!(matches!(e.cycle(&bad), Err(Failure::Error(m)) if m.contains("token")));
    }
}
