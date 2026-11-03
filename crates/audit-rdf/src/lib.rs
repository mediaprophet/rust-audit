//! Minimal Turtle (TTL) writer and a canonical line-subset reader.
//!
//! The writer emits pretty Turtle for per-file graphs. The manifest is written
//! in a restricted canonical form — one `subject predicate object .` triple per
//! line — so it can be re-read without a full Turtle parser.

mod term;
mod writer;
mod manifest;

pub use term::Term;
pub use writer::TurtleWriter;
pub use manifest::{parse_manifest_line, unescape_literal};

/// Namespace IRI for the audit vocabulary.
pub const AUDIT_NS: &str = "https://audit.dev/ns#";
/// Prefix used when minting IRIs for file nodes.
pub const FILE_PREFIX: &str = "f_";
/// Prefix used when minting IRIs for directory nodes.
pub const DIR_PREFIX: &str = "d_";
/// Prefix used when minting IRIs for function nodes.
pub const FN_PREFIX: &str = "fn_";
/// Prefix used when minting IRIs for comment nodes.
pub const COMMENT_PREFIX: &str = "c_";

/// Escape a string for use as a Turtle literal body (without quotes).
pub fn escape_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

/// Turn a relative path (or any string) into a safe Turtle local name.
/// Alphanumerics and `_`/`-` are kept; everything else becomes `_`.
pub fn slugify(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}
