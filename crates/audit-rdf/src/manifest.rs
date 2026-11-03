//! Reader for the canonical line-subset Turtle emitted in `line_mode`.
//!
//! Each non-comment, non-prefix line has the form `subject predicate object .`
//! where object is a prefixed IRI, a quoted literal, or a typed literal.

use thiserror::Error;

use crate::Term;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("malformed manifest line: {0}")]
    Malformed(String),
    #[error("unterminated literal in line: {0}")]
    UnterminatedLiteral(String),
}

/// Unescape a Turtle literal body.
pub fn unescape_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Parse one canonical manifest line into (subject, predicate, object).
/// Returns `Ok(None)` for blank lines, comments, and `@prefix` directives.
pub fn parse_manifest_line(line: &str) -> Result<Option<(String, String, Term)>, ParseError> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') || line.starts_with("@prefix") {
        return Ok(None);
    }
    let line = line.strip_suffix('.').map(str::trim_end).unwrap_or(line);

    // subject
    let (subject, rest) = line
        .split_once(' ')
        .ok_or_else(|| ParseError::Malformed(line.to_string()))?;
    // predicate
    let rest = rest.trim_start();
    let (predicate, rest) = rest
        .split_once(' ')
        .ok_or_else(|| ParseError::Malformed(line.to_string()))?;
    let obj = rest.trim();

    let term = if let Some(inner) = obj.strip_prefix('"') {
        // literal, possibly typed: find closing quote (unescaped)
        let mut end = None;
        let mut escaped = false;
        for (i, c) in inner.char_indices() {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                end = Some(i);
                break;
            }
        }
        let i = end.ok_or_else(|| ParseError::UnterminatedLiteral(line.to_string()))?;
        let value = unescape_literal(&inner[..i]);
        let after = inner[i + 1..].trim();
        if let Some(dt) = after.strip_prefix("^^") {
            Term::TypedLiteral {
                value,
                datatype: dt.to_string(),
            }
        } else if after.is_empty() {
            Term::Literal(value)
        } else {
            return Err(ParseError::Malformed(line.to_string()));
        }
    } else if obj == "true" || obj == "false" {
        Term::Bool(obj == "true")
    } else if obj.parse::<i64>().is_ok() {
        Term::Int(obj.parse().unwrap())
    } else {
        Term::Iri(obj.to_string())
    };

    Ok(Some((
        subject.to_string(),
        predicate.to_string(),
        term,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_iri_object() {
        let (s, p, o) = parse_manifest_line("audit:f_a a audit:SourceFile .")
            .unwrap()
            .unwrap();
        assert_eq!(s, "audit:f_a");
        assert_eq!(p, "a");
        assert_eq!(o, Term::Iri("audit:SourceFile".into()));
    }

    #[test]
    fn parses_literal_with_escaped_quote() {
        let line = r#"audit:f_a audit:text "say \"hi\" \\ done" ."#;
        let (_, _, o) = parse_manifest_line(line).unwrap().unwrap();
        assert_eq!(o, Term::Literal("say \"hi\" \\ done".into()));
    }

    #[test]
    fn parses_typed_literal() {
        let line = r#"audit:f_a audit:processedAt "2026-10-17T01:02:03Z"^^xsd:dateTime ."#;
        let (_, _, o) = parse_manifest_line(line).unwrap().unwrap();
        assert_eq!(
            o,
            Term::TypedLiteral {
                value: "2026-10-17T01:02:03Z".into(),
                datatype: "xsd:dateTime".into()
            }
        );
    }

    #[test]
    fn skips_prefix_and_comments() {
        assert!(parse_manifest_line("@prefix audit: <x> .").unwrap().is_none());
        assert!(parse_manifest_line("# hello").unwrap().is_none());
        assert!(parse_manifest_line("").unwrap().is_none());
    }
}
