//! Shared collector: fresh inventory + per-file analysis for every source
//! file. Used by compare (needs functions) and docs/site (need everything).

use std::path::Path;

use anyhow::{Context, Result};
use audit_extract::{analyzer_for, FileAnalysis};

use crate::inventory::build_inventory;
use crate::model::Manifest;

/// One source file with its analysis (None when analysis failed/unavailable).
pub struct FileRecord {
    pub rel_path: String,
    /// Manifest-minted IRI (matches the graph output of `audit scan`).
    pub file_iri: String,
    pub lang: String,
    pub analysis: FileAnalysis,
}

/// Inventory the tree and run analyzers over all recognized source files.
/// Returns the manifest plus one record per analyzable file.
pub fn collect_analyses(root: &Path) -> Result<(Manifest, Vec<FileRecord>)> {
    let m = build_inventory(root, &Default::default())?;
    let mut records = Vec::new();
    for entry in m.files.values() {
        let Some(lang) = &entry.language else {
            continue;
        };
        let Some(analyzer) = analyzer_for(lang) else {
            continue;
        };
        let abs = root.join(entry.rel_path.replace('/', std::path::MAIN_SEPARATOR_STR));
        let source = crate::read_source::read_source(&abs)
            .with_context(|| format!("reading {}", abs.display()))?;
        let analysis = analyzer.analyze(&source)?;
        records.push(FileRecord {
            rel_path: entry.rel_path.clone(),
            file_iri: entry.iri.clone(),
            lang: lang.clone(),
            analysis,
        });
    }
    Ok((m, records))
}
