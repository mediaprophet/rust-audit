//! Per-language source analysis: extract comments and functions with
//! tree-sitter, returning a language-agnostic `FileAnalysis`.

mod common;
mod python;
mod rust;
mod wgsl;

use anyhow::Result;
use serde::Serialize;

/// A comment found in a source file.
#[derive(Debug, Clone, Serialize)]
pub struct ExtractedComment {
    /// Comment body with markers stripped.
    pub text: String,
    /// 1-based start line.
    pub line: u32,
    /// 1-based end line.
    pub end_line: u32,
    /// Documentation comment (`///`, `//!`, `/**`, `/*!`, or Python docstring).
    pub is_doc: bool,
    /// Doc comment that documents the *following* item (`///`, `/**`) — as
    /// opposed to inner docs (`//!`, `/*!`) which document the enclosing scope.
    /// Used for doc→function attachment.
    pub outer: bool,
}

/// Whether a function is free-standing or belongs to a type/trait/class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum FunctionKind {
    Function,
    Method,
}

/// How a call site names its callee.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CallKind {
    /// Bare `foo(...)` call.
    Function,
    /// Receiver-qualified `x.foo(...)` / `self.foo(...)`.
    Method,
    /// Path-qualified `Type::foo(...)` / `mod::foo(...)`.
    Scoped,
    /// Macro invocation `name!(...)` (Rust).
    Macro,
}

/// A call site found inside a function body. Name-resolved only — no type
/// inference — so `x.foo()` records qualifier `x`, callee `foo`.
#[derive(Debug, Clone, Serialize)]
pub struct ExtractedCall {
    /// Callee identifier (`foo`, `apply_force`).
    pub name: String,
    /// Receiver or path qualifier text (`self`, `x`, `Body`, `crate::m`).
    pub qualifier: Option<String>,
    pub kind: CallKind,
    /// Argument count at the call site.
    pub arity: u32,
    /// 1-based line of the call.
    pub line: u32,
}

/// Intra-procedural facts pulled from a function's signature + body.
#[derive(Debug, Default, Clone, Serialize)]
pub struct BodyFacts {
    pub calls: Vec<ExtractedCall>,
    /// Parameter declarations as `name: type` (or bare `name` untyped).
    pub param_types: Vec<String>,
    /// Declared return type text, if any.
    pub return_type: Option<String>,
    /// Member fields read on `self` (Rust `self.x`, Python `self.x`).
    pub reads_fields: Vec<String>,
    /// Member fields written on `self` (assignment LHS, compound assign).
    pub writes_fields: Vec<String>,
    /// `&mut self` receiver or any `self` field write.
    pub mutates_receiver: bool,
}

/// A function or method found in a source file.
#[derive(Debug, Clone, Serialize)]
pub struct ExtractedFunction {
    pub name: String,
    /// Single-line signature text (attributes included when grammar exposes them).
    pub signature: String,
    /// 1-based start line.
    pub line: u32,
    /// 1-based end line.
    pub end_line: u32,
    pub kind: FunctionKind,
    /// Enclosing scope label, e.g. `impl Foo`, `class Bar`.
    pub scope: Option<String>,
    /// Adjacent doc comment / docstring text, if any.
    pub doc: Option<String>,
    /// Parameter count (includes `self` for methods).
    pub arity: u32,
    /// Publicly visible per the language's rules (`pub` in Rust, no `_`-prefix
    /// in Python — dunders stay public — always true in WGSL).
    pub public: bool,
    /// Call sites + signature/body data-flow facts (name-resolved).
    pub body: BodyFacts,
}

/// Result of analyzing one source file.
#[derive(Debug, Default, Serialize)]
pub struct FileAnalysis {
    pub comments: Vec<ExtractedComment>,
    pub functions: Vec<ExtractedFunction>,
}

/// Language-specific analyzer.
pub trait Analyzer {
    fn analyze(&self, source: &str) -> Result<FileAnalysis>;
}

/// Get the analyzer for a language id (`rust`, `python`, `wgsl`).
pub fn analyzer_for(language: &str) -> Option<Box<dyn Analyzer>> {
    match language {
        "rust" => Some(Box::new(rust::RustAnalyzer)),
        "python" => Some(Box::new(python::PythonAnalyzer)),
        "wgsl" => Some(Box::new(wgsl::WgslAnalyzer)),
        _ => None,
    }
}
