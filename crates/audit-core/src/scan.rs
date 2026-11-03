//! Scan orchestration: pick manifest files, run analyzers, emit Turtle graphs.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use audit_extract::{analyzer_for, FileAnalysis, FunctionKind};
use audit_rdf::{slugify, Term, TurtleWriter};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::domains::{classify, Domain};
use crate::model::{FileStatus, Manifest};

/// Which files a scan run should process.
#[derive(Debug, Default)]
pub struct ScanFilter {
    /// Also rescan `Processed` files (default: pending + stale + failed only).
    pub all: bool,
    /// Only files under this relative directory prefix.
    pub dir: Option<String>,
    /// Only this language id.
    pub lang: Option<String>,
    /// Cap on number of files processed this run.
    pub max_files: Option<usize>,
}

pub struct ScanReport {
    pub processed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub selected: usize,
}

/// Select which files this run will touch.
fn select(m: &Manifest, f: &ScanFilter) -> Vec<String> {
    let mut picked: Vec<String> = m
        .files
        .values()
        .filter(|e| {
            let status_ok = f.all
                || matches!(
                    e.status,
                    FileStatus::Pending | FileStatus::Stale | FileStatus::Failed
                );
            let dir_ok = f
                .dir
                .as_ref()
                .map(|d| e.rel_path == *d || e.rel_path.starts_with(&format!("{d}/")))
                .unwrap_or(true);
            let lang_ok = f
                .lang
                .as_ref()
                .map(|l| e.language.as_deref() == Some(l.as_str()))
                .unwrap_or(true);
            status_ok && dir_ok && lang_ok && e.language.is_some()
        })
        .map(|e| e.rel_path.clone())
        .collect();
    picked.sort();
    if let Some(max) = f.max_files {
        picked.truncate(max);
    }
    picked
}

/// Relative graph output path for a file, e.g. `graphs/src_main_rs.ttl`.
pub fn graph_path_for(rel_path: &str) -> PathBuf {
    PathBuf::from("graphs").join(format!("{}.ttl", slugify(rel_path)))
}

/// Library node IRI for a library/root name.
pub fn lib_iri(lib_name: &str) -> String {
    format!("audit:lib_{}", slugify(lib_name))
}

/// Cross-library canonical identity of a function: same name in the same
/// language → same IRI, so two libraries' graphs unify in one store.
pub fn canonical_iri(lang: &str, name: &str) -> String {
    format!("audit:canon_{}_{}", slugify(lang), slugify(name))
}

/// Mint per-file function IRIs: `audit:fn_{file_slug}_{scope_}{name}` with a
/// numeric suffix on collisions. Deterministic given the same analysis — the
/// `resolve` pass mints identical IRIs by reusing this.
pub fn mint_fn_iris(file_slug: &str, functions: &[audit_extract::ExtractedFunction]) -> Vec<String> {
    let mut used: std::collections::HashSet<String> = Default::default();
    let mut iris = Vec::with_capacity(functions.len());
    for f in functions {
        let scope_slug = f
            .scope
            .as_deref()
            .map(|s| format!("{}_", slugify(s)))
            .unwrap_or_default();
        let mut iri = format!("audit:fn_{file_slug}_{scope_slug}{}", slugify(&f.name));
        let mut n = 1;
        while !used.insert(iri.clone()) {
            n += 1;
            iri = format!("{iri}_{n}");
        }
        iris.push(iri);
    }
    iris
}

