//! Directory-tree inventory: walks a root (honoring `.gitignore`), hashes
//! every file, and merges results into the manifest.

use std::collections::HashSet;
use std::path::Path;

use anyhow::{Context, Result};
use ignore::WalkBuilder;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::hash::content_hash;
use crate::model::{mint_iri, DirEntry, FileEntry, FileStatus, Manifest};

fn system_time_to_rfc3339(t: std::time::SystemTime) -> String {
    OffsetDateTime::from(t)
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

fn normalize_rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Walk `root`, build/refresh the manifest, preserving prior scan state.
///
/// - New files → `Pending`.
/// - Files whose content hash changed → `Stale` if previously `Processed`,
///   `Pending` otherwise.
/// - Unchanged files keep their status and timestamps.
/// - Paths that disappeared are dropped.
pub fn build_inventory(root: &Path, prior: &Manifest) -> Result<Manifest> {
    let root = root
        .canonicalize()
        .with_context(|| format!("scan root {} not found", root.display()))?;

    let root_name = if prior.root_name.is_empty() {
        root.file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "root".to_string())
    } else {
        prior.root_name.clone()
    };

    let mut m = Manifest {
        root_name,
        ..Default::default()
    };
    // Seed with prior IRIs: reused nodes keep theirs, fresh mints avoid them.
    let mut used_iris: HashSet<String> = prior
        .files
        .values()
        .map(|f| f.iri.clone())
        .chain(prior.dirs.values().map(|d| d.iri.clone()))
        .collect();

    // Root directory node.
    let root_iri = match prior.dirs.get("") {
        Some(d) => d.iri.clone(),
        None => mint_iri("d_", "", &mut used_iris),
    };
    used_iris.insert(root_iri.clone());
    m.dirs.insert(
        String::new(),
        DirEntry {
            iri: root_iri,
            rel_path: String::new(),
        },
    );

    let walker = WalkBuilder::new(&root)
        .hidden(true) // skip dotfiles/dirs (also skips our own .audit out dir)
        .build();

    for entry in walker {
        let entry = entry?;
        let path = entry.path();
        if path == root {
            continue;
        }
        let rel = normalize_rel(&root, path);
        let file_type = match entry.file_type() {
            Some(ft) => ft,
            None => continue,
        };

        if file_type.is_dir() {
            let iri = match prior.dirs.get(&rel) {
                Some(d) => d.iri.clone(),
                None => mint_iri("d_", &rel, &mut used_iris),
            };
            used_iris.insert(iri.clone());
            m.dirs.insert(rel.clone(), DirEntry { iri, rel_path: rel });
        } else if file_type.is_file() {
            let hash = content_hash(path)
                .with_context(|| format!("hashing {}", path.display()))?;
            let modified = entry
                .metadata()
                .ok()
                .and_then(|md| md.modified().ok())
                .map(system_time_to_rfc3339)
                .unwrap_or_else(|| "1970-01-01T00:00:00Z".to_string());
            let language = Manifest::language_for(&rel);

            let (status, processed_at, graph_file, error, iri) =
                match prior.files.get(&rel) {
                    Some(old) if old.content_hash == hash => (
                        old.status,
                        old.processed_at.clone(),
                        old.graph_file.clone(),
                        old.error.clone(),
                        old.iri.clone(),
                    ),
                    Some(old) => (
                        if old.status == FileStatus::Processed {
                            FileStatus::Stale
                        } else {
                            FileStatus::Pending
                        },
                        old.processed_at.clone(),
                        old.graph_file.clone(),
                        None,
                        old.iri.clone(),
                    ),
                    None => (
                        FileStatus::Pending,
                        None,
                        None,
                        None,
                        mint_iri("f_", &rel, &mut used_iris),
                    ),
                };
            used_iris.insert(iri.clone());

            m.files.insert(
                rel.clone(),
                FileEntry {
                    iri,
                    rel_path: rel,
                    language,
                    content_hash: hash,
                    modified_at: modified,
                    status,
                    processed_at,
                    graph_file,
                    error,
                },
            );
        }
    }
    Ok(m)
}
