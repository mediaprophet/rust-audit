//! Cross-library coverage comparison: extract function inventories from two
//! roots and report which reference functions are missing from the target.
//!
//! Matching basis is the (case-sensitive) function *name*, so ports across
//! languages still line up. Arity mismatches on name matches are reported as
//! warnings, not absences.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use audit_extract::FunctionKind;
use audit_rdf::{slugify, Term, TurtleWriter};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::scan::lib_iri;

/// One extracted function with provenance.
#[derive(Debug, Clone)]
pub struct FnRef {
    pub lang: String,
    pub name: String,
    pub arity: u32,
    pub kind: FunctionKind,
    pub scope: Option<String>,
    pub rel_path: String,
    pub line: u32,
}

/// All extractable functions under a root.
#[derive(Debug)]
pub struct LibFns {
    pub name: String,
    pub functions: Vec<FnRef>,
}

/// Walk a root and extract every function via the language analyzers.
/// Independent of manifest state — always reflects current sources.
pub fn collect_functions(root: &Path) -> Result<LibFns> {
    let (m, records) = crate::collect::collect_analyses(root)?;
    let mut functions = Vec::new();
    for rec in records {
        for f in rec.analysis.functions {
            functions.push(FnRef {
                lang: rec.lang.clone(),
                name: f.name,
                arity: f.arity,
                kind: f.kind,
                scope: f.scope,
                rel_path: rec.rel_path.clone(),
                line: f.line,
            });
        }
    }
    Ok(LibFns {
        name: m.root_name,
        functions,
    })
}

/// Result of comparing a reference library against a target.
#[derive(Debug)]
pub struct GapReport {
    pub ref_lib: LibFns,
    pub target_lib: LibFns,
    /// Reference functions with no same-named function in the target.
    pub missing: Vec<FnRef>,
    /// Name matched, but arity differs between ref and target.
    pub arity_mismatches: Vec<(FnRef, FnRef)>,
    /// Target functions with no same-named function in the reference.
    pub extra: Vec<FnRef>,
    /// Reference functions fully covered (name + arity match somewhere).
    pub covered: usize,
}

pub fn compare(ref_lib: LibFns, target_lib: LibFns) -> GapReport {
    // name (lowercased) -> target fns
    let mut by_name: BTreeMap<String, Vec<&FnRef>> = BTreeMap::new();
    for f in &target_lib.functions {
        by_name.entry(f.name.to_lowercase()).or_default().push(f);
    }
    let mut ref_names: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    for f in &ref_lib.functions {
        ref_names.insert(f.name.to_lowercase());
    }

    let mut missing = Vec::new();
    let mut arity_mismatches = Vec::new();
    let mut covered = 0usize;
    let mut seen_missing: std::collections::HashSet<(String, u32)> =
        std::collections::HashSet::new();

    for rf in &ref_lib.functions {
        match by_name.get(&rf.name.to_lowercase()) {
            None => {
                if seen_missing.insert((rf.name.to_lowercase(), rf.arity)) {
                    missing.push(rf.clone());
                }
            }
            Some(cands) => {
                if cands.iter().any(|t| t.arity == rf.arity) {
                    covered += 1;
                } else if let Some(t) = cands.first() {
                    arity_mismatches.push((rf.clone(), (*t).clone()));
                }
            }
        }
    }

    let extra = target_lib
        .functions
        .iter()
        .filter(|f| !ref_names.contains(&f.name.to_lowercase()))
        .cloned()
        .collect();

    GapReport {
        ref_lib,
        target_lib,
        missing,
        arity_mismatches,
        extra,
        covered,
    }
}

/// Name-only canonical IRI (language-agnostic — for ports).
fn canonx_iri(name: &str) -> String {
    format!("audit:canonx_{}", slugify(&name.to_lowercase()))
}

/// Self-contained comparison graph: libraries, canonical functions, and
/// `audit:exports` links — the substrate for SPARQL/N3 reasoning in a store.
pub fn compare_turtle(ref_lib: &LibFns, target_lib: &LibFns) -> String {
    let mut w = TurtleWriter::new();
    for lib in [ref_lib, target_lib] {
        let li = lib_iri(&lib.name);
        w.triple(&li, "a", Term::iri("audit:Library"));
        w.triple(&li, "audit:label", Term::lit(&lib.name));
        // distinct canonical exports
        let mut seen = std::collections::HashSet::new();
        for f in &lib.functions {
            if seen.insert(f.name.to_lowercase()) {
                w.triple(&li, "audit:exports", Term::iri(canonx_iri(&f.name)));
            }
        }
    }
    // canonical fn nodes (union of both libs)
    let mut seen = std::collections::HashSet::new();
    for f in ref_lib
        .functions
        .iter()
        .chain(target_lib.functions.iter())
    {
        if !seen.insert(f.name.to_lowercase()) {
            continue;
        }
        let ci = canonx_iri(&f.name);
        w.triple(&ci, "a", Term::iri("audit:CanonicalFunction"));
        w.triple(&ci, "audit:name", Term::lit(&f.name));
        w.triple(&ci, "audit:arity", Term::int(f.arity as i64));
    }
    w.render()
}