/// Scan the selected files: analyze, write per-file Turtle graphs, update the
/// manifest in memory (caller persists it).
pub fn scan_files(
    root: &Path,
    out_dir: &Path,
    m: &mut Manifest,
    filter: &ScanFilter,
    domains: &[Domain],
    lib_name: &str,
) -> Result<ScanReport> {
    let picked = select(m, filter);
    let mut report = ScanReport {
        processed: 0,
        failed: 0,
        skipped: m.files.len().saturating_sub(picked.len()),
        selected: picked.len(),
    };

    for rel in picked {
        let entry = m.files.get(&rel).unwrap().clone();
        let abs = root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        let lang = entry.language.clone().unwrap_or_default();

        let outcome: Result<String> = (|| {
            let source = crate::read_source::read_source(&abs)
                .with_context(|| format!("reading {}", abs.display()))?;
            let analyzer = analyzer_for(&lang)
                .ok_or_else(|| anyhow::anyhow!("no analyzer for {lang}"))?;
            let analysis = analyzer.analyze(&source)?;
            let turtle = file_graph(&entry, &analysis, domains, lib_name);
            let graph_rel = graph_path_for(&rel);
            let graph_abs = out_dir.join(&graph_rel);
            if let Some(p) = graph_abs.parent() {
                std::fs::create_dir_all(p)?;
            }
            std::fs::write(&graph_abs, turtle)
                .with_context(|| format!("writing {}", graph_abs.display()))?;
            Ok(graph_rel.to_string_lossy().replace('\\', "/"))
        })();

        let now = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_else(|_| "1970-01-01T00:00:00Z".into());
        let e = m.files.get_mut(&rel).unwrap();
        match outcome {
            Ok(graph_rel) => {
                e.status = FileStatus::Processed;
                e.processed_at = Some(now);
                e.graph_file = Some(graph_rel);
                e.error = None;
                report.processed += 1;
            }
            Err(err) => {
                e.status = FileStatus::Failed;
                e.error = Some(err.to_string().replace('\n', " "));
                report.failed += 1;
            }
        }
    }
    Ok(report)
}

