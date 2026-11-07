//! What the orthros app and its sync server share: how documents are named
//! and stored, how two edited versions are merged, and the wire protocol.

pub mod merge;
pub mod protocol;

use std::path::{Path, PathBuf};

pub use merge::merge3;

/// The file a document lives in, relative to a space root, or None if the
/// name is not a valid document name. Names are "later", "pins",
/// "notes/<slug>" and "days/YYYY-MM-DD"; nothing else can ever become a path.
pub fn doc_path(root: &Path, name: &str) -> Option<PathBuf> {
    match name {
        "later" => return Some(root.join("later.md")),
        "pins" => return Some(root.join("pins.md")),
        _ => {}
    }
    if let Some(slug) = name.strip_prefix("notes/") {
        return is_slug(slug).then(|| root.join("notes").join(format!("{slug}.md")));
    }
    let date = name.strip_prefix("days/")?;
    is_date(date).then(|| root.join("days").join(format!("{date}.md")))
}

/// `YYYY-MM-DD`, digits only. Not a calendar check, just a safe file name.
pub fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

/// A note's file name stem: 1 to 64 of `[a-z0-9-]`, not starting or ending
/// with `-`. Safe as a file name on every platform.
pub fn is_slug(s: &str) -> bool {
    let b = s.as_bytes();
    (1..=64).contains(&b.len())
        && b[0] != b'-'
        && b[b.len() - 1] != b'-'
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// Content hash used to detect concurrent edits. An absent document has the
/// hash of the empty string.
pub fn hash(content: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(content.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        let r = Path::new("/s");
        assert_eq!(doc_path(r, "later"), Some(PathBuf::from("/s/later.md")));
        assert_eq!(
            doc_path(r, "days/2026-10-15"),
            Some(PathBuf::from("/s/days/2026-10-15.md"))
        );
        assert_eq!(doc_path(r, "pins"), Some(PathBuf::from("/s/pins.md")));
        assert_eq!(
            doc_path(r, "notes/trip-ideas-2"),
            Some(PathBuf::from("/s/notes/trip-ideas-2.md"))
        );
        let long = "a".repeat(64);
        assert!(doc_path(r, &format!("notes/{long}")).is_some());
        for bad in [
            "../x",
            "days/../../x",
            "days/2026-1-15",
            "days/2026-10-15.md",
            "Later",
            "",
            "notes/../x",
            "notes/A",
            "notes/-x",
            "notes/x-",
            "notes/x.md",
            "notes/",
            "notes/a/b",
            "Pins",
        ] {
            assert_eq!(doc_path(r, bad), None, "{bad}");
        }
        assert_eq!(doc_path(r, &format!("notes/{long}a")), None);
    }
}
