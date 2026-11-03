//! Cross-file call resolution: mint function IRIs identically to `scan`, then
//! resolve call sites to concrete function IRIs where the evidence is
//! sufficient. Emits `audit:callsResolved` edges — a stricter layer on top of
//! the name-level `audit:calls` graph.
//!
//! Resolution rules (all name-based, conservative):
//! - `self.foo()` / `Self::foo()` / `cls.foo()` → methods in the caller's own
//!   impl/class/trait scope.
//! - `Type::foo()` → methods in `impl Type` / `impl _ for Type` / `class Type`
//!   scopes anywhere in the scan (generics and module prefixes stripped).
//! - `foo()` → same-file free functions; if none, a codebase-unique free
//!   function of that name.
//!
//! Where multiple candidates remain (cfg-gated twins, same-type impls in
//! several files), ALL candidates get edges — the edge asserts "may resolve
//! to", not "is". Unresolvable receivers (`x.foo()`) get no resolved edge.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{Context, Result};
use audit_extract::{analyzer_for, CallKind};
use audit_rdf::{Term, TurtleWriter};

use crate::model::Manifest;
use crate::scan::mint_fn_iris;

/// Base type name of a scope label:
/// `impl Foo<T>` / `impl Tr for Foo` → `Foo`, `class Bar` → `Bar`,
/// `trait Tr` → `None` (traits aren't call targets).
pub fn impl_target(scope: &str) -> Option<String> {
    let s = scope
        .strip_prefix("impl ")
        .or_else(|| scope.strip_prefix("class "))?;
    // `impl Tr for X` → X; `impl X` → X. Strip generics then take last segment.
    let s = s.rsplit(" for ").next().unwrap_or(s).trim();
    let s = s.split('<').next().unwrap_or(s).trim();
    s.rsplit("::").next().map(|t| t.to_string())
}

/// The enclosing type name a `self`/`Self` qualifier refers to: same as
/// `impl_target` but `trait Tr` scopes also count (default-method calls).
fn self_target(scope: &str) -> Option<String> {
    if let Some(s) = scope.strip_prefix("trait ") {
        return Some(s.split('<').next().unwrap_or(s).trim().to_string());
    }
    impl_target(scope)
}

/// Strip generics and module path from a qualifier, leaving the last segment:
/// `Vec<T>` → `Vec`, `crate::m::Type` → `Type`.
pub fn head_type(qualifier: &str) -> &str {
    let q = qualifier.split('(').next().unwrap_or(qualifier).trim();
    let q = q.split('<').next().unwrap_or(q);
    let last = q.rsplit("::").next().unwrap_or(q);
    last.rsplit('.').next().unwrap_or(last)
}

/// One extracted call site (owned copy for the single-pass resolve).
struct SiteCall {
    kind: CallKind,
    name: String,
    qualifier: Option<String>,
}

struct FnRec {
    iri: String,
    name: String,
    /// Slug of the containing file (for same-file free-fn preference).
    file_slug: String,
    /// Normalized enclosing type (`self_target(scope)`), if a method.
    self_ty: Option<String>,
    /// Normalized impl-target (`impl_target(scope)`) — call-target type.
    impl_ty: Option<String>,
    /// `impl`/`class`/`trait` scope label, verbatim (None = free function).
    scope: Option<String>,
    /// Call sites in this function's body.
    calls: Vec<SiteCall>,
}

/// Resolve one call site to concrete fn IRIs.
fn resolve_call<'a>(
    c: &SiteCall,
    caller_self_ty: Option<&str>,
    type_methods: &HashMap<(String, String), Vec<&'a str>>,
    self_methods: &HashMap<(String, String), Vec<&'a str>>,
    free_fns: &HashMap<String, Vec<&'a str>>,
    file_free: &HashMap<String, Vec<&'a str>>,
) -> Vec<&'a str> {
    let is_self = |q: &str| matches!(q, "self" | "Self" | "cls");

    match c.kind {
        CallKind::Method => {
            if let Some(q) = c.qualifier.as_deref() {
                if is_self(q) {
                    if let Some(ty) = caller_self_ty {
                        return self_methods
                            .get(&(ty.to_string(), c.name.clone()))
                            .cloned()
                            .unwrap_or_default();
                    }
                }
                // `x.foo()` — receiver type unknown → unresolvable.
            }
            Vec::new()
        }
        CallKind::Scoped => {
            let q = c.qualifier.as_deref().unwrap_or("");
            if is_self(q) {
                if let Some(ty) = caller_self_ty {
                    return self_methods
                        .get(&(ty.to_string(), c.name.clone()))
                        .cloned()
                        .unwrap_or_default();
                }
                return Vec::new();
            }
            let t = head_type(q);
            if t.is_empty() {
                return Vec::new();
            }
            type_methods
                .get(&(t.to_string(), c.name.clone()))
                .cloned()
                .unwrap_or_default()
        }
        CallKind::Function => {
            // Prefer same-file free functions; else a codebase-unique one.
            if let Some(local) = file_free.get(c.name.as_str()) {
                if !local.is_empty() {
                    return local.clone();
                }
            }
            match free_fns.get(c.name.as_str()) {
                Some(v) if v.len() == 1 => v.clone(),
                _ => Vec::new(),
            }
        }
        CallKind::Macro => Vec::new(),
    }
}

pub struct ResolveStats {
    pub edges: usize,
    pub resolved_sites: usize,
    pub total_sites: usize,
}

