//! The space: a folder of plain markdown files, which are the source of truth.
//!
//!   <root>/days/2026-10-15.md   one file per day
//!   <root>/later.md             the "come back to it someday" list
//!   <root>/.sync/               sync bookkeeping (see sync.rs)
//!   <root>/.lock                held during every read-modify-write
//!
//! Several writers touch these files: the editor, sync, the quick-capture
//! process and anything else (vim). Every change goes through `update`, which
//! holds an OS file lock, so the app and a separate capture process never
//! interleave. Writing an empty document removes its file, so an untouched
//! "tomorrow" never leaves an empty file behind.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use orthros_core::{doc_path, merge3};
use serde::Serialize;

pub struct Store {
    root: PathBuf,
}

#[derive(Serialize)]
pub struct Hit {
    pub doc: String,
    /// 0-based line number within the document.
    pub line: usize,
    pub text: String,
}

fn bad_name(name: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, format!("invalid document name: {name}"))
}

fn read_file(path: &Path) -> io::Result<String> {
    match fs::read_to_string(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        r => r,
    }
}

/// Atomic: write a sibling temp file, then rename over the target, so a
/// crash mid-write never leaves a half-written day. Empty content removes it.
fn write_file(path: &Path, content: &str) -> io::Result<()> {
    if content.trim().is_empty() {
        return match fs::remove_file(path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("md.tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(content.as_bytes())?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)
}

impl Store {
    pub fn new(root: PathBuf) -> io::Result<Self> {
        fs::create_dir_all(root.join("days"))?;
        Ok(Self { root })
    }

    /// `ORTHROS_DIR` if set, else the platform data dir
    /// (~/.local/share/orthros, ~/Library/Application Support/orthros).
    pub fn default_root() -> PathBuf {
        if let Some(dir) = std::env::var_os("ORTHROS_DIR") {
            return PathBuf::from(dir);
        }
        dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("orthros")
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Run `f` while holding the space lock (shared with other processes).
    pub fn locked<T>(&self, f: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
        let lock = fs::OpenOptions::new().create(true).truncate(false).write(true).open(self.root.join(".lock"))?;
        lock.lock()?;
        let out = f();
        let _ = lock.unlock();
        out
    }

    fn path(&self, name: &str) -> io::Result<PathBuf> {
        doc_path(&self.root, name).ok_or_else(|| bad_name(name))
    }

    pub fn read(&self, name: &str) -> io::Result<String> {
        read_file(&self.path(name)?)
    }

    /// Write without any merging. Callers hold the lock.
    pub fn write_raw(&self, name: &str, content: &str) -> io::Result<()> {
        write_file(&self.path(name)?, content)
    }

    /// The editor saves `content`, which it based on `prev` (what it last
    /// loaded or saved). If the file changed underneath it meanwhile (sync,
    /// capture, another editor), both edits are merged. Returns what is now
    /// on disk, so the editor can show it.
    pub fn save(&self, name: &str, content: &str, prev: &str) -> io::Result<String> {
        self.locked(|| {
            let disk = self.read(name)?;
            let out = if disk == prev || disk == content { content.to_string() } else { merge3(prev, content, &disk) };
            if out != disk {
                self.write_raw(name, &out)?;
            }
            Ok(out)
        })
    }

    /// Append one line, e.g. from quick capture. Returns the new content.
    pub fn append_line(&self, name: &str, line: &str) -> io::Result<String> {
        self.locked(|| {
            let mut s = self.read(name)?;
            if !s.is_empty() && !s.ends_with('\n') {
                s.push('\n');
            }
            s.push_str(line);
            s.push('\n');
            self.write_raw(name, &s)?;
            Ok(s)
        })
    }

    // -------------------------------------------------- sync bookkeeping

    /// The content last agreed on with the server, per document.
    pub fn read_base(&self, name: &str) -> io::Result<String> {
        read_file(&self.base_path(name)?)
    }

    pub fn write_base(&self, name: &str, content: &str) -> io::Result<()> {
        write_file(&self.base_path(name)?, content)
    }

    pub fn clear_bases(&self) -> io::Result<()> {
        match fs::remove_dir_all(self.root.join(".sync/base")) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    fn base_path(&self, name: &str) -> io::Result<PathBuf> {
        doc_path(&self.root.join(".sync/base"), name).ok_or_else(|| bad_name(name))
    }

    /// Every document that exists locally or has a base (so deletions count).
    pub fn all_names(&self) -> io::Result<Vec<String>> {
        let mut names: Vec<String> = Vec::new();
        for root in [self.root.clone(), self.root.join(".sync/base")] {
            if root.join("later.md").exists() {
                names.push("later".into());
            }
            if let Ok(rd) = fs::read_dir(root.join("days")) {
                for e in rd.flatten() {
                    let file = e.file_name().to_string_lossy().into_owned();
                    if let Some(date) = file.strip_suffix(".md").filter(|d| orthros_core::is_date(d)) {
                        names.push(format!("days/{date}"));
                    }
                }
            }
        }
        names.sort();
        names.dedup();
        Ok(names)
    }

    // -------------------------------------------------- browsing

    /// Dates of all existing day files, newest first.
    pub fn days(&self) -> io::Result<Vec<String>> {
        let mut days: Vec<String> = fs::read_dir(self.root.join("days"))?
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().into_string().ok()?;
                let date = name.strip_suffix(".md")?;
                orthros_core::is_date(date).then(|| date.to_string())
            })
            .collect();
        days.sort_unstable_by(|a, b| b.cmp(a));
        Ok(days)
    }

    /// Case-insensitive substring search over every document. Plain scanning
    /// is instant at journal scale (years of days are a few MB).
    pub fn search(&self, query: &str, limit: usize) -> io::Result<Vec<Hit>> {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return Ok(Vec::new());
        }
        let mut docs = vec!["later".to_string()];
        docs.extend(self.days()?.into_iter().map(|d| format!("days/{d}")));

        let mut hits = Vec::new();
        for doc in docs {
            for (line, text) in self.read(&doc)?.lines().enumerate() {
                if text.to_lowercase().contains(&needle) {
                    hits.push(Hit { doc: doc.clone(), line, text: text.to_string() });
                    if hits.len() >= limit {
                        return Ok(hits);
                    }
                }
            }
        }
        Ok(hits)
    }

    /// Map a changed file path back to its document name.
    pub fn name_of(&self, path: &Path) -> Option<String> {
        let rel = path.strip_prefix(&self.root).ok()?;
        let s = rel.to_str()?;
        if s == "later.md" {
            return Some("later".into());
        }
        let date = s.strip_prefix("days/")?.strip_suffix(".md")?;
        orthros_core::is_date(date).then(|| format!("days/{date}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> Store {
        let dir = std::env::temp_dir().join(format!("orthros-test-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = fs::remove_dir_all(&dir);
        Store::new(dir).unwrap()
    }

    #[test]
    fn rejects_names_outside_the_space() {
        let s = temp_store();
        for bad in ["../etc/passwd", "days/../../x", "days/2026-1-15", "days/2026-10-15.md", "Later", ""] {
            assert!(s.read(bad).is_err(), "{bad} should be rejected");
        }
    }

    #[test]
    fn save_roundtrip_and_empty_removes() {
        let s = temp_store();
        s.save("days/2026-10-15", "- [ ] write stats\n", "").unwrap();
        s.save("days/2026-10-14", "- [x] load data\n", "").unwrap();
        assert_eq!(s.read("days/2026-10-15").unwrap(), "- [ ] write stats\n");
        assert_eq!(s.days().unwrap(), vec!["2026-10-15", "2026-10-14"]);

        s.save("days/2026-10-15", "  \n", "- [ ] write stats\n").unwrap();
        assert_eq!(s.days().unwrap(), vec!["2026-10-14"]);
    }

    #[test]
    fn save_merges_changes_made_underneath() {
        let s = temp_store();
        s.save("later", "- a\n- b\n", "").unwrap();
        // Capture appends while the editor still holds "- a\n- b\n".
        s.append_line("later", "- captured").unwrap();
        let out = s.save("later", "- a!\n- b\n", "- a\n- b\n").unwrap();
        assert_eq!(out, "- a!\n- b\n- captured\n");
        assert_eq!(s.read("later").unwrap(), out);
    }

    #[test]
    fn names_include_deleted_bases() {
        let s = temp_store();
        s.save("later", "- x\n", "").unwrap();
        s.write_base("days/2026-10-01", "- gone\n").unwrap();
        assert_eq!(s.all_names().unwrap(), vec!["days/2026-10-01", "later"]);
        assert_eq!(s.name_of(&s.root().join("days/2026-10-01.md")).as_deref(), Some("days/2026-10-01"));
        assert_eq!(s.name_of(&s.root().join(".sync/base/later.md")), None);
    }

    #[test]
    fn search_is_case_insensitive() {
        let s = temp_store();
        s.save("later", "- Quantile regression for spikes\n", "").unwrap();
        s.save("days/2026-10-15", "- [ ] Investigate January outliers\n- [ ] price lags\n", "").unwrap();
        let hits = s.search("JANUARY", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].doc, "days/2026-10-15");
        assert_eq!(s.search("e", 2).unwrap().len(), 2);
    }
}
