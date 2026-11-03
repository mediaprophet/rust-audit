use anyhow::Result;
use tree_sitter::{Language, Node};

use crate::common::{body, extract, LangSpec, ScopeSpec};
use crate::{Analyzer, BodyFacts, CallKind, FileAnalysis};

pub struct PythonAnalyzer;

fn class_label(node: &Node, src: &[u8]) -> String {
    let name = node
        .child_by_field_name("name")
        .and_then(|n| n.utf8_text(src).ok())
        .unwrap_or("?");
    format!("class {name}")
}

/// Pull the docstring: first statement of the body block when it is a bare
/// string expression.
fn docstring(fnode: &Node, src: &[u8]) -> Option<String> {
    let body = fnode.child_by_field_name("body")?;
    let first = body.named_child(0)?;
    if first.kind() != "expression_statement" {
        return None;
    }
    let s = first.named_child(0)?;
    if !matches!(s.kind(), "string" | "concatenated_string") {
        return None;
    }
    let text = s.utf8_text(src).ok()?;
    Some(strip_py_quotes(text))
}

fn strip_py_quotes(s: &str) -> String {
    let t = s.trim();
    // drop string prefixes like r", b", u"
    let t = t
        .strip_prefix(|c: char| c == 'r' || c == 'b' || c == 'u' || c == 'f')
        .unwrap_or(t);
    let t = t
        .strip_prefix(|c: char| c == 'r' || c == 'b' || c == 'u' || c == 'f')
        .unwrap_or(t);
    for q in ["\"\"\"", "'''", "\"", "'"] {
        if t.starts_with(q) && t.ends_with(q) && t.len() >= 2 * q.len() {
            return t[q.len()..t.len() - q.len()].trim().to_string();
        }
    }
    t.trim_matches(|c| c == '"' || c == '\'').trim().to_string()
}

static SCOPES: &[ScopeSpec] = &[ScopeSpec {
    kind: "class_definition",
    label: class_label,
}];

/// Name for a parameter node (`identifier`, `typed_parameter`,
/// `default_parameter`, `typed_default_parameter`, splat params).
fn param_label(p: &Node, src: &[u8]) -> Option<String> {
    match p.kind() {
        "typed_parameter" | "typed_default_parameter" => {
            let name = p
                .named_child(0)
                .and_then(|n| n.utf8_text(src).ok())
                .unwrap_or("_");
            let ty = body::field_text(p, "type", src);
            Some(match ty {
                Some(t) => format!("{}: {}", name, body::squash(t)),
                None => name.to_string(),
            })
        }
        "default_parameter" => body::field_text(p, "name", src).map(|n| n.to_string()),
        "identifier" | "list_splat_pattern" | "dictionary_splat_pattern" => {
            Some(body::squash(p.utf8_text(src).ok()?))
        }
        _ => None,
    }
}

/// Extract calls, param/return types, and `self` attribute reads/writes.
fn body_of(fnode: &Node, src: &[u8]) -> BodyFacts {
    let mut out = BodyFacts::default();

    if let Some(params) = fnode.child_by_field_name("parameters") {
        let mut c = params.walk();
        for p in params.named_children(&mut c) {
            if let Some(label) = param_label(&p, src) {
                out.param_types.push(label);
            }
        }
    }
    out.return_type = body::field_text(fnode, "return_type", src).map(body::squash);

    if let Some(body_node) = fnode.child_by_field_name("body") {
        body::descend(&body_node, &mut |n| {
            match n.kind() {
                // Nested defs/lambdas get their own facts — don't descend.
                "function_definition" | "lambda" | "class_definition" => return false,
                "call" => {
                    if let Some(f) = n.child_by_field_name("function") {
                        let (name, qual, kind) = match f.kind() {
                            "identifier" => {
                                (f.utf8_text(src).unwrap_or(""), None, CallKind::Function)
                            }
                            "attribute" => (
                                body::field_text(&f, "attribute", src).unwrap_or(""),
                                body::field_text(&f, "object", src).map(body::squash),
                                CallKind::Method,
                            ),
                            _ => ("", None, CallKind::Function),
                        };
                        if !name.is_empty() {
                            out.calls.push(body::call(
                                name,
                                qual,
                                kind,
                                body::field_arity(n, "arguments"),
                                n,
                            ));
                        }
                    }
                }
                _ => {}
            }
            true
        });

        // self.attr writes (assignment LHS incl. augmented/for/with targets).
        let mut writes = Vec::new();
        body::descend(&body_node, &mut |n| {
            match n.kind() {
                "function_definition" | "lambda" | "class_definition" => return false,
                "assignment" | "augmented_assignment" | "for_statement" => {
                    if let Some(lhs) = n.child_by_field_name("left") {
                        let mut visit = |l: &Node| -> bool {
                            if l.kind() == "attribute"
                                && body::field_text(l, "object", src) == Some("self")
                            {
                                if let Some(m) = body::field_text(l, "attribute", src) {
                                    writes.push(m.to_string());
                                }
                            }
                            true
                        };
                        if visit(&lhs) {
                            body::descend(&lhs, &mut visit);
                        }
                    }
                }
                _ => {}
            }
            true
        });
        let mut reads = Vec::new();
        body::descend(&body_node, &mut |n| {
            match n.kind() {
                "function_definition" | "lambda" | "class_definition" => return false,
                "attribute"
                    if body::field_text(n, "object", src) == Some("self") =>
                {
                    if let Some(m) = body::field_text(n, "attribute", src) {
                        reads.push(m.to_string());
                    }
                }
                _ => {}
            }
            true
        });
        for w in writes {
            if !out.writes_fields.contains(&w) {
                out.writes_fields.push(w);
            }
        }
        for r in reads {
            if !out.reads_fields.contains(&r) {
                out.reads_fields.push(r);
            }
        }
        if !out.writes_fields.is_empty() {
            out.mutates_receiver = true;
        }
    }
    out
}

impl Analyzer for PythonAnalyzer {
    fn analyze(&self, source: &str) -> Result<FileAnalysis> {
        let spec = LangSpec {
            language: Language::from(tree_sitter_python::LANGUAGE),
            comment_kinds: &["comment"],
            function_kinds: &["function_definition"],
            method_scopes: SCOPES,
            doc_prefixes: &[],
            doc_extractor: Some(docstring),
            visibility: |_node, _src, name| {
                // `_private` convention; dunders (`__x__`) stay public.
                !(name.starts_with('_')
                    && !(name.starts_with("__") && name.ends_with("__") && name.len() > 4))
            },
            body_extractor: Some(body_of),
        };
        extract(&spec, source)
    }
}
