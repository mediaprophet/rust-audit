//! Read/write `manifest.ttl` — the canonical line-subset Turtle document that
//! is the authoritative state for incremental scanning.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use audit_rdf::{parse_manifest_line, Term, TurtleWriter};

use crate::model::{DirEntry, FileEntry, FileStatus, Manifest};

/// Serialize the manifest to canonical line-mode Turtle.
pub fn manifest_to_turtle(m: &Manifest) -> String {
    let mut w = TurtleWriter::new().line_mode();
    w.triple("audit:manifest", "a", Term::iri("audit:ScanManifest"));
    w.triple("audit:manifest", "audit:rootName", Term::lit(&m.root_name));

    for dir in m.dirs.values() {
        w.triple(&dir.iri, "a", Term::iri("audit:Directory"));
        w.triple(&dir.iri, "audit:relativePath", Term::lit(&dir.rel_path));
        if let Some(parent) = parent_iri(m, &dir.rel_path) {
            w.triple(&dir.iri, "audit:inDirectory", Term::iri(parent));
        }
    }

    for f in m.files.values() {
        let class = if f.language.is_some() {
            "audit:SourceFile"
        } else {
            "audit:File"
        };
        w.triple(&f.iri, "a", Term::iri(class));
        w.triple(&f.iri, "audit:relativePath", Term::lit(&f.rel_path));
        if let Some(parent) = parent_iri(m, &f.rel_path) {
            w.triple(&f.iri, "audit:inDirectory", Term::iri(parent));
        }
        if let Some(lang) = &f.language {
            w.triple(&f.iri, "audit:language", Term::lit(lang));
        }
        w.triple(&f.iri, "audit:contentHash", Term::lit(&f.content_hash));
        w.triple(&f.iri, "audit:modifiedAt", Term::typed(&f.modified_at, "xsd:dateTime"));
        w.triple(&f.iri, "audit:status", Term::iri(f.status.as_iri()));
        if let Some(t) = &f.processed_at {
            w.triple(&f.iri, "audit:processedAt", Term::typed(t, "xsd:dateTime"));
        }
        if let Some(g) = &f.graph_file {
            w.triple(&f.iri, "audit:graphFile", Term::lit(g));
        }
        if let Some(e) = &f.error {
            w.triple(&f.iri, "audit:error", Term::lit(e));
        }
    }
    w.render()
}

/// Look up the IRI of a node's parent directory via the manifest's dir table.
fn parent_iri(m: &Manifest, rel_path: &str) -> Option<String> {
    if rel_path.is_empty() {
        return None;
    }
    let parent = match rel_path.rfind('/') {
        Some(i) => &rel_path[..i],
        None => "",
    };
    m.dirs.get(parent).map(|d| d.iri.clone())
}

/// Write the manifest to a path.
pub fn write_manifest(m: &Manifest, path: &Path) -> Result<()> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)
            .with_context(|| format!("creating {}", p.display()))?;
    }
    std::fs::write(path, manifest_to_turtle(m))
        .with_context(|| format!("writing {}", path.display()))
}

/// Load a manifest previously written by [`write_manifest`]. Missing file →
/// empty manifest.
pub fn load_manifest(path: &Path) -> Result<Manifest> {
    if !path.exists() {
        return Ok(Manifest::default());
    }
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;

    // subject -> collected predicates
    let mut nodes: HashMap<String, HashMap<String, Vec<Term>>> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for line in text.lines() {
        if let Some((s, p, o)) = parse_manifest_line(line)? {
            nodes.entry(s.clone()).or_default().entry(p).or_default().push(o);
            if !order.contains(&s) {
                order.push(s);
            }
        }
    }

    let mut m = Manifest::default();
    for subject in order {
        let props = &nodes[&subject];
        let types = props.get("a").cloned().unwrap_or_default();
        if subject == "audit:manifest" {
            if let Some(Term::Literal(v)) = first(props.get("audit:rootName")) {
                m.root_name = v.clone();
            }
            continue;
        }
        let rel = match first(props.get("audit:relativePath")) {
            Some(Term::Literal(v)) => v.clone(),
            _ => continue,
        };
        if types.contains(&Term::Iri("audit:Directory".into())) {
            m.dirs.insert(
                rel.clone(),
                DirEntry {
                    iri: subject.clone(),
                    rel_path: rel,
                },
            );
        } else if types.contains(&Term::Iri("audit:SourceFile".into()))
            || types.contains(&Term::Iri("audit:File".into()))
        {
            let entry = FileEntry {
                iri: subject.clone(),
                rel_path: rel.clone(),
                language: lit(props.get("audit:language")),
                content_hash: lit(props.get("audit:contentHash")).unwrap_or_default(),
                modified_at: lit(props.get("audit:modifiedAt")).unwrap_or_default(),
                status: iri(props.get("audit:status"))
                    .and_then(|s| FileStatus::from_iri(&s))
                    .unwrap_or(FileStatus::Pending),
                processed_at: lit(props.get("audit:processedAt")),
                graph_file: lit(props.get("audit:graphFile")),
                error: lit(props.get("audit:error")),
            };
            m.files.insert(rel, entry);
        }
    }
    Ok(m)
}

