//! Watch the space for changed files: the editor's own saves, quick capture
//! (a separate process on Linux), sync, or an edit in vim. Every change is
//! passed to the frontend (which ignores what it already shows) and nudges
//! sync, so edits reach the server a moment after you stop typing.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use notify_debouncer_mini::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use tauri::{AppHandle, Emitter, Manager};

use crate::store::Store;
use crate::sync::Sync;
use crate::DocChanged;

pub struct Watcher(#[allow(dead_code)] Mutex<Debouncer<RecommendedWatcher>>);

pub fn start(app: AppHandle, store: Arc<Store>) -> notify_debouncer_mini::notify::Result<Watcher> {
    let root = store.root().to_path_buf();
    let mut debouncer = new_debouncer(Duration::from_millis(250), move |res: DebounceEventResult| {
        let Ok(events) = res else { return };
        let mut any = false;
        for e in events {
            let Some(name) = store.name_of(&e.path) else { continue };
            let Ok(content) = store.read(&name) else { continue };
            let _ = app.emit("doc-changed", DocChanged { name, content });
            any = true;
        }
        if any {
            if let Some(sync) = app.try_state::<Sync>() {
                sync.changed();
            }
        }
    })?;
    debouncer.watcher().watch(&root, RecursiveMode::Recursive)?;
    Ok(Watcher(Mutex::new(debouncer)))
}
