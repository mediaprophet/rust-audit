//! `audit` — scan a codebase into RDF/Turtle: inventory, scan, status.

use std::path::{Path, PathBuf};

mod qualia;

use anyhow::Result;
use audit_core::{
    build_inventory, load_domains, load_manifest, scan_files, write_manifest, Domain,
    FileStatus, ScanFilter,
};
use clap::{Parser, Subcommand};

/// Bundled default domains ontology (canonical line-subset Turtle).
const DEFAULT_DOMAINS: &str = include_str!("../../../ontology/domains.ttl");

#[derive(Parser)]
#[command(name = "audit", version, about = "Codebase scanner emitting RDF/Turtle")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Walk a codebase and (re)build the manifest of dirs/files with status.
    Inventory {
        /// Codebase root to scan.
        root: PathBuf,
        /// Output dir for manifest + graphs. Default: <root>/.audit
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Analyze files and emit Turtle graphs (resumable/segmented).
    Scan {
        root: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
        /// Rescan already-processed files too.
        #[arg(long)]
        all: bool,
        /// Restrict to a subdirectory (relative path).
        #[arg(long)]
        dir: Option<String>,
        /// Restrict to a language: rust, python, wgsl.
        #[arg(long)]
        lang: Option<String>,
        /// Process at most N files this run.
        #[arg(long)]
        max: Option<usize>,
        /// Domains ontology file (defaults to ontology/domains.ttl in CWD,
        /// else the bundled copy).
        #[arg(long)]
        domains: Option<PathBuf>,
    },
    /// Show per-file processing status.
    Status {
        root: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Resolve call sites to concrete functions across the whole scan
    /// (`audit:callsResolved` edges) -> .audit/resolved.ttl.
    Resolve {
        root: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Convert the emitted graphs (+ manifest) into a QualiaDB .q42 volume
    /// via `qualia-cli`.
    Q42 {
        root: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
        /// Path to the qualia-cli binary (else QUALIA_CLI / tools/ / PATH).
        #[arg(long)]
        qualia_cli: Option<PathBuf>,
        /// Output .q42 path (default: <out>/codebase.q42).
        #[arg(long)]
        q42_out: Option<PathBuf>,
        /// Skip the manifest — graphs only.
        #[arg(long)]
        no_manifest: bool,
        /// Skip the `q42 audit-source` pre-ingest report.
        #[arg(long)]
        no_audit_source: bool,
    },
    /// Compare two libraries: which functions does the reference have that
    /// the target lacks? Emits gaps.ttl, compare.ttl and gaps.n3 rules.
    Compare {
        /// Reference library root (e.g. the MIT lib being reimplemented).
        reference: PathBuf,
        /// Target library root (your implementation).
        target: PathBuf,
        /// Output dir (default: <target>/.audit/compare).
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Generate a self-contained HTML viewer (faceted search) for the scan.
    Site {
        root: PathBuf,
        /// Output dir (default: <root>/.audit). index.html lands in site/.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Domains ontology file (as for `scan`).
        #[arg(long)]
        domains: Option<PathBuf>,
    },
    /// Generate agent/human docs: INDEX.md, FUNCTIONS.md, CAPABILITIES.md,
    /// modules/*.md — authoritative function registry from actual source.
    Docs {
        root: PathBuf,
        /// Output dir (default: <root>/audit-docs).
        #[arg(long)]
        out: Option<PathBuf>,
        /// Domains ontology file (as for `scan`).
        #[arg(long)]
        domains: Option<PathBuf>,
    },
    /// Run a SPARQL query against a .q42 volume via `qualia-cli`.
    Query {
        /// .q42 file to query.
        vault: PathBuf,
        /// SPARQL query string (or use --file).
        query: Option<String>,
        /// Read the query from a file.
        #[arg(long, short = 'f')]
        file: Option<PathBuf>,
        #[arg(long)]
        qualia_cli: Option<PathBuf>,
    },
}

fn out_dir(root: &Path, out: &Option<PathBuf>) -> PathBuf {
    out.clone().unwrap_or_else(|| root.join(".audit"))
}

fn load_domains_or_default(flag: &Option<PathBuf>) -> Result<Vec<Domain>> {
    if let Some(p) = flag {
        return load_domains(p);
    }
    let cwd_default = Path::new("ontology/domains.ttl");
    if cwd_default.exists() {
        return load_domains(cwd_default);
    }
    // Parse the bundled copy through a temp file.
    let tmp = std::env::temp_dir().join("audit_default_domains.ttl");
    std::fs::write(&tmp, DEFAULT_DOMAINS)?;
    load_domains(&tmp)
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Inventory { root, out } => {
            let out = out_dir(&root, &out);
            let manifest_path = out.join("manifest.ttl");
            let prior = load_manifest(&manifest_path)?;
            let m = build_inventory(&root, &prior)?;
            write_manifest(&m, &manifest_path)?;

            let pending = m
                .files
                .values()
                .filter(|f| f.status == FileStatus::Pending)
                .count();
            let stale = m
                .files
                .values()
                .filter(|f| f.status == FileStatus::Stale)
                .count();
            println!(
                "inventoried {} dirs, {} files ({} pending, {} stale) -> {}",
                m.dirs.len(),
                m.files.len(),
                pending,
                stale,
                manifest_path.display()
            );
        }
        Cmd::Scan {
            root,
            out,
            all,
            dir,
            lang,
            max,
            domains,
        } => {
            let out = out_dir(&root, &out);
            let manifest_path = out.join("manifest.ttl");
            let mut m = load_manifest(&manifest_path)?;
            if m.files.is_empty() && !manifest_path.exists() {
                let fresh = build_inventory(&root, &m)?;
                write_manifest(&fresh, &manifest_path)?;
                m = fresh;
            }
            let domains = load_domains_or_default(&domains)?;
            let lib_name = m.root_name.clone();
            let report = scan_files(
                &root,
                &out,
                &mut m,
                &ScanFilter {
                    all,
                    dir,
                    lang,
                    max_files: max,
                },
                &domains,
                &lib_name,
            )?;
            write_manifest(&m, &manifest_path)?;
            println!(
                "scanned {} files: {} processed, {} failed, {} skipped",
                report.selected, report.processed, report.failed, report.skipped
            );
        }
        Cmd::Status { root, out } => {
            let out = out_dir(&root, &out);
            let m = load_manifest(&out.join("manifest.ttl"))?;
            if m.files.is_empty() {
                println!("no manifest found at {}", out.join("manifest.ttl").display());
                return Ok(());
            }
            let mut counts = std::collections::BTreeMap::new();
            for f in m.files.values() {
                *counts.entry(f.status).or_insert(0usize) += 1;
            }
            println!("{:<10} {:<60} {}", "status", "file", "processed at");
            for f in m.files.values() {
                println!(
                    "{:<10} {:<60} {}",
                    f.status.to_string(),
                    f.rel_path,
                    f.processed_at.as_deref().unwrap_or("-")
                );
            }
            println!();
            for (s, n) in counts {
                println!("{s}: {n}");
            }
        }
        Cmd::Resolve { root, out } => {
            let out = out_dir(&root, &out);
            let m = load_manifest(&out.join("manifest.ttl"))?;
            if m.files.is_empty() {
                anyhow::bail!("no manifest — run `audit inventory` + `audit scan` first");
            }
            let stats = audit_core::resolve::write_resolved(&root, &out, &m)?;
            println!(
                "resolved {}/{} call sites ({} edges) -> {}",
                stats.resolved_sites,
                stats.total_sites,
                stats.edges,
                out.join("resolved.ttl").display()
            );
        }
        Cmd::Q42 {
            root,
            out,
            qualia_cli,
            q42_out,
            no_manifest,
            no_audit_source,
        } => {
            let out = out_dir(&root, &out);
            let cli = qualia::find_cli(&qualia_cli)?;
            let q42_out = q42_out.unwrap_or_else(|| out.join("codebase.q42"));

            // Collect inputs: manifest + resolved edges + every emitted
            // graph, sorted for deterministic combined.ttl output.
            let mut inputs: Vec<PathBuf> = Vec::new();
            let manifest_path = out.join("manifest.ttl");
            if !no_manifest && manifest_path.exists() {
                inputs.push(manifest_path);
            }
            let resolved_path = out.join("resolved.ttl");
            if resolved_path.exists() {
                inputs.push(resolved_path);
            }
            let graphs_dir = out.join("graphs");
            if graphs_dir.exists() {
                let mut graphs: Vec<PathBuf> = std::fs::read_dir(&graphs_dir)?
                    .filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|p| p.extension().map(|e| e == "ttl").unwrap_or(false))
                    .collect();
                graphs.sort();
                inputs.extend(graphs);
            }
            if inputs.is_empty() {
                anyhow::bail!("no .ttl inputs under {} — run `audit scan` first", out.display());
            }

            let combined = out.join("codebase.ttl");
            let n = qualia::concat_ttl(&inputs, &combined)?;
            println!("merged {n} ttl files -> {}", combined.display());

            if !no_audit_source {
                qualia::audit_source(&cli, &combined)?;
            }
            qualia::import(&cli, &combined, &q42_out)?;
            println!("wrote {}", q42_out.display());
            println!(
                "query it:  audit query {} \"SELECT ?s WHERE {{ ?s a <https://audit.dev/ns#Function> . }}\"",
                q42_out.display()
            );
        }
        Cmd::Compare {
            reference,
            target,
            out,
        } => {
            let out = out.unwrap_or_else(|| target.join(".audit").join("compare"));
            std::fs::create_dir_all(&out)?;

            let ref_fns = audit_core::collect_functions(&reference)?;
            let tgt_fns = audit_core::collect_functions(&target)?;
            let report = audit_core::compare(ref_fns, tgt_fns);

            println!(
                "reference '{}' ({} fns) vs target '{}' ({} fns)\n",
                report.ref_lib.name,
                report.ref_lib.functions.len(),
                report.target_lib.name,
                report.target_lib.functions.len()
            );
            println!("covered (name+arity): {}", report.covered);
            println!("missing from target : {}", report.missing.len());
            for f in &report.missing {
                println!(
                    "  - {} ({}, arity {})  {}:{}",
                    f.name, f.lang, f.arity, f.rel_path, f.line
                );
            }
            println!("arity mismatches    : {}", report.arity_mismatches.len());
            for (r, t) in &report.arity_mismatches {
                println!(
                    "  ~ {} (ref arity {} vs target arity {})  {}:{}",
                    r.name, r.arity, t.arity, t.rel_path, t.line
                );
            }
            println!("extra in target     : {}", report.extra.len());

            std::fs::write(out.join("compare.ttl"), audit_core::compare_turtle(
                &report.ref_lib,
                &report.target_lib,
            ))?;
            std::fs::write(out.join("gaps.ttl"), audit_core::gaps_turtle(&report))?;
            std::fs::write(out.join("gaps.n3"), audit_core::gaps_n3())?;
            std::fs::write(
                out.join("gaps.sparql"),
                audit_core::gaps_sparql(&report.ref_lib.name, &report.target_lib.name),
            )?;
            println!(
                "\nwrote compare.ttl, gaps.ttl, gaps.n3, gaps.sparql -> {}",
                out.display()
            );
        }
        Cmd::Docs { root, out, domains } => {
            let out = out.unwrap_or_else(|| root.join("audit-docs"));
            let domains = load_domains_or_default(&domains)?;
            let written = audit_core::docs::generate_docs(&root, &out, &domains)?;
            println!("wrote {} docs -> {}", written.len(), out.display());
        }
        Cmd::Site { root, out, domains } => {
            let out = out_dir(&root, &out);
            let domains = load_domains_or_default(&domains)?;
            let index = audit_core::build_site(&root, &out, &domains)?;
            println!("wrote {}", index.display());
        }
        Cmd::Query {
            vault,
            query,
            file,
            qualia_cli,
        } => {
            let cli = qualia::find_cli(&qualia_cli)?;
            let query = match (query, file) {
                (Some(q), _) => q,
                (None, Some(f)) => std::fs::read_to_string(&f)?,
                (None, None) => anyhow::bail!("provide a query string or --file"),
            };
            qualia::sparql(&cli, &vault, &query)?;
        }
    }
    Ok(())
}
