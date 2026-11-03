use std::collections::BTreeMap;
use std::fmt;

use audit_rdf::slugify;

/// Processing status of a single file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FileStatus {
    /// Seen in inventory, not yet scanned.
    Pending,
    /// Scanned successfully; graph file exists.
    Processed,
    /// Previously processed but the content hash changed since.
    Stale,
    /// Scan attempted and failed.
    Failed,
}

impl FileStatus {
    /// The `audit:` local name used in the manifest.
    pub fn as_iri(self) -> &'static str {
        match self {
            FileStatus::Pending => "audit:Pending",
            FileStatus::Processed => "audit:Processed",
            FileStatus::Stale => "audit:Stale",
            FileStatus::Failed => "audit:Failed",
        }
    }

    pub fn from_iri(s: &str) -> Option<Self> {
        match s {
            "audit:Pending" => Some(FileStatus::Pending),
            "audit:Processed" => Some(FileStatus::Processed),
            "audit:Stale" => Some(FileStatus::Stale),
            "audit:Failed" => Some(FileStatus::Failed),
            _ => None,
        }
    }
}

impl fmt::Display for FileStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            FileStatus::Pending => "pending",
            FileStatus::Processed => "processed",
            FileStatus::Stale => "stale",
            FileStatus::Failed => "failed",
        };
        f.write_str(s)
    }
}

/// A directory discovered during inventory.
#[derive(Debug, Clone)]
pub struct DirEntry {
    /// IRI local part, e.g. `audit:d_src`.
    pub iri: String,
    /// Path relative to scan root, `/`-separated. Root itself is `""`.
    pub rel_path: String,
}

/// A file discovered during inventory, with its scan state.
#[derive(Debug, Clone)]
pub struct FileEntry {
    /// IRI local part, e.g. `audit:f_src_main_rs`.
    pub iri: String,
    /// Path relative to scan root, `/`-separated.
    pub rel_path: String,
    /// Detected language id (`rust`, `python`, `wgsl`) or `None`.
    pub language: Option<String>,
    /// `sha256:<hex>` of file contents at last inventory.
    pub content_hash: String,
    /// Filesystem mtime, RFC 3339.
    pub modified_at: String,
    pub status: FileStatus,
    /// RFC 3339 timestamp of last successful scan.
    pub processed_at: Option<String>,
    /// Graph output path relative to the output dir.
    pub graph_file: Option<String>,
    /// Last scan error, if status is `Failed`.
    pub error: Option<String>,
}

/// The full manifest for one scan root.
#[derive(Debug, Default)]
pub struct Manifest {
    /// Display name of the scan root (used in IRI base descriptions).
    pub root_name: String,
    /// Directories keyed by relative path ("" = root).
    pub dirs: BTreeMap<String, DirEntry>,
    /// Files keyed by relative path.
    pub files: BTreeMap<String, FileEntry>,
}

impl Manifest {
    /// Detect source language from a file extension.
    pub fn language_for(path: &str) -> Option<String> {
        let ext = path.rsplit('.').next()?;
        match ext {
            "rs" => Some("rust".to_string()),
            "py" | "pyi" => Some("python".to_string()),
            "wgsl" => Some("wgsl".to_string()),
            _ => None,
        }
    }
}

/// Mint a stable, unique Turtle IRI local name for a node.
///
/// Produces `{prefix}{slug}` normally; on collision within `used`, appends
/// `_h{8 hex}` of the path's FNV-1a hash so results stay deterministic.
pub fn mint_iri(prefix: &str, path: &str, used: &mut std::collections::HashSet<String>) -> String {
    let base = format!(
        "audit:{prefix}{}",
        slugify(if path.is_empty() { "root" } else { path })
    );
    if used.insert(base.clone()) {
        return base;
    }
    let mut h: u64 = 0xcbf29ce484222325;
    for b in path.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    let iri = format!("{base}_h{h:08x}");
    used.insert(iri.clone());
    iri
}
