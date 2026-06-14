//! Git repository provenance captured at index time (Wave 7 — the "live brain").
//!
//! Two layers of git identity:
//!   * [`RepoInfo`] — repo-wide HEAD state (commit / branch / remote / dirty), stored once in `meta`.
//!   * per-file **git blob SHA** — the content-addressed id git itself uses (`git hash-object`):
//!     `sha1("blob " + byte_len + "\0" + content)`. Stable across renames, identical for identical
//!     content. Recorded on the `files` row so the graph can correlate to git history and key
//!     retained old file versions by blob SHA (content-addressed history, dedup for free).

use serde::{Deserialize, Serialize};

/// Repo-wide git state at the moment of indexing. All fields are `Option`/`false` when the indexed
/// path is not a git repo (or git is unavailable) — git provenance is additive, never required.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoInfo {
    /// HEAD commit SHA.
    pub commit: Option<String>,
    /// Current branch name (`None` in detached-HEAD).
    pub branch: Option<String>,
    /// `origin` remote URL, if any.
    pub remote: Option<String>,
    /// Working tree had uncommitted changes at index time (the graph reflects dirty state).
    pub dirty: bool,
}