/// Build `resolved.ttl`: `audit:callsResolved` edges for the whole scan root.
/// Single analysis pass — every function's calls are kept with its record,
/// then resolved against global indexes.
pub fn resolve_graph(root: &Path, m: &Manifest) -> Result<(String, ResolveStats)> {
    // Collect every function + its call sites (single pass over sources).
    let mut fns: Vec<FnRec> = Vec::new();

    for entry in m.files.values() {
        let Some(lang) = &entry.language else { continue };
        let Some(analyzer) = analyzer_for(lang) else { continue };
        let abs = root.join(entry.rel_path.replace('/', std::path::MAIN_SEPARATOR_STR));
        let Ok(src) = crate::read_source::read_source(&abs) else { continue };
        let Ok(a) = analyzer.analyze(&src) else { continue };

        let file_slug = entry.iri.trim_start_matches("audit:").to_string();
        let iris = mint_fn_iris(&file_slug, &a.functions);

        for (f, iri) in a.functions.iter().zip(&iris) {
            fns.push(FnRec {
                iri: iri.clone(),
                name: f.name.clone(),
                file_slug: file_slug.clone(),
                self_ty: f.scope.as_deref().and_then(self_target),
                impl_ty: f.scope.as_deref().and_then(impl_target),
                scope: f.scope.clone(),
                calls: f
                    .body
                    .calls
                    .iter()
                    .map(|c| SiteCall {
                        kind: c.kind,
                        name: c.name.clone(),
                        qualifier: c.qualifier.clone(),
                    })
                    .collect(),
            });
        }
    }

    // Global indexes: (type, method) → impl-scope and self-scope candidates;
    // name → free functions (global + per-file).
    let mut type_methods: HashMap<(String, String), Vec<&str>> = HashMap::new();
    let mut self_methods: HashMap<(String, String), Vec<&str>> = HashMap::new();
    let mut free_fns: HashMap<String, Vec<&str>> = HashMap::new();
    let mut file_free: HashMap<&str, HashMap<String, Vec<&str>>> = HashMap::new();
    for r in &fns {
        if let Some(ty) = &r.impl_ty {
            type_methods
                .entry((ty.clone(), r.name.clone()))
                .or_default()
                .push(&r.iri);
        }
        if let Some(ty) = &r.self_ty {
            self_methods
                .entry((ty.clone(), r.name.clone()))
                .or_default()
                .push(&r.iri);
        }
        if r.scope.is_none() {
            free_fns
                .entry(r.name.clone())
                .or_default()
                .push(&r.iri);
            file_free
                .entry(r.file_slug.as_str())
                .or_default()
                .entry(r.name.clone())
                .or_default()
                .push(&r.iri);
        }
    }

    // Resolve each call site and emit deduped edges per caller.
    let mut w = TurtleWriter::new();
    let mut edge_count = 0usize;
    let mut resolved_sites = 0usize;
    let mut total_sites = 0usize;
    let empty_local: HashMap<String, Vec<&str>> = HashMap::new();

    for r in &fns {
        let mut emitted: HashSet<&str> = HashSet::new();
        let local_free = file_free
            .get(r.file_slug.as_str())
            .unwrap_or(&empty_local);
        for c in &r.calls {
            total_sites += 1;
            let targets = resolve_call(
                c,
                r.self_ty.as_deref(),
                &type_methods,
                &self_methods,
                &free_fns,
                &local_free,
            );
            if !targets.is_empty() {
                resolved_sites += 1;
            }
            for t in targets {
                if emitted.insert(t) {
                    w.triple(&r.iri, "audit:callsResolved", Term::iri(t));
                    edge_count += 1;
                }
            }
        }
    }

    Ok((
        w.render(),
        ResolveStats {
            edges: edge_count,
            resolved_sites,
            total_sites,
        },
    ))
}

/// Write `resolved.ttl` under `out_dir`; returns the resolution stats.
pub fn write_resolved(root: &Path, out_dir: &Path, m: &Manifest) -> Result<ResolveStats> {
    let (ttl, stats) = resolve_graph(root, m)?;
    let path = out_dir.join("resolved.ttl");
    std::fs::write(&path, &ttl).with_context(|| format!("writing {}", path.display()))?;
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn impl_targets() {
        assert_eq!(impl_target("impl Foo").as_deref(), Some("Foo"));
        assert_eq!(impl_target("impl Foo<T>").as_deref(), Some("Foo"));
        assert_eq!(impl_target("impl Tr for Foo").as_deref(), Some("Foo"));
        assert_eq!(impl_target("impl Tr for Foo<T>").as_deref(), Some("Foo"));
        assert_eq!(impl_target("impl crate::m::Foo").as_deref(), Some("Foo"));
        assert_eq!(impl_target("class Bar").as_deref(), Some("Bar"));
        assert_eq!(impl_target("trait Tr"), None);
    }

    #[test]
    fn self_targets() {
        assert_eq!(self_target("trait Tr").as_deref(), Some("Tr"));
        assert_eq!(self_target("impl Foo").as_deref(), Some("Foo"));
    }

    #[test]
    fn head_types() {
        assert_eq!(head_type("Vec"), "Vec");
        assert_eq!(head_type("Vec<T>"), "Vec");
        assert_eq!(head_type("crate::m::Type"), "Type");
        assert_eq!(head_type("std::collections::HashMap<K, V>"), "HashMap");
    }
}
