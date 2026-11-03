//! Generate human/agent-facing documentation from scan data:
//!
//! - `INDEX.md`      — overview, stats, agent-verification protocol
//! - `FUNCTIONS.md`  — complete alphabetical function index (grep target)
//! - `CAPABILITIES.md` — functional-purpose overview per module + domain
//! - `DOCS-COVERAGE.md` — doc-comment coverage + undocumented-public worklist
//! - `docrefs.ttl`   — RDF overlay linking every function to its module doc
//! - `modules/*.md`  — per-module function registry
//!
//! "Module" = nearest ancestor dir holding a manifest file (Cargo.toml,
//! pyproject.toml, setup.py); fallback = top-level directory.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use audit_extract::{ExtractedFunction, FunctionKind};
use audit_rdf::slugify;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::collect::{collect_analyses, FileRecord};
use crate::domains::{classify, Domain};

const MODULE_MARKERS: &[&str] = &["Cargo.toml", "pyproject.toml", "setup.py", "package.json"];

const AGENT_PROTOCOL: &str = "\
> **GENERATED — do not edit.** Regenerate with `audit docs`.
>
> **Agent verification protocol:** before implementing, renaming, or claiming
> a function/module does not exist, grep `FUNCTIONS.md` (and the relevant
> `modules/*.md`) for the name or related terms. If it appears here, it exists
> at the listed `path:line` — read the source before writing anything new.
> Absence from this index means absence from the codebase as of the
> generation timestamp, not a guess.
";

struct FnInfo {
    rec_idx: usize,
    f: ExtractedFunction,
    domains: Vec<String>,
}

/// Which module (crate boundary) a relative path belongs to.
fn module_of<'a>(rel_path: &'a str, module_dirs: &'a BTreeMap<String, String>) -> &'a str {
    let mut dir = match rel_path.rfind('/') {
        Some(i) => &rel_path[..i],
        None => "",
    };
    loop {
        if let Some(m) = module_dirs.get(dir) {
            return m;
        }
        match dir.rfind('/') {
            Some(i) => dir = &dir[..i],
            None => break,
        }
    }
    // fallback: top-level segment
    rel_path.split('/').next().unwrap_or("root")
}

/// Inner doc comments (`//!`, `/*!`) of a file → module doc text.
fn module_doc(rec: &FileRecord) -> Option<String> {
    let mut lines = Vec::new();
    for c in &rec.analysis.comments {
        if c.is_doc && !c.outer {
            lines.push(c.text.as_str());
        }
    }
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or("").trim()
}

/// Mint function IRIs identical to `scan.rs` (`audit:fn_{file_slug}_{scope}{name}`).
fn fn_iris(rec: &FileRecord) -> Vec<String> {
    let file_slug = rec.file_iri.trim_start_matches("audit:");
    let mut used = std::collections::HashSet::new();
    rec.analysis
        .functions
        .iter()
        .map(|f| {
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
            iri
        })
        .collect()
}

