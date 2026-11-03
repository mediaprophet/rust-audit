use anyhow::Result;
use tree_sitter::{Language, Node};

use crate::common::{body, extract, LangSpec};
use crate::{Analyzer, BodyFacts, CallKind, FileAnalysis};

pub struct WgslAnalyzer;

/// First identifier text under a node. Builtin vector/matrix names
/// (`vec3<f32>`) are anonymous tokens in the grammar, so fall back to the
/// leading word of the node text.
fn first_ident<'a>(node: &Node, src: &'a [u8]) -> Option<&'a str> {
    if node.kind() == "identifier" {
        return node.utf8_text(src).ok();
    }
    let mut c = node.walk();
    for ch in node.named_children(&mut c) {
        if let Some(t) = first_ident(&ch, src) {
            return Some(t);
        }
    }
    None
}

/// Leading word (`vec3<f32>` → `vec3`, `MyStruct` → `MyStruct`).
fn head_word(text: &str) -> &str {
    let end = text
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(text.len());
    &text[..end]
}

/// Extract calls (`type_constructor_or_function_call_expression`), param
/// types (`variable_identifier_declaration`), and return type.
fn body_of(fnode: &Node, src: &[u8]) -> BodyFacts {
    let mut out = BodyFacts::default();

    if let Some(params) = fnode.child_by_field_name("parameters") {
        let mut c = params.walk();
        for p in params.named_children(&mut c) {
            if p.kind() == "parameter" {
                if let Some(vid) = p.named_child(0) {
                    if vid.kind() == "variable_identifier_declaration" {
                        let name = body::field_text(&vid, "name", src).unwrap_or("_");
                        let ty = body::field_text(&vid, "type", src);
                        out.param_types.push(match ty {
                            Some(t) => format!("{}: {}", name, body::squash(t)),
                            None => name.to_string(),
                        });
                    }
                }
            }
        }
    }
    if let Some(rt) = fnode.child_by_field_name("type") {
        out.return_type = Some(
            rt.utf8_text(src)
                .unwrap_or("")
                .trim_start_matches("->")
                .trim()
                .to_string(),
        );
    }

    if let Some(body_node) = fnode.child_by_field_name("body") {
        body::descend(&body_node, &mut |n| {
            match n.kind() {
                "function_declaration" => return false,
                "type_constructor_or_function_call_expression" => {
                    // callee = leading type_declaration child
                    if let Some(td) = n.named_child(0) {
                        if td.kind() == "type_declaration" {
                            let td_text = td.utf8_text(src).unwrap_or("");
                            let name = first_ident(&td, src)
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| head_word(td_text).to_string());
                            if !name.is_empty() {
                                let arity = n
                                    .named_children(&mut n.walk())
                                    .find(|c| c.kind() == "argument_list_expression")
                                    .map(|a| a.named_child_count() as u32)
                                    .unwrap_or(0);
                                out.calls.push(body::call(
                                    &name,
                                    None,
                                    CallKind::Function,
                                    arity,
                                    n,
                                ));
                            }
                        }
                    }
                }
                _ => {}
            }
            true
        });
    }
    out
}

impl Analyzer for WgslAnalyzer {
    fn analyze(&self, source: &str) -> Result<FileAnalysis> {
        let spec = LangSpec {
            language: Language::from(tree_sitter_wgsl_bevy::LANGUAGE),
            comment_kinds: &["line_comment", "block_comment"],
            function_kinds: &["function_declaration"],
            method_scopes: &[],
            doc_prefixes: &[],
            doc_extractor: None,
            visibility: |_node, _src, _name| true,
            body_extractor: Some(body_of),
        };
        extract(&spec, source)
    }
}
