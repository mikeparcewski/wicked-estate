//! Graph nodes — the entities in the code graph. See `docs/adr/ADR-001-graph-schema.md`.

use crate::symbol::SymbolId;
use serde::{Deserialize, Serialize};

/// A language tag, keyed by tree-sitter grammar name. A **newtype, not an enum**, so that
/// adding a language is zero core-code change (rules-as-data; see Wave 6.3 / research 06).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Language(pub String);

impl Language {
    pub fn new(s: impl Into<String>) -> Self {
        Language(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A byte+line+column span. Mutable across edits; **not** part of symbol identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Span {
    pub start_byte: u32,
    pub end_byte: u32,
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

impl Span {
    pub const ZERO: Span = Span {
        start_byte: 0,
        end_byte: 0,
        start_line: 0,
        start_col: 0,
        end_line: 0,
        end_col: 0,
    };
}

/// Where a node lives. The `file` is repo-relative.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Location {
    pub file: String,
    pub span: Span,
}

impl Location {
    pub fn new(file: impl Into<String>, span: Span) -> Self {
        Self {
            file: file.into(),
            span,
        }
    }
}

/// The kind of a node. `Other(String)` keeps the model open for extractor-specific kinds.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    File,
    Module,
    Namespace,
    Class,
    Struct,
    Enum,
    Interface,
    Trait,
    Function,
    Method,
    Constructor,
    Field,
    Constant,
    Variable,
    Parameter,
    TypeAlias,
    Macro,
    Import,
    /// A synthetic node injected by a drop-in extractor (event-bus topic, capability, …).
    Synthetic,
    Other(String),
}

/// Free-form, JSON-typed extension bag (matches the edge/node metadata pattern).
pub type Metadata = serde_json::Map<String, serde_json::Value>;

/// A node in the code graph. The [`SymbolId`] is its stable primary key; `location` is mutable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub symbol: SymbolId,
    pub kind: NodeKind,
    pub name: String,
    pub language: Language,
    pub location: Location,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    #[serde(default)]
    pub metadata: Metadata,
}

impl Node {
    pub fn new(
        symbol: SymbolId,
        kind: NodeKind,
        name: impl Into<String>,
        language: Language,
        location: Location,
    ) -> Self {
        Self {
            symbol,
            kind,
            name: name.into(),
            language,
            location,
            signature: None,
            doc: None,
            metadata: Metadata::new(),
        }
    }
}

/// A single source file handed to an [`crate::Extractor`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    pub path: String,
    pub language: Language,
    pub text: String,
}
