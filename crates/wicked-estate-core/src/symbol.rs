//! Stable symbol identity (SCIP-inspired) — see `docs/adr/ADR-002-stable-symbol-identity.md`.
//!
//! Identity is derived from a symbol's *logical name path*, never from source bytes or line
//! numbers. Reformatting, line shifts, and unrelated edits in a file therefore do **not** churn
//! IDs, and edges survive. A true rename — or a module-path move — yields a *new* identity,
//! which is correct: it is a different logical symbol. This is the deliberate fix for the
//! content-hash node IDs that break on rename.

use serde::{Deserialize, Serialize};
use std::fmt;

/// The kind of a name component, rendered with a SCIP-style sigil.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Suffix {
    /// module / package path component → `name/`
    Namespace,
    /// type → `name#`
    Type,
    /// value (function, constant, field) → `name.`
    Term,
    /// method (may carry an overload disambiguator) → `name(disamb).`
    Method,
    /// type parameter → `[name]`
    TypeParameter,
    /// parameter → `(name)`
    Parameter,
    /// meta / anchor → `name:`
    Meta,
    /// macro → `name!`
    Macro,
}

impl Suffix {
    fn render(self, name: &str, disambiguator: Option<&str>) -> String {
        match self {
            Suffix::Namespace => format!("{name}/"),
            Suffix::Type => format!("{name}#"),
            Suffix::Term => format!("{name}."),
            Suffix::Method => format!("{name}({}).", disambiguator.unwrap_or("")),
            Suffix::TypeParameter => format!("[{name}]"),
            Suffix::Parameter => format!("({name})"),
            Suffix::Meta => format!("{name}:"),
            Suffix::Macro => format!("{name}!"),
        }
    }
}

/// One component of a symbol's logical path.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Descriptor {
    pub name: String,
    pub suffix: Suffix,
    /// Disambiguator for overloaded methods (arity or a stable hash). Part of identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disambiguator: Option<String>,
}

impl Descriptor {
    pub fn new(name: impl Into<String>, suffix: Suffix) -> Self {
        Self {
            name: name.into(),
            suffix,
            disambiguator: None,
        }
    }
    pub fn method(name: impl Into<String>, disambiguator: Option<String>) -> Self {
        Self {
            name: name.into(),
            suffix: Suffix::Method,
            disambiguator,
        }
    }
}

/// Package coordinates for a *global* symbol — stable across file moves within the package.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Package {
    pub manager: String, // npm, pip, cargo, go, maven, …
    pub name: String,
    pub version: String,
}

/// A structured, stable symbol identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Symbol {
    /// File-scoped local (function locals, block bindings). Not addressable cross-file.
    Local {
        scheme: String,
        file: String,
        id: String,
    },
    /// Globally addressable symbol (exported / qualified by a logical path).
    Global {
        scheme: String,
        package: Option<Package>,
        descriptors: Vec<Descriptor>,
    },
    /// A source-file node.
    File { path: String },
    /// A synthetic node injected by an extractor (event-bus topic, capability, …).
    Synthetic { scheme: String, id: String },
}

impl Symbol {
    /// The canonical string identity used as the primary key everywhere in storage.
    pub fn id(&self) -> SymbolId {
        SymbolId(self.to_string())
    }
    pub fn file(path: impl Into<String>) -> Self {
        Symbol::File { path: path.into() }
    }
    pub fn global(
        scheme: impl Into<String>,
        package: Option<Package>,
        descriptors: Vec<Descriptor>,
    ) -> Self {
        Symbol::Global {
            scheme: scheme.into(),
            package,
            descriptors,
        }
    }
    pub fn synthetic(scheme: impl Into<String>, id: impl Into<String>) -> Self {
        Symbol::Synthetic {
            scheme: scheme.into(),
            id: id.into(),
        }
    }
}

impl fmt::Display for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Symbol::Local { scheme, file, id } => write!(f, "{scheme} local {file}#{id}"),
            Symbol::File { path } => write!(f, "file . {path}:"),
            Symbol::Synthetic { scheme, id } => write!(f, "{scheme} synthetic {id}:"),
            Symbol::Global {
                scheme,
                package,
                descriptors,
            } => {
                write!(f, "{scheme} ")?;
                match package {
                    Some(p) => write!(f, "{} {} {} ", p.manager, p.name, p.version)?,
                    None => write!(f, ". . . ")?,
                }
                for d in descriptors {
                    write!(
                        f,
                        "{}",
                        d.suffix.render(&d.name, d.disambiguator.as_deref())
                    )?;
                }
                Ok(())
            }
        }
    }
}

/// The canonical string form of a [`Symbol`] — the primary key used in storage and edges.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SymbolId(pub String);

impl SymbolId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SymbolId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for SymbolId {
    fn from(s: &str) -> Self {
        SymbolId(s.to_string())
    }
}

impl From<String> for SymbolId {
    fn from(s: String) -> Self {
        SymbolId(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn func(module: &str, name: &str) -> Symbol {
        Symbol::global(
            "scip-ts",
            None,
            vec![
                Descriptor::new(module, Suffix::Namespace),
                Descriptor::method(name, None),
            ],
        )
    }

    /// ADR-002 worked example: identity is independent of source location.
    /// Two definitions of the same logical symbol at *different* line numbers share an id.
    #[test]
    fn id_is_stable_across_location_changes() {
        let before = func("src/util", "parse");
        let after = func("src/util", "parse"); // same logical path, imagine lines shifted
        assert_eq!(
            before.id(),
            after.id(),
            "line shifts must not churn the symbol id"
        );
    }

    /// A genuine rename yields a new identity (correct: it is a different symbol).
    #[test]
    fn rename_changes_identity() {
        let original = func("src/util", "parse");
        let renamed = func("src/util", "parse_input");
        assert_ne!(original.id(), renamed.id());
    }

    /// A module-path move is a logical change → new identity (also correct).
    #[test]
    fn module_move_changes_identity() {
        let here = func("src/util", "parse");
        let moved = func("src/text", "parse");
        assert_ne!(here.id(), moved.id());
    }

    #[test]
    fn renders_scip_like_string() {
        let s = func("src/util", "parse").id();
        assert_eq!(s.as_str(), "scip-ts . . . src/util/parse().");
    }
}
