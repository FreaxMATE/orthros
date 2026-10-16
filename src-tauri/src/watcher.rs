//! Watch the space for changed files: the editor's own saves, quick capture
//! (a separate process on Linux), sync, or an edit in vim. A change is passed
//! to the frontend (which ignores what it already shows) and nudges sync, so
//! edits reach the server a moment after you stop typing.
//!
//! Only writes count. Reading a file also produces events (open, access,
//! close), and sync reads every file: reacting to those would restart sync
//! forever. And a document is only passed on when its content differs from
//! what this watcher last saw, so repeated events for one write are one change.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notify::event::{AccessKind, AccessMode, ModifyKind};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};
use tauri::{AppHandle, Emitter, Manager};

use crate::store::Store;
use crate::sync::Sync;
use crate::DocChanged;

/// Events closer together than this are handled as one batch.
const SETTLE: Duration = Duration::from_millis(250);

pub struct Watcher(#[allow(dead_code)] Mutex<RecommendedWatcher>);

fn is_write(kind: &EventKind) -> bool {
    match kind {
        EventKind::Create(_) | EventKind::Remove(_) => true,
        EventKind::Modify(ModifyKind::Metadata(_)) => false,
        EventKind::Modify(_) => true,
        EventKind::Access(AccessKind::Close(AccessMode::Write)) => true,
        _ => false,
    }
}

pub fn start(app: AppHandle, store: Arc<Store>) -> notify::Result<Watcher> {
    let (tx, rx) = mpsc::channel::<PathBuf>();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
        let Ok(event) = res else { return };
        if is_write(&event.kind) {
            for path in event.paths {
                let _ = tx.send(path);
            }
        }
    })?;
    watcher.watch(store.root(), RecursiveMode::Recursive)?;

    std::thread::spawn(move || {
        let mut last: HashMap<String, String> = HashMap::new();
        while let Ok(first) = rx.recv() {
            let mut paths = HashSet::from([first]);
            while let Ok(p) = rx.recv_timeout(SETTLE) {
                paths.insert(p);
            }
            let mut changed = false;
            for path in paths {
                let Some(name) = store.name_of(&path) else { continue };
                let Ok(content) = store.read(&name) else { continue };
                if last.get(&name) == Some(&content) {
                    continue;
                }
                last.insert(name.clone(), content.clone());
                let _ = app.emit("doc-changed", DocChanged { name, content });
                changed = true;
            }
            if changed {
                if let Some(sync) = app.try_state::<Sync>() {
                    sync.changed();
                }
            }
        }
    });

    Ok(Watcher(Mutex::new(watcher)))
}