pub fn generate_docs(
    root: &Path,
    out_dir: &Path,
    domains: &[Domain],
) -> Result<Vec<PathBuf>> {
    let (m, records) = collect_analyses(root)?;
    let now = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "?".into());

    // module dirs: dir -> module name (dir path as display name)
    let mut module_dirs: BTreeMap<String, String> = BTreeMap::new();
    for rel in m.files.keys() {
        if let Some(name) = rel.rsplit('/').next() {
            if MODULE_MARKERS.contains(&name) {
                match rel.rfind('/') {
                    // Marker inside a dir → that dir is a module root.
                    Some(i) => {
                        module_dirs.insert(rel[..i].to_string(), rel[..i].to_string());
                    }
                    // Marker at tree root → root itself is the module.
                    None => {
                        module_dirs.insert(String::new(), m.root_name.clone());
                    }
                }
            }
        }
    }

    // Gather function info per record.
    let mut all_fns: Vec<FnInfo> = Vec::new();
    for (i, rec) in records.iter().enumerate() {
        for f in &rec.analysis.functions {
            let hay = format!(
                "{} {} {}",
                f.name,
                f.signature,
                f.doc.as_deref().unwrap_or("")
            );
            let doms: Vec<String> = classify(domains, &hay)
                .iter()
                .map(|d| d.trim_start_matches("audit:dom_").replace('_', " "))
                .collect();
            all_fns.push(FnInfo {
                rec_idx: i,
                f: f.clone(),
                domains: doms,
            });
        }
    }

    // module name -> record indices
    let mut by_module: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, rec) in records.iter().enumerate() {
        by_module
            .entry(module_of(&rec.rel_path, &module_dirs).to_string())
            .or_default()
            .push(i);
    }

    std::fs::create_dir_all(out_dir)?;
    // Remove only the artifacts this generator owns — stale docs are worse
    // than missing docs for the agent-verification use case.
    let mod_dir_prev = out_dir.join("modules");
    if mod_dir_prev.exists() {
        std::fs::remove_dir_all(&mod_dir_prev)?;
    }
    for f in [
        "INDEX.md",
        "FUNCTIONS.md",
        "CAPABILITIES.md",
        "DOCS-COVERAGE.md",
        "docrefs.ttl",
    ] {
        let p = out_dir.join(f);
        if p.exists() {
            std::fs::remove_file(&p)?;
        }
    }
    let mut written = Vec::new();

    // ---- FUNCTIONS.md -----------------------------------------------------
    let mut fns_sorted: Vec<&FnInfo> = all_fns.iter().collect();
    fns_sorted.sort_by(|a, b| {
        a.f.name
            .to_lowercase()
            .cmp(&b.f.name.to_lowercase())
            .then(a.f.name.cmp(&b.f.name))
    });
    let mut idx = String::from(
        "# Function Index\n\nEvery function/method found in the codebase, alphabetical.\n\n",
    );
    idx.push_str(AGENT_PROTOCOL);
    idx.push_str(
        "\n| function | arity | location | domains | doc |\n|---|---|---|---|---|\n",
    );
    for fi in &fns_sorted {
        let rec = &records[fi.rec_idx];
        let scope = fi
            .f
            .scope
            .as_deref()
            .map(|s| format!("{s}::"))
            .unwrap_or_default();
        let doms = if fi.domains.is_empty() {
            "-".into()
        } else {
            fi.domains.join(", ")
        };
        let doc = fi.f.doc.as_deref().map(first_line).unwrap_or("");
        idx.push_str(&format!(
            "| `{scope}{}` | {} | `{}:{}` | {} | {} |\n",
            fi.f.name,
            fi.f.arity,
            rec.rel_path,
            fi.f.line,
            doms,
            doc.replace('|', "\\|")
        ));
    }
    let p = out_dir.join("FUNCTIONS.md");
    std::fs::write(&p, idx)?;
    written.push(p);

    // ---- modules/<slug>.md -------------------------------------------------
    let mod_dir = out_dir.join("modules");
    std::fs::create_dir_all(&mod_dir)?;
    for (modname, recs) in &by_module {
        let slug = audit_rdf::slugify(modname);
        let mut md = format!("# `{modname}`\n\n{AGENT_PROTOCOL}\n\n");
        // module doc from lib.rs/mod.rs/__init__.py inside this module
        for &i in recs {
            let rec = &records[i];
            let base = rec.rel_path.rsplit('/').next().unwrap_or("");
            if matches!(base, "lib.rs" | "mod.rs" | "main.rs" | "__init__.py") {
                if let Some(d) = module_doc(rec) {
                    md.push_str(&format!("_{}_\n\n", d.trim()));
                    break;
                }
            }
        }
        md.push_str("| function | arity | location | domains | doc |\n|---|---|---|---|---|\n");
        let mut fns: Vec<&FnInfo> = all_fns
            .iter()
            .filter(|fi| recs.contains(&fi.rec_idx))
            .collect();
        fns.sort_by_key(|fi| (records[fi.rec_idx].rel_path.clone(), fi.f.line));
        for fi in fns {
            let rec = &records[fi.rec_idx];
            let scope = fi
                .f
                .scope
                .as_deref()
                .map(|s| format!("{s}::"))
                .unwrap_or_default();
            let doms = if fi.domains.is_empty() {
                "-".into()
            } else {
                fi.domains.join(", ")
            };
            let doc = fi.f.doc.as_deref().map(first_line).unwrap_or("");
            md.push_str(&format!(
                "| `{scope}{}` | {} | `{}:{}` | {} | {} |\n",
                fi.f.name,
                fi.f.arity,
                rec.rel_path,
                fi.f.line,
                doms,
                doc.replace('|', "\\|")
            ));
        }
        let p = mod_dir.join(format!("{slug}.md"));
        std::fs::write(&p, md)?;
        written.push(p);
    }

    // ---- CAPABILITIES.md ---------------------------------------------------
    let mut cap = format!(
        "# {} — capabilities\n\nFunctional overview generated from source on {}.\n\
         {} source files, {} functions ({} documented).\n\n{AGENT_PROTOCOL}\n\n",
        m.root_name,
        now,
        records.len(),
        all_fns.len(),
        all_fns.iter().filter(|fi| fi.f.doc.is_some()).count()
    );
    cap.push_str("## Domain coverage\n\n| domain | functions |\n|---|---|\n");
    let mut dom_counts: BTreeMap<String, usize> = BTreeMap::new();
    for fi in &all_fns {
        for d in &fi.domains {
            *dom_counts.entry(d.clone()).or_insert(0) += 1;
        }
    }
    for (d, n) in &dom_counts {
        cap.push_str(&format!("| {d} | {n} |\n"));
    }
    cap.push_str("\n## Modules\n\n");
    for (modname, recs) in &by_module {
        let n_fns: usize = recs
            .iter()
            .map(|&i| records[i].analysis.functions.len())
            .sum();
        let n_doc: usize = recs
            .iter()
            .flat_map(|&i| records[i].analysis.functions.iter())
            .filter(|f| f.doc.is_some())
            .count();
        cap.push_str(&format!(
            "### `{modname}` — {} function{} ({} documented)\n\n",
            n_fns,
            if n_fns == 1 { "" } else { "s" },
            n_doc
        ));
        // module doc
        for &i in recs {
            let rec = &records[i];
            let base = rec.rel_path.rsplit('/').next().unwrap_or("");
            if matches!(base, "lib.rs" | "mod.rs" | "main.rs" | "__init__.py") {
                if let Some(d) = module_doc(rec) {
                    cap.push_str(&format!("_{}_\n\n", d.trim()));
                    break;
                }
            }
        }
        // documented fns grouped by domain
        let mut by_dom: BTreeMap<String, Vec<&FnInfo>> = BTreeMap::new();
        let mut undoc = 0usize;
        for &i in recs {
            for fi in all_fns.iter().filter(|fi| fi.rec_idx == i) {
                if fi.f.doc.is_none() {
                    undoc += 1;
                    continue;
                }
                let key = fi
                    .domains
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "general".into());
                by_dom.entry(key).or_default().push(fi);
            }
        }
        for (d, fns) in &by_dom {
            cap.push_str(&format!("**{d}**\n\n"));
            for fi in fns {
                let kind = if fi.f.kind == FunctionKind::Method {
                    "method"
                } else {
                    "function"
                };
                cap.push_str(&format!(
                    "- `{}` ({kind}) — {}\n",
                    fi.f.name,
                    first_line(fi.f.doc.as_deref().unwrap_or(""))
                ));
            }
            cap.push('\n');
        }
        if undoc > 0 {
            cap.push_str(&format!("_{undoc} undocumented function{} — see `modules/{}.md`._\n\n", if undoc == 1 { "" } else { "s" }, audit_rdf::slugify(modname)));
        }
    }
    let p = out_dir.join("CAPABILITIES.md");
    std::fs::write(&p, cap)?;
    written.push(p);

    // ---- DOCS-COVERAGE.md --------------------------------------------------
    // Per-module doc-comment coverage + a prioritized undocumented-public
    // worklist. "Public" follows each language's rules (pub / non-_-prefixed).
    let mut cov = String::from(
        "# Documentation coverage\n\n\
         A `///`/`/**`/docstring attached to a function counts as documented. \
         Test functions, private helpers and WGSL (no doc-comment convention) \
         lower the raw rate — track **public coverage** as the KPI.\n\n",
    );
    cov.push_str(AGENT_PROTOCOL);
    cov.push_str(
        "\n| module | functions | documented | coverage | undocumented | undocumented public |\n|---|---|---|---|---|---|\n",
    );
    let n_doc_total = all_fns.iter().filter(|fi| fi.f.doc.is_some()).count();
    let n_pub = all_fns.iter().filter(|fi| fi.f.public).count();
    let n_pub_doc = all_fns
        .iter()
        .filter(|fi| fi.f.public && fi.f.doc.is_some())
        .count();
    // module -> (total, documented, pub_undocumented, pub_undoc_fns)
    let mut mod_stats: BTreeMap<String, (usize, usize, Vec<&FnInfo>)> = BTreeMap::new();
    for fi in &all_fns {
        let m = module_of(&records[fi.rec_idx].rel_path, &module_dirs).to_string();
        let e = mod_stats.entry(m).or_default();
        e.0 += 1;
        if fi.f.doc.is_some() {
            e.1 += 1;
        } else if fi.f.public {
            e.2.push(fi);
        }
    }
    let mut ranked: Vec<_> = mod_stats.iter().collect();
    ranked.sort_by(|a, b| b.1 .2.len().cmp(&a.1 .2.len()).then(a.0.cmp(b.0)));
    for (modname, (total, documented, undoc_pub)) in &ranked {
        let pct = if *total > 0 {
            format!("{:.0}%", 100.0 * *documented as f64 / *total as f64)
        } else {
            "-".into()
        };
        cov.push_str(&format!(
            "| `{modname}` | {total} | {documented} | {pct} | {} | {} |\n",
            total - documented,
            undoc_pub.len()
        ));
    }
    cov.push_str(&format!(
        "| **total** | **{}** | **{n_doc_total}** | **{:.0}%** | **{}** | **{}** |\n",
        all_fns.len(),
        100.0 * n_doc_total as f64 / all_fns.len().max(1) as f64,
        all_fns.len() - n_doc_total,
        n_pub - n_pub_doc
    ));
    cov.push_str(&format!(
        "\n**Public-surface coverage: {n_pub_doc}/{n_pub} ({:.0}%)** — the KPI worth tracking.\n",
        100.0 * n_pub_doc as f64 / n_pub.max(1) as f64
    ));
    cov.push_str("\n## Undocumented-public worklist\n\nTop 25 per module (full list in `modules/*.md` — blank `doc` column):\n\n");
    for (modname, (_, _, undoc_pub)) in &ranked {
        if undoc_pub.is_empty() {
            continue;
        }
        cov.push_str(&format!("### `{modname}` ({} undocumented public)\n\n", undoc_pub.len()));
        let mut sorted = undoc_pub.clone();
        sorted.sort_by_key(|fi| (records[fi.rec_idx].rel_path.clone(), fi.f.line));
        for fi in sorted.iter().take(25) {
            let rec = &records[fi.rec_idx];
            let scope = fi
                .f
                .scope
                .as_deref()
                .map(|s| format!("{s}::"))
                .unwrap_or_default();
            cov.push_str(&format!(
                "- `{scope}{}` — `{}:{}`\n",
                fi.f.name, rec.rel_path, fi.f.line
            ));
        }
        if undoc_pub.len() > 25 {
            cov.push_str(&format!("- _…{} more in `modules/{}.md`_\n", undoc_pub.len() - 25, slugify(modname)));
        }
        cov.push('\n');
    }
    let p = out_dir.join("DOCS-COVERAGE.md");
    std::fs::write(&p, cov)?;
    written.push(p);

    // ---- docrefs.ttl -------------------------------------------------------
    // External-documentation overlay: every function IRI → its generated
    // module doc. User-editable — repoint or extend entries to any IRI (spec
    // sections, wikis, ontology nodes) and merge into the codebase graph.
    let mut refs = String::from(
        "@prefix audit: <https://audit.dev/ns#> .\n\n\
         # GENERATED — safe to edit. External-documentation overlay.\n\
         # audit:documentedBy links a function IRI to an audit:Document.\n\
         # Repoint entries at any IRI (docs site, spec section, ontology node);\n\
         # merge this file into codebase.ttl to publish the references.\n\n",
    );
    for modname in by_module.keys() {
        let slug = slugify(modname);
        refs.push_str(&format!("audit:doc_modules_{slug} a audit:Document .\n"));
        refs.push_str(&format!(
            "audit:doc_modules_{slug} audit:docPath \"modules/{slug}.md\" .\n"
        ));
    }
    refs.push('\n');
    for (modname, recs) in &by_module {
        let doc_iri = format!("audit:doc_modules_{}", slugify(modname));
        for &i in recs {
            let rec = &records[i];
            for iri in fn_iris(rec) {
                refs.push_str(&format!("{iri} audit:documentedBy {doc_iri} .\n"));
            }
        }
    }
    let p = out_dir.join("docrefs.ttl");
    std::fs::write(&p, refs)?;
    written.push(p);

    // ---- INDEX.md ----------------------------------------------------------
    let mut index = format!(
        "# {} — audit docs\n\nGenerated {} · {} files · {} functions · {} modules\n\n{AGENT_PROTOCOL}\n\n",
        m.root_name,
        now,
        records.len(),
        all_fns.len(),
        by_module.len()
    );
    index.push_str("## Documents\n\n");
    index.push_str("- [`FUNCTIONS.md`](FUNCTIONS.md) — complete alphabetical index (grep this first)\n");
    index.push_str("- [`CAPABILITIES.md`](CAPABILITIES.md) — functional-purpose overview per module/domain\n");
    index.push_str("- [`DOCS-COVERAGE.md`](DOCS-COVERAGE.md) — doc coverage + undocumented-public worklist\n");
    index.push_str("- [`docrefs.ttl`](docrefs.ttl) — RDF overlay: every function → its module doc (external-doc refs)\n");
    index.push_str("- [`modules/`](modules/) — per-module registries:\n");
    for modname in by_module.keys() {
        index.push_str(&format!(
            "  - [`{modname}`](modules/{}.md)\n",
            audit_rdf::slugify(modname)
        ));
    }
    let p = out_dir.join("INDEX.md");
    std::fs::write(&p, index)?;
    written.push(p);

    Ok(written)
}
