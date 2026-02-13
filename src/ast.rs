/// Intermediate representation produced by the parser.
/// Mirrors the TypeScript `TagStatement` types.

/// A scalar or reference value.
#[derive(Debug, Clone, PartialEq)]
pub enum ScalarValue {
    String(String),
    Number(f64),
    Boolean(bool),
    /// ISO 8601 date string
    Date(String),
    /// Reference: ups = number of `^` characters, path = dotted segments with optional array indices
    Reference {
        ups: usize,
        path: Vec<RefPathSegment>,
    },
}

/// A segment in a reference path: either a named property or an array index.
#[derive(Debug, Clone, PartialEq)]
pub enum RefPathSegment {
    Name(String),
    Index(usize),
}

/// A value that can be assigned with `=`.
#[derive(Debug, Clone, PartialEq)]
pub enum TagValue {
    Scalar(ScalarValue),
    Array(Vec<ArrayElement>),
}

/// An element in an array literal.
#[derive(Debug, Clone, PartialEq)]
pub struct ArrayElement {
    pub value: Option<TagValue>,
    pub properties: Option<Vec<Statement>>,
}

/// A parsed statement (the IR between the parser and interpreter).
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    /// `name = value` optionally followed by `{ properties }` or `{ ... }`
    SetEq {
        path: Vec<String>,
        value: TagValue,
        /// If present, the `{ ... }` block contained these statements (replace properties)
        properties: Option<Vec<Statement>>,
        /// If true, `{ ... }` was used (preserve existing properties)
        preserve_properties: bool,
    },
    /// `name = { properties }` or `name: { properties }` or `name = ... { properties }`
    ReplaceProperties {
        path: Vec<String>,
        properties: Vec<Statement>,
        /// If true, `...` was used (preserve existing value)
        preserve_value: bool,
    },
    /// `name { properties }` (merge semantics)
    UpdateProperties {
        path: Vec<String>,
        properties: Vec<Statement>,
    },
    /// `name` or `-name`
    Define {
        path: Vec<String>,
        deleted: bool,
    },
    /// `-...`
    ClearAll,
}
