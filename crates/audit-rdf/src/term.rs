use std::fmt;

use crate::escape_literal;

/// An RDF term usable as predicate object (or inline subject).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Term {
    /// A prefixed or absolute IRI, e.g. `audit:SourceFile` or `<https://...>`.
    Iri(String),
    /// A plain literal.
    Literal(String),
    /// A typed literal, e.g. `"2026-10-17T12:00:00Z"^^xsd:dateTime`.
    TypedLiteral { value: String, datatype: String },
    /// An integer literal.
    Int(i64),
    /// A boolean literal.
    Bool(bool),
}

impl Term {
    pub fn iri(s: impl Into<String>) -> Self {
        Term::Iri(s.into())
    }

    pub fn lit(s: impl Into<String>) -> Self {
        Term::Literal(s.into())
    }

    pub fn typed(s: impl Into<String>, datatype: impl Into<String>) -> Self {
        Term::TypedLiteral {
            value: s.into(),
            datatype: datatype.into(),
        }
    }

    pub fn int(n: i64) -> Self {
        Term::Int(n)
    }

    pub fn bool(b: bool) -> Self {
        Term::Bool(b)
    }
}

impl fmt::Display for Term {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Term::Iri(i) => {
                if i.starts_with('<') {
                    write!(f, "{i}")
                } else {
                    write!(f, "{i}")
                }
            }
            Term::Literal(v) => write!(f, "\"{}\"", escape_literal(v)),
            Term::TypedLiteral { value, datatype } => {
                write!(f, "\"{}\"^^{}", escape_literal(value), datatype)
            }
            Term::Int(n) => write!(f, "{n}"),
            Term::Bool(b) => write!(f, "{b}"),
        }
    }
}
