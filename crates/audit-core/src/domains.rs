//! Domain classification: load `ontology/domains.ttl` (canonical line-subset
//! Turtle) and tag functions with `audit:hasDomain` by keyword match.

use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use audit_rdf::{parse_manifest_line, Term};

/// One domain entry: IRI subject + its keyword set.
#[derive(Debug, Clone)]
pub struct Domain {
    /// e.g. `audit:dom_physics`
    pub iri: String,
    /// Human label, e.g. `physics`.
    pub label: String,
    /// Lowercased keyword phrases; a hit on any tags the domain.
    pub keywords: Vec<String>,
}

/// Load domains from a canonical-subset Turtle file. Missing file → empty list.
pub fn load_domains(path: &Path) -> Result<Vec<Domain>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(path)?;
    let mut by_subject: HashMap<String, HashMap<String, Vec<Term>>> = HashMap::new();
    let mut order = Vec::new();
    for line in text.lines() {
        if let Some((s, p, o)) = parse_manifest_line(line)? {
            if !by_subject.contains_key(&s) {
                order.push(s.clone());
            }
            by_subject
                .entry(s)
                .or_default()
                .entry(p)
                .or_default()
                .push(o);
        }
    }

    let mut domains = Vec::new();
    for s in order {
        let props = &by_subject[&s];
        let is_domain = props
            .get("a")
            .map(|v| v.contains(&Term::Iri("audit:Domain".into())))
            .unwrap_or(false);
        if !is_domain {
            continue;
        }
        let label = props
            .get("audit:label")
            .and_then(|v| v.first())
            .and_then(|t| match t {
                Term::Literal(s) => Some(s.clone()),
                _ => None,
            })
            .unwrap_or_else(|| s.clone());
        let keywords = props
            .get("audit:keyword")
            .map(|v| {
                v.iter()
                    .filter_map(|t| match t {
                        Term::Literal(s) => Some(s.to_lowercase()),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        domains.push(Domain {
            iri: s,
            label,
            keywords,
        });
    }
    Ok(domains)
}

/// Return the domain IRIs whose keywords appear in `haystack` (lowercased by
/// caller conventions — we lowercase here anyway). Substring match for
/// multi-word keywords, word-boundary for single words to reduce noise like
/// "graph" matching "autograph".
pub fn classify(domains: &[Domain], haystack: &str) -> Vec<String> {
    let hay = format!(" {} ", haystack.to_lowercase().replace(|c: char| !c.is_alphanumeric(), " "));
    let mut out = Vec::new();
    for d in domains {
        let hit = d.keywords.iter().any(|k| {
            if k.contains(' ') {
                hay.contains(k.as_str())
            } else {
                hay.contains(&format!(" {k} "))
            }
        });
        if hit {
            out.push(d.iri.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_word_keywords_use_boundaries() {
        let d = vec![Domain {
            iri: "audit:dom_graph".into(),
            label: "graph".into(),
            keywords: vec!["graph".into(), "bfs".into()],
        }];
        assert_eq!(classify(&d, "fn graph_bfs()"), vec!["audit:dom_graph"]);
        assert!(classify(&d, "fn autograph()").is_empty());
    }

    #[test]
    fn multiword_keywords_substring() {
        let d = vec![Domain {
            iri: "audit:dom_physics".into(),
            label: "physics".into(),
            keywords: vec!["runge kutta".into()],
        }];
        assert_eq!(
            classify(&d, "fn runge_kutta_step()"),
            vec!["audit:dom_physics"]
        );
    }
}
