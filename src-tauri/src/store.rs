//! The space: a folder of plain markdown files, which are the source of truth.
//!
//!   <root>/days/2026-10-15.md   one file per day
//!   <root>/later.md             the "come back to it someday" list
//!   <root>/pins.md              which notes are pinned, one "notes/<slug>" per line
//!   <root>/notes/<slug>.md      one file per note
//!   <root>/.cache/              link previews, places and map tiles (see links.rs,
//!                               places.rs, tiles.rs), never synced
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
use std::time::UNIX_EPOCH;

use orthros_core::{doc_path, merge3};
use serde::Serialize;

pub struct Store {
    root: PathBuf,
}

/// One note in the notes list.
#[derive(Serialize)]
pub struct NoteInfo {
    /// Document name, "notes/<slug>".
    pub name: String,
    /// The first `# ` heading, else the slug.
    pub title: String,
    /// File modification time, seconds since the epoch.
    pub modified: u64,
}

#[derive(Serialize)]
pub struct Hit {
    pub doc: String,
    /// 0-based line number within the document.
    pub line: usize,
    pub text: String,
}

fn bad_name(name: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("invalid document name: {name}"),
    )
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
        fs::create_dir_all(root.join("notes"))?;
        Ok(Self { root })
    }

    /// `ORTHROS_DIR` if set, else the platform data dir
    /// (~/.local/share/orthros, ~/Library/Application Support/orthros).
    pub fn default_root() -> PathBuf {
        if let Some(dir) = std::env::var_os("ORTHROS_DIR") {
            return PathBuf::from(dir);
        }
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("orthros")
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Run `f` while holding the space lock (shared with other processes).
    pub fn locked<T>(&self, f: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.root.join(".lock"))?;
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
            let out = if disk == prev || disk == content {
                content.to_string()
            } else {
                merge3(prev, content, &disk)
            };
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
            for n in ["later", "pins"] {
                if root.join(format!("{n}.md")).exists() {
                    names.push(n.into());
                }
            }
            if let Ok(rd) = fs::read_dir(root.join("notes")) {
                for e in rd.flatten() {
                    let file = e.file_name().to_string_lossy().into_owned();
                    if let Some(slug) = file
                        .strip_suffix(".md")
                        .filter(|s| orthros_core::is_slug(s))
                    {
                        names.push(format!("notes/{slug}"));
                    }
                }
            }
            if let Ok(rd) = fs::read_dir(root.join("days")) {
                for e in rd.flatten() {
                    let file = e.file_name().to_string_lossy().into_owned();
                    if let Some(date) = file
                        .strip_suffix(".md")
                        .filter(|d| orthros_core::is_date(d))
                    {
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

    /// All notes, most recently edited first.
    pub fn notes(&self) -> io::Result<Vec<NoteInfo>> {
        let rd = match fs::read_dir(self.root.join("notes")) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            r => r?,
        };
        let mut notes: Vec<NoteInfo> = rd
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let file = e.file_name().into_string().ok()?;
                let slug = file
                    .strip_suffix(".md")
                    .filter(|s| orthros_core::is_slug(s))?;
                let modified = e
                    .metadata()
                    .ok()?
                    .modified()
                    .ok()?
                    .duration_since(UNIX_EPOCH)
                    .ok()?
                    .as_secs();
                let text = fs::read_to_string(e.path()).unwrap_or_default();
                let heading = text
                    .lines()
                    .find_map(|l| l.strip_prefix("# "))
                    .map(str::trim)
                    .filter(|t| !t.is_empty());
                Some(NoteInfo {
                    name: format!("notes/{slug}"),
                    title: heading.unwrap_or(slug).to_string(),
                    modified,
                })
            })
            .collect();
        notes.sort_unstable_by(|a, b| {
            b.modified
                .cmp(&a.modified)
                .then_with(|| a.name.cmp(&b.name))
        });
        Ok(notes)
    }

    /// Case-insensitive substring search over every document (except pins,
    /// which is only a list of names). Plain scanning
    /// is instant at journal scale (years of days are a few MB).
    pub fn search(&self, query: &str, limit: usize) -> io::Result<Vec<Hit>> {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return Ok(Vec::new());
        }
        let mut docs = vec!["later".to_string()];
        docs.extend(self.notes()?.into_iter().map(|n| n.name));
        docs.extend(self.days()?.into_iter().map(|d| format!("days/{d}")));

        let mut hits = Vec::new();
        for doc in docs {
            for (line, text) in self.read(&doc)?.lines().enumerate() {
                if text.to_lowercase().contains(&needle) {
                    hits.push(Hit {
                        doc: doc.clone(),
                        line,
                        text: text.to_string(),
                    });
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
        match s {
            "later.md" => return Some("later".into()),
            "pins.md" => return Some("pins".into()),
            _ => {}
        }
        if let Some(slug) = s.strip_prefix("notes/") {
            return slug
                .strip_suffix(".md")
                .filter(|s| orthros_core::is_slug(s))
                .map(|s| format!("notes/{s}"));
        }
        let date = s.strip_prefix("days/")?.strip_suffix(".md")?;
        orthros_core::is_date(date).then(|| format!("days/{date}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn temp_store() -> Store {
        let dir = std::env::temp_dir().join(format!(
            "orthros-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        Store::new(dir).unwrap()
    }

    #[test]
    fn rejects_names_outside_the_space() {
        let s = temp_store();
        for bad in [
            "../etc/passwd",
            "days/../../x",
            "days/2026-1-15",
            "days/2026-10-15.md",
            "Later",
            "",
        ] {
            assert!(s.read(bad).is_err(), "{bad} should be rejected");
        }
    }

    #[test]
    fn save_roundtrip_and_empty_removes() {
        let s = temp_store();
        s.save("days/2026-10-15", "- [ ] write stats\n", "")
            .unwrap();
        s.save("days/2026-10-14", "- [x] load data\n", "").unwrap();
        assert_eq!(s.read("days/2026-10-15").unwrap(), "- [ ] write stats\n");
        assert_eq!(s.days().unwrap(), vec!["2026-10-15", "2026-10-14"]);

        s.save("days/2026-10-15", "  \n", "- [ ] write stats\n")
            .unwrap();
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
        assert_eq!(
            s.name_of(&s.root().join("days/2026-10-01.md")).as_deref(),
            Some("days/2026-10-01")
        );
        assert_eq!(s.name_of(&s.root().join(".sync/base/later.md")), None);
    }

    #[test]
    fn search_is_case_insensitive() {
        let s = temp_store();
        s.save("later", "- Quantile regression for spikes\n", "")
            .unwrap();
        s.save(
            "days/2026-10-15",
            "- [ ] Investigate January outliers\n- [ ] price lags\n",
            "",
        )
        .unwrap();
        let hits = s.search("JANUARY", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].doc, "days/2026-10-15");
        assert_eq!(s.search("e", 2).unwrap().len(), 2);
    }

    fn touch(s: &Store, name: &str, secs: u64) {
        let f = fs::File::options()
            .write(true)
            .open(s.path(name).unwrap())
            .unwrap();
        f.set_modified(UNIX_EPOCH + Duration::from_secs(secs))
            .unwrap();
    }

    #[test]
    fn notes_are_listed_newest_first_with_titles() {
        let s = temp_store();
        s.save("notes/trip-ideas", "# Trip ideas \n\n- Lisbon\n", "")
            .unwrap();
        s.save("notes/untitled", "- just a bullet\n", "").unwrap();
        s.save("notes/recipes", "intro\n#not a heading\n# Recipes\n", "")
            .unwrap();
        fs::write(s.root().join("notes/Bad Name.md"), "# Nope\n").unwrap();
        fs::write(s.root().join("notes/readme.txt"), "# Nope\n").unwrap();
        touch(&s, "notes/trip-ideas", 1000);
        touch(&s, "notes/untitled", 3000);
        touch(&s, "notes/recipes", 2000);
        let notes = s.notes().unwrap();
        let got: Vec<_> = notes
            .iter()
            .map(|n| (n.name.as_str(), n.title.as_str(), n.modified))
            .collect();
        assert_eq!(
            got,
            [
                ("notes/untitled", "untitled", 3000),
                ("notes/recipes", "Recipes", 2000),
                ("notes/trip-ideas", "Trip ideas", 1000)
            ]
        );
    }

    #[test]
    fn pins_and_notes_are_names_and_searchable() {
        let s = temp_store();
        s.save("pins", "notes/trip-ideas\n", "").unwrap();
        s.save("notes/trip-ideas", "# Trip\n- Lisbon in March\n", "")
            .unwrap();
        s.save("notes/old", "- lisbon airport\n", "").unwrap();
        s.save("days/2026-10-15", "- [ ] book Lisbon\n", "")
            .unwrap();
        s.save("later", "- lisbon?\n", "").unwrap();
        touch(&s, "notes/trip-ideas", 2000);
        touch(&s, "notes/old", 1000);
        s.write_base("notes/gone", "- x\n").unwrap();

        assert_eq!(
            s.all_names().unwrap(),
            vec![
                "days/2026-10-15",
                "later",
                "notes/gone",
                "notes/old",
                "notes/trip-ideas",
                "pins"
            ]
        );
        assert_eq!(
            s.name_of(&s.root().join("pins.md")).as_deref(),
            Some("pins")
        );
        assert_eq!(
            s.name_of(&s.root().join("notes/trip-ideas.md")).as_deref(),
            Some("notes/trip-ideas")
        );
        for other in [
            "notes/Bad.md",
            "notes/a/b.md",
            "notes/x.txt",
            ".cache/links/ab.json",
            ".sync/base/notes/gone.md",
        ] {
            assert_eq!(s.name_of(&s.root().join(other)), None, "{other}");
        }

        // Order: later, notes (newest first), days. Pins is never searched.
        let hits = s.search("lisbon", 10).unwrap();
        assert_eq!(
            hits.iter().map(|h| h.doc.as_str()).collect::<Vec<_>>(),
            ["later", "notes/trip-ideas", "notes/old", "days/2026-10-15"]
        );
        assert!(s.search("notes/trip", 10).unwrap().is_empty());
    }
}