/// Build the Turtle graph for one analyzed file.
fn file_graph(
    entry: &crate::model::FileEntry,
    analysis: &FileAnalysis,
    domains: &[Domain],
    lib_name: &str,
) -> String {
    let mut w = TurtleWriter::new();
    let file_iri = &entry.iri;
    let file_slug = file_iri.trim_start_matches("audit:");

    w.triple(file_iri, "a", Term::iri("audit:SourceFile"));
    w.triple(file_iri, "audit:relativePath", Term::lit(&entry.rel_path));
    w.triple(
        file_iri,
        "audit:inLibrary",
        Term::iri(lib_iri(lib_name)),
    );
    if let Some(lang) = &entry.language {
        w.triple(file_iri, "audit:language", Term::lit(lang));
    }
    w.triple(file_iri, "audit:contentHash", Term::lit(&entry.content_hash));

    // Mint function IRIs first so comments can reference them.
    let fn_iris = mint_fn_iris(file_slug, &analysis.functions);

    // For each function, the comment indices forming its doc block
    // (contiguous doc comments ending on the line above the function).
    let doc_runs: Vec<Vec<usize>> = analysis
        .functions
        .iter()
        .map(|f| {
            let mut idxs = Vec::new();
            let mut cursor = f.line;
            loop {
                match analysis
                    .comments
                    .iter()
                    .position(|c| c.outer && c.end_line + 1 == cursor)
                {
                    Some(i) => {
                        idxs.push(i);
                        cursor = analysis.comments[i].line;
                    }
                    None => break,
                }
            }
            idxs
        })
        .collect();

    for (f, fn_iri) in analysis.functions.iter().zip(&fn_iris) {
        let class = if f.kind == FunctionKind::Method {
            "audit:Method"
        } else {
            "audit:Function"
        };
        w.triple(file_iri, "audit:hasFunction", Term::iri(fn_iri));
        w.triple(fn_iri, "a", Term::iri(class));
        w.triple(fn_iri, "audit:name", Term::lit(&f.name));
        w.triple(fn_iri, "audit:signature", Term::lit(&f.signature));
        w.triple(fn_iri, "audit:line", Term::int(f.line as i64));
        w.triple(fn_iri, "audit:endLine", Term::int(f.end_line as i64));
        w.triple(fn_iri, "audit:belongsTo", Term::iri(file_iri));
        w.triple(fn_iri, "audit:arity", Term::int(f.arity as i64));
        w.triple(
            fn_iri,
            "audit:visibility",
            Term::lit(if f.public { "pub" } else { "private" }),
        );
        if let Some(lang) = &entry.language {
            w.triple(
                fn_iri,
                "audit:canonical",
                Term::iri(canonical_iri(lang, &f.name)),
            );
        }
        if let Some(scope) = &f.scope {
            w.triple(fn_iri, "audit:inScope", Term::lit(scope));
        }

        // Semantic layer: calls (name-resolved), types, self-field access.
        if let Some(lang) = &entry.language {
            let mut seen = std::collections::HashSet::new();
            for c in &f.body.calls {
                if seen.insert(c.name.as_str()) {
                    w.triple(
                        fn_iri,
                        "audit:calls",
                        Term::iri(canonical_iri(lang, &c.name)),
                    );
                }
            }
        }
        let mut seen_target = std::collections::HashSet::new();
        let mut seen_expr = std::collections::HashSet::new();
        for c in &f.body.calls {
            let target = match &c.qualifier {
                Some(q) => {
                    let sep = if c.kind == audit_extract::CallKind::Method {
                        "."
                    } else {
                        "::"
                    };
                    format!("{q}{sep}{}", c.name)
                }
                None => c.name.clone(),
            };
            if c.qualifier.is_some() && seen_target.insert(target.clone()) {
                w.triple(fn_iri, "audit:callTarget", Term::lit(&target));
            }
            if c.kind != audit_extract::CallKind::Macro {
                let expr = format!("{target}/{}", c.arity);
                if seen_expr.insert(expr.clone()) {
                    w.triple(fn_iri, "audit:callExpr", Term::lit(&expr));
                }
            }
        }
        let mut seen_macro = std::collections::HashSet::new();
        for c in &f.body.calls {
            if c.kind == audit_extract::CallKind::Macro
                && seen_macro.insert(c.name.as_str())
            {
                w.triple(
                    fn_iri,
                    "audit:invokesMacro",
                    Term::lit(&format!("{}!", c.name)),
                );
            }
        }
        if !f.body.calls.is_empty() {
            w.triple(
                fn_iri,
                "audit:callCount",
                Term::int(f.body.calls.len() as i64),
            );
        }
        for pt in &f.body.param_types {
            w.triple(fn_iri, "audit:paramType", Term::lit(pt));
        }
        if let Some(rt) = &f.body.return_type {
            w.triple(fn_iri, "audit:returnType", Term::lit(rt));
        }
        for field in &f.body.reads_fields {
            w.triple(fn_iri, "audit:readsField", Term::lit(field));
        }
        for field in &f.body.writes_fields {
            w.triple(fn_iri, "audit:writesField", Term::lit(field));
        }
        if f.body.mutates_receiver {
            w.triple(fn_iri, "audit:mutatesReceiver", Term::bool(true));
        }

        if let Some(doc) = &f.doc {
            w.triple(fn_iri, "audit:doc", Term::lit(doc));
            // `docref:` marker → external documentation pointer.
            if let Some(pos) = doc.find("docref:") {
                let target: String = doc[pos + 7..]
                    .chars()
                    .take_while(|c| !c.is_whitespace())
                    .collect();
                if !target.is_empty() {
                    w.triple(fn_iri, "audit:docRef", Term::lit(&target));
                }
            }
        }

        let hay = format!(
            "{} {} {}",
            f.name,
            f.signature,
            f.doc.as_deref().unwrap_or("")
        );
        for d in classify(domains, &hay) {
            w.triple(fn_iri, "audit:hasDomain", Term::iri(d));
        }
    }

    for (i, c) in analysis.comments.iter().enumerate() {
        let c_iri = format!("audit:c_{file_slug}_{i}");
        w.triple(&c_iri, "a", Term::iri("audit:Comment"));
        w.triple(&c_iri, "audit:text", Term::lit(&c.text));
        w.triple(&c_iri, "audit:line", Term::int(c.line as i64));
        w.triple(&c_iri, "audit:endLine", Term::int(c.end_line as i64));
        w.triple(&c_iri, "audit:isDocComment", Term::bool(c.is_doc));
        w.triple(&c_iri, "audit:belongsTo", Term::iri(file_iri));
        for (fi, run) in doc_runs.iter().enumerate() {
            if run.contains(&i) {
                w.triple(&c_iri, "audit:annotates", Term::iri(&fn_iris[fi]));
            }
        }
    }

    w.render()
}
