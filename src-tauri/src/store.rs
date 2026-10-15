//! The space: a folder of plain markdown files, which are the source of truth.
//!
//!   <root>/days/2026-10-15.md   one file per day
//!   <root>/later.md             the "come back to it someday" list
//!
//! Documents are addressed by name ("later", "days/2026-10-15"); a name is
//! validated before it ever becomes a path, so the frontend can't escape the
//! root. Writing an empty document removes its file, so an untouched
//! "tomorrow" never leaves an empty file behind.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

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
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("orthros")
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn path(&self, name: &str) -> io::Result<PathBuf> {
        if name == "later" {
            return Ok(self.root.join("later.md"));
        }
        if let Some(date) = name.strip_prefix("days/") {
            if is_date(date) {
                return Ok(self.root.join("days").join(format!("{date}.md")));
            }
        }
        Err(io::Error::new(io::ErrorKind::InvalidInput, format!("invalid document name: {name}")))
    }

    pub fn read(&self, name: &str) -> io::Result<String> {
        match fs::read_to_string(self.path(name)?) {
            Ok(s) => Ok(s),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(String::new()),
            Err(e) => Err(e),
        }
    }

    /// Atomic: write a sibling temp file, then rename over the target, so a
    /// crash mid-write never leaves a half-written day.
    pub fn write(&self, name: &str, content: &str) -> io::Result<()> {
        let path = self.path(name)?;
        if content.trim().is_empty() {
            return match fs::remove_file(&path) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            };
        }
        let tmp = path.with_extension("md.tmp");
        {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(content.as_bytes())?;
            f.sync_all()?;
        }
        fs::rename(&tmp, &path)
    }

    /// Dates of all existing day files, newest first.
    pub fn days(&self) -> io::Result<Vec<String>> {
        let mut days: Vec<String> = fs::read_dir(self.root.join("days"))?
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().into_string().ok()?;
                let date = name.strip_suffix(".md")?;
                is_date(date).then(|| date.to_string())
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
}

/// `YYYY-MM-DD`, digits only. Not a calendar check, just a safe file name.
fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
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
    fn roundtrip_and_empty_removes() {
        let s = temp_store();
        s.write("days/2026-10-15", "- [ ] write stats\n").unwrap();
        s.write("days/2026-10-14", "- [x] load data\n").unwrap();
        assert_eq!(s.read("days/2026-10-15").unwrap(), "- [ ] write stats\n");
        assert_eq!(s.days().unwrap(), vec!["2026-10-15", "2026-10-14"]);

        s.write("days/2026-10-15", "  \n").unwrap();
        assert_eq!(s.days().unwrap(), vec!["2026-10-14"]);
        assert_eq!(s.read("days/2026-10-15").unwrap(), "");
    }

    #[test]
    fn search_is_case_insensitive() {
        let s = temp_store();
        s.write("later", "- Quantile regression for spikes\n").unwrap();
        s.write("days/2026-10-15", "- [ ] Investigate January outliers\n- [ ] price lags\n").unwrap();
        let hits = s.search("JANUARY", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].doc, "days/2026-10-15");
        assert_eq!(hits[0].line, 0);
        assert_eq!(s.search("e", 2).unwrap().len(), 2);
    }
}
