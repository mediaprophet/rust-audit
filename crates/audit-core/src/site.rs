//! Static site generation: analyze the tree and emit a single self-contained
//! `index.html` with embedded JSON — hostable anywhere static, no server.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use audit_extract::{analyzer_for, FunctionKind};
use serde::Serialize;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::domains::{classify, Domain};
use crate::inventory::build_inventory;
use crate::manifest::load_manifest;

const TEMPLATE: &str = include_str!("../assets/site.html");

#[derive(Serialize)]
struct SiteFn {
    name: String,
    signature: String,
    line: u32,
    end_line: u32,
    kind: &'static str,
    scope: Option<String>,
    arity: u32,
    doc: Option<String>,
    domains: Vec<String>,
    /// Distinct callee names (qualified where known), name-resolved.
    calls: Vec<String>,
}

#[derive(Serialize)]
struct SiteComment {
    text: String,
    line: u32,
    is_doc: bool,
}

#[derive(Serialize)]
struct SiteFile {
    path: String,
    lang: Option<String>,
    status: String,
    processed_at: Option<String>,
    hash: String,
    functions: Vec<SiteFn>,
    comments: Vec<SiteComment>,
}

#[derive(Serialize)]
struct SiteData {
    library: String,
    generated_at: String,
    files: Vec<SiteFile>,
}

/// Build `<out_dir>/site/index.html` for `root`. Reuses the manifest for
/// status info, analyzes source files live (fresh results regardless of
/// scan state).
pub fn build_site(root: &Path, out_dir: &Path, domains: &[Domain]) -> Result<PathBuf> {
    let prior = load_manifest(&out_dir.join("manifest.ttl")).unwrap_or_default();
    let m = build_inventory(root, &prior)?;

    let mut files = Vec::with_capacity(m.files.len());
    for entry in m.files.values() {
        let mut functions = Vec::new();
        let mut comments = Vec::new();
        if let Some(lang) = &entry.language {
            if let Some(analyzer) = analyzer_for(lang) {
                let abs =
                    root.join(entry.rel_path.replace('/', std::path::MAIN_SEPARATOR_STR));
                if let Ok(src) = crate::read_source::read_source(&abs) {
                    if let Ok(a) = analyzer.analyze(&src) {
                        for f in a.functions {
                            let hay = format!(
                                "{} {} {}",
                                f.name,
                                f.signature,
                                f.doc.as_deref().unwrap_or("")
                            );
                            let mut seen = std::collections::HashSet::new();
                            let calls: Vec<String> = f
                                .body
                                .calls
                                .iter()
                                .filter_map(|c| {
                                    let t = match &c.qualifier {
                                        Some(q) => format!("{q}.{}", c.name),
                                        None => c.name.clone(),
                                    };
                                    seen.insert(t).then(|| {
                                        match &c.qualifier {
                                            Some(q) => format!("{q}.{}", c.name),
                                            None => c.name.clone(),
                                        }
                                    })
                                })
                                .collect();
                            functions.push(SiteFn {
                                name: f.name,
                                signature: f.signature,
                                line: f.line,
                                end_line: f.end_line,
                                kind: if f.kind == FunctionKind::Method {
                                    "method"
                                } else {
                                    "function"
                                },
                                scope: f.scope,
                                arity: f.arity,
                                doc: f.doc,
                                domains: classify(domains, &hay)
                                    .iter()
                                    .map(|d| d.trim_start_matches("audit:dom_").replace('_', " "))
                                    .collect(),
                                calls,
                            });
                        }
                        for c in a.comments {
                            comments.push(SiteComment {
                                text: c.text,
                                line: c.line,
                                is_doc: c.is_doc,
                            });
                        }
                    }
                }
            }
        }
        files.push(SiteFile {
            path: entry.rel_path.clone(),
            lang: entry.language.clone(),
            status: entry.status.to_string(),
            processed_at: entry.processed_at.clone(),
            hash: entry
                .content_hash
                .trim_start_matches("sha256:")
                .chars()
                .take(8)
                .collect(),
            functions,
            comments,
        });
    }

    let data = SiteData {
        library: m.root_name.clone(),
        generated_at: OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_else(|_| "?".into()),
        files,
    };
    let json = serde_json::to_string(&data)?;
    let html = TEMPLATE.replace("__AUDIT_DATA__", &json);

    let site_dir = out_dir.join("site");
    std::fs::create_dir_all(&site_dir)?;
    let index = site_dir.join("index.html");
    std::fs::write(&index, html).with_context(|| format!("writing {}", index.display()))?;
    Ok(index)
}
