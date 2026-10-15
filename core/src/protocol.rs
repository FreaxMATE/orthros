//! The sync protocol: plain JSON over HTTPS, bearer token.
//!
//!   GET /api/changes?since=N   every document changed after revision N
//!   PUT /api/doc/<name>        upload, if the server still has `base`
//!   GET /api/wait?since=N      answers as soon as the revision passes N
//!                              (or after ~50 s): clients keep one open to
//!                              hear about other devices' edits at once
//!
//! The server keeps one global revision counter; each accepted upload bumps
//! it. A client remembers the last revision it saw and, per document, the
//! content it last agreed on with the server (its base). An upload names the
//! hash of that base: if someone else changed the document meanwhile, the
//! server refuses with its current content, the client merges and retries.
//! Deleting a document is uploading empty content.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RemoteDoc {
    pub name: String,
    pub rev: u64,
    /// Empty when the document was deleted.
    pub content: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Changes {
    /// The server's current revision: the client's next `since`.
    pub rev: u64,
    pub docs: Vec<RemoteDoc>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Put {
    /// Hash of the content this edit was based on (`hash("")` for new).
    pub base: String,
    pub content: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum PutResult {
    /// Stored as revision `rev`.
    Ok { rev: u64 },
    /// The server's copy moved on: merge with `current` and try again.
    Conflict { rev: u64, current: String },
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Wait {
    /// The server's current revision.
    pub rev: u64,
}
