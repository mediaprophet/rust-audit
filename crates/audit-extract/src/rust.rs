use anyhow::Result;
use tree_sitter::{Language, Node};

use crate::common::{body, extract, LangSpec, ScopeSpec};
use crate::{Analyzer, BodyFacts, CallKind, FileAnalysis};

pub struct RustAnalyzer;

fn impl_label(node: &Node, src: &[u8]) -> String {
    let ty = node
        .child_by_field_name("type")
        .and_then(|n| n.utf8_text(src).ok())
        .unwrap_or("?");
    match node
        .child_by_field_name("trait")
        .and_then(|n| n.utf8_text(src).ok())
    {
        Some(tr) => format!("impl {tr} for {ty}"),
        None => format!("impl {ty}"),
    }
}

fn trait_label(node: &Node, src: &[u8]) -> String {
    let name = node
        .child_by_field_name("name")
        .and_then(|n| n.utf8_text(src).ok())
        .unwrap_or("?");
    format!("trait {name}")
}

static SCOPES: &[ScopeSpec] = &[
    ScopeSpec {
        kind: "impl_item",
        label: impl_label,
    },
    ScopeSpec {
        kind: "trait_item",
        label: trait_label,
    },
];

/// The callee inside a `call_expression` function field: identifier,
/// `x.foo()` field_expression, `A::b()` scoped_identifier, or `f::<T>()`
/// generic_function (which wraps one of the former).
fn callee_of(fnode: &Node, src: &[u8], out: &mut BodyFacts, site: &Node) {
    let (name, qual, kind) = match fnode.kind() {
        "identifier" => (fnode.utf8_text(src).unwrap_or(""), None, CallKind::Function),
        "field_expression" => (
            body::field_text(fnode, "field", src).unwrap_or(""),
            body::field_text(fnode, "value", src).map(body::squash),
            CallKind::Method,
        ),
        "scoped_identifier" => (
            body::field_text(fnode, "name", src).unwrap_or(""),
            body::field_text(fnode, "path", src).map(body::squash),
            CallKind::Scoped,
        ),
        "generic_function" => {
            if let Some(inner) = fnode.child_by_field_name("function") {
                return callee_of(&inner, src, out, site);
            }
            ("", None, CallKind::Function)
        }
        _ => ("", None, CallKind::Function),
    };
    if !name.is_empty() {
        out.calls.push(body::call(
            name,
            qual,
            kind,
            body::field_arity(site, "arguments"),
            site,
        ));
    }
}

/// Extract calls, param/return types, and `self` field reads/writes.
fn body_of(fnode: &Node, src: &[u8]) -> BodyFacts {
    let mut out = BodyFacts::default();
    let text = |n: &Node| n.utf8_text(src).unwrap_or("").to_string();

    if let Some(params) = fnode.child_by_field_name("parameters") {
        let mut c = params.walk();
        for p in params.named_children(&mut c) {
            match p.kind() {
                "parameter" => {
                    let pat = body::field_text(&p, "pattern", src).unwrap_or("_");
                    let ty = body::field_text(&p, "type", src);
                    out.param_types.push(match ty {
                        Some(t) => format!("{}: {}", body::squash(pat), body::squash(t)),
                        None => body::squash(pat),
                    });
                }
                "self_parameter" => {
                    let t = body::squash(&text(&p));
                    if t.contains("mut") {
                        out.mutates_receiver = true;
                    }
                    out.param_types.push(t);
                }
                "variadic_parameter" => out.param_types.push("...".into()),
                _ => {}
            }
        }
    }
    out.return_type = body::field_text(fnode, "return_type", src).map(body::squash);

    if let Some(body_node) = fnode.child_by_field_name("body") {
        body::descend(&body_node, &mut |n| {
            match n.kind() {
                // Nested fns get their own BodyFacts — don't descend.
                "function_item" | "function_signature_item" => return false,
                "call_expression" => {
                    if let Some(f) = n.child_by_field_name("function") {
                        callee_of(&f, src, &mut out, n);
                    }
                }
                "macro_invocation" => {
                    if let Some(m) = n.child_by_field_name("macro") {
                        let (name, qual) = if m.kind() == "scoped_identifier" {
                            (
                                body::field_text(&m, "name", src).unwrap_or(""),
                                body::field_text(&m, "path", src).map(body::squash),
                            )
                        } else {
                            (m.utf8_text(src).unwrap_or(""), None)
                        };
                        if !name.is_empty() {
                            out.calls
                                .push(body::call(name, qual, CallKind::Macro, 0, n));
                        }
                    }
                }
                _ => {}
            }
            true
        });

        // `self.x` reads vs writes: collect every assignment-LHS subtree, mark
        // the self-member appearing inside as written; every self-field
        // occurrence elsewhere counts as a read.
        let mut writes = Vec::new();
        body::descend(&body_node, &mut |n| {
            match n.kind() {
                "function_item" => return false,
                "assignment_expression" | "compound_assignment_expr" => {
                    if let Some(lhs) = n.child_by_field_name("left") {
                        let mut visit = |l: &Node| -> bool {
                            if l.kind() == "field_expression"
                                && body::field_text(l, "value", src) == Some("self")
                            {
                                if let Some(m) = body::field_text(l, "field", src) {
                                    writes.push(m.to_string());
                                }
                                return false;
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
            if n.kind() == "function_item" {
                return false;
            }
            if n.kind() == "field_expression"
                && body::field_text(n, "value", src) == Some("self")
            {
                if let Some(m) = body::field_text(n, "field", src) {
                    reads.push(m.to_string());
                }
            }
            true
        });
        for w in &writes {
            if !out.writes_fields.contains(w) {
                out.writes_fields.push(w.clone());
            }
        }
        // A member that is only ever a write LHS is still a write; compound
        // (`+=`) sites appear in `reads` too and stay there — honest.
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

impl Analyzer for RustAnalyzer {
    fn analyze(&self, source: &str) -> Result<FileAnalysis> {
        let spec = LangSpec {
            language: Language::from(tree_sitter_rust::LANGUAGE),
            comment_kinds: &["line_comment", "block_comment"],
            function_kinds: &["function_item", "function_signature_item"],
            method_scopes: SCOPES,
            doc_prefixes: &["///", "//!", "/**", "/*!"],
            doc_extractor: None,
            visibility: |node, _src, _name| {
                let mut cursor = node.walk();
                let found = node
                    .children(&mut cursor)
                    .any(|c| c.kind() == "visibility_modifier");
                found
            },
            body_extractor: Some(body_of),
        };
        extract(&spec, source)
    }
}