/// Gap report as Turtle.
pub fn gaps_turtle(report: &GapReport) -> String {
    let mut w = TurtleWriter::new();
    let now = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".into());

    w.triple("audit:gapReport", "a", Term::iri("audit:GapReport"));
    w.triple(
        "audit:gapReport",
        "audit:refLibrary",
        Term::iri(lib_iri(&report.ref_lib.name)),
    );
    w.triple(
        "audit:gapReport",
        "audit:targetLibrary",
        Term::iri(lib_iri(&report.target_lib.name)),
    );
    w.triple(
        "audit:gapReport",
        "audit:generatedAt",
        Term::typed(now, "xsd:dateTime"),
    );
    w.triple(
        "audit:gapReport",
        "audit:matchBasis",
        Term::lit("name"),
    );

    for (i, f) in report.missing.iter().enumerate() {
        let g = format!("audit:gap_{i}");
        w.triple(&g, "a", Term::iri("audit:Gap"));
        w.triple(&g, "audit:function", Term::iri(canonx_iri(&f.name)));
        w.triple(
            &g,
            "audit:missingFrom",
            Term::iri(lib_iri(&report.target_lib.name)),
        );
        w.triple(&g, "audit:declaredIn", Term::lit(&f.rel_path));
        w.triple(&g, "audit:line", Term::int(f.line as i64));
        w.triple(&g, "audit:arity", Term::int(f.arity as i64));
    }
    for (i, f) in report.extra.iter().enumerate() {
        let g = format!("audit:extra_{i}");
        w.triple(&g, "a", Term::iri("audit:Extra"));
        w.triple(&g, "audit:function", Term::iri(canonx_iri(&f.name)));
        w.triple(
            &g,
            "audit:extraIn",
            Term::iri(lib_iri(&report.target_lib.name)),
        );
        w.triple(&g, "audit:declaredIn", Term::lit(&f.rel_path));
    }
    w.render()
}

/// SPARQL MINUS query computing the gaps directly on the compare.q42 store.
/// Verified working on QualiaDB v0.0.40.15 (range executor falls back to the
/// compatibility executor for MINUS). Do NOT rewrite as FILTER NOT EXISTS —
/// that returns wrong results silently (see docs/QUALIADB-NOTES.md).
pub fn gaps_sparql(ref_name: &str, target_name: &str) -> String {
    let ns = audit_rdf::AUDIT_NS;
    format!(
        "# Canonical functions exported by the reference but not the target.\n\
         SELECT ?c WHERE {{\n\
         \x20 <{ns}lib_{ref}> <{ns}exports> ?c .\n\
         \x20 MINUS {{ <{ns}lib_{tgt}> <{ns}exports> ?c . }}\n\
         }}\n",
        ns = ns,
        ref = slugify(ref_name),
        tgt = slugify(target_name),
    )
}

/// N3 rules expressing the same coverage logic — best effort, UNVERIFIED
/// against QualiaDB's N3 engine (see docs/QUALIADB-NOTES.md).
pub fn gaps_n3() -> &'static str {
    r#"# Coverage-gap rules for the compare.ttl graph.
# STATUS: unverified — QualiaDB N3 rule support is untested (see
# docs/QUALIADB-NOTES.md). CWM-style negation via log:notIncludes.

@prefix audit: <https://audit.dev/ns#> .
@prefix log:   <http://www.w3.org/2000/10/swap/log#> .

# NOTE: `log:notIncludes` operates on formulas (graph terms). `audit:graph`
# below is a placeholder for the enclosing dataset — bind it to whatever term
# QualiaDB uses for the current graph once its N3 dialect is confirmed.

# A canonical function exported by the reference library but not exported
# by the target library is a gap.
{
    ?ref a audit:Library .
    ?ref audit:exports ?c .
    ?tgt a audit:Library .
    audit:graph log:notIncludes { ?tgt audit:exports ?c } .
}
=>
{
    ?c audit:missingFrom ?tgt .
} .

# The inverse: anything the target exports that the reference does not.
{
    ?tgt a audit:Library .
    ?tgt audit:exports ?c .
    ?ref a audit:Library .
    audit:graph log:notIncludes { ?ref audit:exports ?c } .
}
=>
{
    ?c audit:extraIn ?tgt .
} .
"#
}
