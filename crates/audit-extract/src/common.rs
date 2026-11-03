//! Shared spec-driven tree-sitter extraction.

use anyhow::{anyhow, Result};
use tree_sitter::{Language, Node, Parser};

use crate::{
    BodyFacts, ExtractedComment, ExtractedFunction, FileAnalysis, FunctionKind,
};

/// Declarative description of one language's grammar surface.
pub struct LangSpec {
    pub language: Language,
    /// Node kinds that are comments.
    pub comment_kinds: &'static [&'static str],
    /// Node kinds that are functions/methods.
    pub function_kinds: &'static [&'static str],
    /// Node kinds that turn enclosed functions into methods, with the field
    /// holding the scope's display name.
    pub method_scopes: &'static [ScopeSpec],
    /// Comment-text prefixes that mark doc comments.
    pub doc_prefixes: &'static [&'static str],
    /// Extract a docstring from inside a function node (e.g. Python).
    pub doc_extractor: Option<fn(&Node, &[u8]) -> Option<String>>,
    /// Decide whether a function is publicly visible.
    pub visibility: fn(&Node, &[u8], &str) -> bool,
    /// Extract intra-procedural facts (calls, param/return types, self-field
    /// reads/writes) from a function node. `None` yields empty facts.
    pub body_extractor: Option<fn(&Node, &[u8]) -> BodyFacts>,
}

pub struct ScopeSpec {
    pub kind: &'static str,
    /// Build a scope label from the scope node, e.g. `impl Foo`.
    pub label: fn(&Node, &[u8]) -> String,
}

/// Run the extraction for one source string.
pub fn extract(spec: &LangSpec, source: &str) -> Result<FileAnalysis> {
    let mut parser = Parser::new();
    parser
        .set_language(&spec.language)
        .map_err(|e| anyhow!("loading grammar: {e}"))?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| anyhow!("tree-sitter returned no tree"))?;
    let src = source.as_bytes();

    let mut comments = Vec::new();
    let mut functions = Vec::new();
    walk(&tree.root_node(), src, spec, None, &mut comments, &mut functions);

    attach_docs(&mut functions, &comments, source);
    comments.sort_by_key(|c| c.line);
    functions.sort_by_key(|f| f.line);
    Ok(FileAnalysis { comments, functions })
}

fn walk(
    node: &Node,
    src: &[u8],
    spec: &LangSpec,
    scope: Option<String>,
    comments: &mut Vec<ExtractedComment>,
    functions: &mut Vec<ExtractedFunction>,
) {
    let kind = node.kind();

    if spec.comment_kinds.contains(&kind) {
        comments.push(make_comment(node, src, spec));
    }

    let is_function = spec.function_kinds.contains(&kind);
    if is_function {
        let mut f = make_function(node, src, &scope, spec);
        if let Some(extract_doc) = spec.doc_extractor {
            f.doc = extract_doc(node, src);
        }
        functions.push(f);
    }

    // Determine the scope children see. Children of a function node get no
    // scope — a nested def inside a method is not itself a method.
    let child_scope = if is_function {
        None
    } else {
        spec.method_scopes
            .iter()
            .find(|s| s.kind == kind)
            .map(|s| (s.label)(node, src))
            .or(scope.clone())
    };

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(&child, src, spec, child_scope.clone(), comments, functions);
    }
}

fn node_text<'a>(node: &Node, src: &'a [u8]) -> &'a str {
    node.utf8_text(src).unwrap_or("")
}

fn make_comment(node: &Node, src: &[u8], spec: &LangSpec) -> ExtractedComment {
    let raw = node_text(node, src);
    let trimmed = raw.trim_start();
    let is_doc = spec
        .doc_prefixes
        .iter()
        .any(|p| trimmed.starts_with(p));
    let outer = trimmed.starts_with("///") || trimmed.starts_with("/**");
    // Comment nodes often swallow the trailing newline, ending at column 0 of
    // the following row; that row isn't really part of the comment.
    let end = node.end_position();
    let end_line = if end.column == 0 && end.row > node.start_position().row {
        end.row as u32
    } else {
        end.row as u32 + 1
    };
    ExtractedComment {
        text: clean_comment(raw),
        line: node.start_position().row as u32 + 1,
        end_line,
        is_doc,
        outer,
    }
}

/// Strip comment markers from raw text.
fn clean_comment(raw: &str) -> String {
    let t = raw.trim();
    if let Some(body) = t
        .strip_prefix("///")
        .or_else(|| t.strip_prefix("//!"))
        .or_else(|| t.strip_prefix("//"))
        .or_else(|| t.strip_prefix('#'))
    {
        return body.trim().to_string();
    }
    if t.starts_with("/*") {
        let body = t
            .trim_start_matches('/')
            .trim_start_matches('*')
            .trim_end_matches('/')
            .trim_end_matches('*');
        return body.trim().to_string();
    }
    t.to_string()
}