fn first(v: Option<&Vec<Term>>) -> Option<&Term> {
    v.and_then(|v| v.first())
}

fn lit(v: Option<&Vec<Term>>) -> Option<String> {
    match first(v) {
        Some(Term::Literal(s)) => Some(s.clone()),
        Some(Term::TypedLiteral { value, .. }) => Some(value.clone()),
        _ => None,
    }
}

fn iri(v: Option<&Vec<Term>>) -> Option<String> {
    match first(v) {
        Some(Term::Iri(s)) => Some(s.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{mint_iri, DirEntry, FileEntry, FileStatus};
    use std::collections::HashSet;

    fn sample_manifest() -> Manifest {
        let mut used = HashSet::new();
        let mut m = Manifest {
            root_name: "proj".into(),
            ..Default::default()
        };
        m.dirs.insert(
            String::new(),
            DirEntry {
                iri: mint_iri("d_", "", &mut used),
                rel_path: String::new(),
            },
        );
        m.dirs.insert(
            "src".into(),
            DirEntry {
                iri: mint_iri("d_", "src", &mut used),
                rel_path: "src".into(),
            },
        );
        m.files.insert(
            "src/main.rs".into(),
            FileEntry {
                iri: mint_iri("f_", "src/main.rs", &mut used),
                rel_path: "src/main.rs".into(),
                language: Some("rust".into()),
                content_hash: "sha256:abc".into(),
                modified_at: "2026-10-17T01:00:00Z".into(),
                status: FileStatus::Processed,
                processed_at: Some("2026-10-17T02:00:00Z".into()),
                graph_file: Some("graphs/src_main_rs.ttl".into()),
                error: None,
            },
        );
        m
    }

    #[test]
    fn manifest_round_trips() {
        let m = sample_manifest();
        let ttl = manifest_to_turtle(&m);

        // write to temp and reload through the real path
        let dir = std::env::temp_dir().join(format!("audit_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("manifest.ttl");
        std::fs::write(&path, &ttl).unwrap();
        let loaded = load_manifest(&path).unwrap();
        std::fs::remove_dir_all(&dir).ok();

        assert_eq!(loaded.root_name, "proj");
        assert_eq!(loaded.dirs.len(), 2);
        let f = loaded.files.get("src/main.rs").unwrap();
        assert_eq!(f.status, FileStatus::Processed);
        assert_eq!(f.language.as_deref(), Some("rust"));
        assert_eq!(f.content_hash, "sha256:abc");
        assert_eq!(f.graph_file.as_deref(), Some("graphs/src_main_rs.ttl"));
    }

    #[test]
    fn literals_with_special_chars_round_trip() {
        let mut m = Manifest::default();
        m.root_name = "r".into();
        m.files.insert(
            "weird \"name\".rs".into(),
            FileEntry {
                iri: "audit:f_weird_name_rs".into(),
                rel_path: "weird \"name\".rs".into(),
                language: Some("rust".into()),
                content_hash: "sha256:x".into(),
                modified_at: "2026-10-17T00:00:00Z".into(),
                status: FileStatus::Pending,
                processed_at: None,
                graph_file: None,
                error: Some("line1\nline2 \"q\"".into()),
            },
        );
        let ttl = manifest_to_turtle(&m);
        let dir = std::env::temp_dir().join(format!("audit_test2_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("m.ttl");
        std::fs::write(&path, &ttl).unwrap();
        let loaded = load_manifest(&path).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        let f = loaded.files.get("weird \"name\".rs").unwrap();
        assert_eq!(f.error.as_deref(), Some("line1\nline2 \"q\""));
    }
}
