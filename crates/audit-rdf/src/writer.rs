use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::Term;

/// Collects triples grouped by subject and serializes them as Turtle.
///
/// Triples sharing a subject are emitted with `;` continuation for readable
/// output. For the manifest's canonical line form, use [`crate::parse_manifest_line`]
/// — compatible because each emitted line is a complete statement only when
/// `one_line_per_triple` is set.
pub struct TurtleWriter {
    prefixes: BTreeMap<String, String>,
    /// subject -> [(predicate, object)] preserving insertion order per subject.
    triples: Vec<(String, Vec<(String, Term)>)>,
    index: std::collections::HashMap<String, usize>,
    /// When true, emit `s p o .` one triple per line (manifest canonical form).
    one_line_per_triple: bool,
}

impl TurtleWriter {
    pub fn new() -> Self {
        let mut prefixes = BTreeMap::new();
        prefixes.insert("audit".to_string(), crate::AUDIT_NS.to_string());
        prefixes.insert(
            "xsd".to_string(),
            "http://www.w3.org/2001/XMLSchema#".to_string(),
        );
        prefixes.insert(
            "rdf".to_string(),
            "http://www.w3.org/1999/02/22-rdf-syntax-ns#".to_string(),
        );
        prefixes.insert(
            "rdfs".to_string(),
            "http://www.w3.org/2000/01/rdf-schema#".to_string(),
        );
        Self {
            prefixes,
            triples: Vec::new(),
            index: std::collections::HashMap::new(),
            one_line_per_triple: false,
        }
    }

    /// Emit canonical `s p o .` lines instead of grouped `;` form.
    pub fn line_mode(mut self) -> Self {
        self.one_line_per_triple = true;
        self
    }

    pub fn add_prefix(&mut self, prefix: &str, iri: &str) {
        self.prefixes.insert(prefix.to_string(), iri.to_string());
    }

    pub fn triple(&mut self, subject: &str, predicate: &str, object: Term) {
        let idx = match self.index.get(subject) {
            Some(&i) => i,
            None => {
                let i = self.triples.len();
                self.index.insert(subject.to_string(), i);
                self.triples.push((subject.to_string(), Vec::new()));
                i
            }
        };
        self.triples[idx]
            .1
            .push((predicate.to_string(), object));
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        for (p, iri) in &self.prefixes {
            let _ = writeln!(out, "@prefix {p}: <{iri}> .");
        }
        out.push('\n');
        if self.one_line_per_triple {
            for (subject, preds) in &self.triples {
                for (pred, obj) in preds {
                    let _ = writeln!(out, "{subject} {pred} {obj} .");
                }
            }
        } else {
            for (subject, preds) in &self.triples {
                let _ = writeln!(out, "{subject}");
                for (i, (pred, obj)) in preds.iter().enumerate() {
                    let tail = if i + 1 == preds.len() { " ." } else { " ;" };
                    let _ = writeln!(out, "    {pred} {obj}{tail}");
                }
                out.push('\n');
            }
        }
        out
    }
}

impl Default for TurtleWriter {
    fn default() -> Self {
        Self::new()
    }
}