fn make_function(
    node: &Node,
    src: &[u8],
    scope: &Option<String>,
    spec: &LangSpec,
) -> ExtractedFunction {
    let name = node
        .child_by_field_name("name")
        .map(|n| node_text(&n, src).to_string())
        .unwrap_or_else(|| "<anonymous>".to_string());

    // Signature: node start → body start (exclusive), or first line if no body.
    let signature = match node.child_by_field_name("body") {
        Some(body) if body.start_byte() > node.start_byte() => {
            node_text(node, src)[..body.start_byte() - node.start_byte()].to_string()
        }
        _ => node_text(node, src)
            .lines()
            .next()
            .unwrap_or("")
            .to_string(),
    };
    let signature = signature.split_whitespace().collect::<Vec<_>>().join(" ");

    // Arity: named children of the `parameters` field node (`parameters` in
    // rust/python grammars, `parameter_list` in wgsl-bevy).
    let arity = node
        .child_by_field_name("parameters")
        .map(|p| p.named_child_count() as u32)
        .unwrap_or(0);

    let public = (spec.visibility)(node, src, &name);
    let body = spec
        .body_extractor
        .map(|extract| extract(node, src))
        .unwrap_or_default();
    ExtractedFunction {
        name,
        signature,
        arity,
        line: node.start_position().row as u32 + 1,
        end_line: node.end_position().row as u32 + 1,
        kind: if scope.is_some() {
            FunctionKind::Method
        } else {
            FunctionKind::Function
        },
        scope: scope.clone(),
        doc: None,
        public,
        body,
    }
}

/// Shared helpers for per-language `body_extractor` implementations.
pub mod body {
    use super::node_text;
    use crate::{CallKind, ExtractedCall};
    use tree_sitter::Node;

    /// Walk `node` descendants, invoking `visit` on each. Children of nodes
    /// for which `visit` returns `false` are skipped.
    pub fn descend<F: FnMut(&Node) -> bool>(node: &Node, visit: &mut F) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if visit(&child) {
                descend(&child, visit);
            }
        }
    }

    /// Text of a node's named field, or `None`.
    pub fn field_text<'a>(node: &Node, field: &str, src: &'a [u8]) -> Option<&'a str> {
        node.child_by_field_name(field)
            .map(|n| node_text(&n, src))
            .filter(|t| !t.trim().is_empty())
    }

    /// Count named children of a node's field (e.g. call arguments).
    pub fn field_arity(node: &Node, field: &str) -> u32 {
        node.child_by_field_name(field)
            .map(|n| n.named_child_count() as u32)
            .unwrap_or(0)
    }

    /// Collapse whitespace for stable qualifier/type strings.
    pub fn squash(text: &str) -> String {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// Build a call record.
    pub fn call(
        name: &str,
        qualifier: Option<String>,
        kind: CallKind,
        arity: u32,
        node: &Node,
    ) -> ExtractedCall {
        ExtractedCall {
            name: name.to_string(),
            qualifier,
            kind,
            arity,
            line: node.start_position().row as u32 + 1,
        }
    }
}

/// Attach a contiguous run of doc comments ending above a function to that
/// function's `doc`. Attribute lines (`#[...]`) may sit between the doc block
/// and the item without detaching it; `#[doc = "..."]` attributes are
/// themselves doc sources.
fn attach_docs(functions: &mut [ExtractedFunction], comments: &[ExtractedComment], src: &str) {
    let lines: Vec<&str> = src.lines().collect();
    for f in functions.iter_mut() {
        if f.doc.is_some() {
            continue;
        }
        let mut doc_lines: Vec<String> = Vec::new();
        let mut cursor_line = f.line;
        loop {
            // Step over attribute lines between the doc block and the item.
            let prev = cursor_line.saturating_sub(1) as usize;
            if prev >= 1 {
                let text = lines[prev - 1].trim();
                if text.starts_with("#[") {
                    if let Some(doc) = doc_attr_text(text) {
                        doc_lines.push(doc.to_string());
                    }
                    cursor_line = prev as u32;
                    continue;
                }
            }
            let doc = comments.iter().find(|c| {
                c.outer && c.end_line + 1 == cursor_line
            });
            match doc {
                Some(c) => {
                    doc_lines.push(c.text.clone());
                    cursor_line = c.line;
                }
                None => break,
            }
        }
        if !doc_lines.is_empty() {
            doc_lines.reverse();
            f.doc = Some(doc_lines.join("\n"));
        }
    }
}

/// Extract the literal of a `#[doc = "..."]` attribute line.
fn doc_attr_text(line: &str) -> Option<&str> {
    let inner = line.strip_prefix("#[doc = ")?.trim_end_matches(']');
    inner.strip_prefix('"')?.strip_suffix('"')
}
