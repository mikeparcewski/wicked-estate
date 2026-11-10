//! The two-phase pipeline staging types (EXTRACT → RESOLVE).
//!
//! Extraction is per-file and parallelizable and emits [`UnresolvedRef`]s — raw references by
//! *name*, not yet bound to a target [`crate::SymbolId`]. The resolver pass runs once the whole
//! project's symbols are known and turns refs into [`Edge`]s. Decoupling the two phases means
//! resolution can be swapped without re-parsing (Wave 1.1) and avoids file-ordering bugs
//! (the `unresolved_refs` pattern; see the design notes).

use crate::edge::{Edge, EdgeKind};
use crate::node::{Location, Metadata, Node};
use crate::symbol::SymbolId;
use serde::{Deserialize, Serialize};

/// `raw_name` prefix of a COBOL dynamic call whose program-name variable holds exactly one literal
/// in its own source (`01 WS-PGM … VALUE 'PRICEPGM'` then `CALL WS-PGM`): the ref is
/// `cobol-dynamic-call:PRICEPGM`. The prefix keeps the name-based resolvers from binding it at their
/// `ImportMap` confidence. Only the dedicated dynamic-call resolver binds it, at `Heuristic`, and the
/// prefix survives in the parked `unresolved_refs` row, so a back-fill re-binds it the same way.
pub const COBOL_DYNAMIC_CALL_PREFIX: &str = "cobol-dynamic-call:";

/// The persisted form of an APEXlang `@reference` (estate#313).
///
/// `SqliteStore` keeps a parked ref's `raw_name`, `kind` and site, never its hints, so everything
/// the resolver needs travels in the name: `apexlang-ref:<target kind>:<slug>:<scope>`.
/// - `<target kind>` is the component kind the property refers to (from the extractor's data
///   table), [`apexlang_ref::ANY_KIND`] when the property is not in the table (the slug must then
///   name exactly one component in the application), or [`apexlang_ref::NO_BIND`] when the ref must
///   never bind (a theme-relative `@/x` outside the table, or a scope that does not exist).
/// - `<slug>` is percent-encoded with [`apexlang_ref::encode`], so it never contains `:`.
/// - `<scope>` is the `SymbolId` string the target must sit strictly under (the application or the
///   enclosing page/region), kept verbatim as the last field.
///
/// The prefix keeps the name-based resolvers away (no node is named `apexlang-ref:…`); only the
/// APEXlang resolver binds these, and a back-fill can probe them exactly from a new node.
pub mod apexlang_ref {
    /// `raw_name` prefix of every APEXlang reference.
    pub const PREFIX: &str = "apexlang-ref:";
    /// Target kind of a reference whose property is not in the data table.
    pub const ANY_KIND: &str = "*";
    /// Target kind of a reference that must stay unresolved.
    pub const NO_BIND: &str = "-";
    /// Prefix on a target kind found by a text scan rather than a grammar (an APEXlang bind
    /// variable in an unparsed PL/SQL block): `~pageItem` binds like `pageItem`, at `Heuristic`.
    pub const HEURISTIC: &str = "~";
    /// `raw_name` prefix of a top-level component's link to the application or container
    /// directory that holds it: `apexlang-in:<holder SymbolId>`. Bound to a `Contains` edge from
    /// the holder once the holder exists, so a holder declared in another file never dangles.
    pub const CONTAINER_PREFIX: &str = "apexlang-in:";

    /// The `raw_name` linking a component to its holder.
    pub fn container(holder: &str) -> String {
        format!("{CONTAINER_PREFIX}{holder}")
    }

    /// The holder `SymbolId` of a name written by [`container`].
    pub fn parse_container(raw_name: &str) -> Option<&str> {
        raw_name.strip_prefix(CONTAINER_PREFIX)
    }

    /// A decoded APEXlang reference name.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct ApexlangRef {
        pub kind: String,
        pub slug: String,
        pub scope: String,
    }

    /// Percent-encode every byte outside `[A-Za-z0-9_.$'-]`. Injective, and the output never
    /// contains `:`, `#`, `/`, `~` or whitespace, so it is safe as a descriptor or a name field.
    pub fn encode(raw: &str) -> String {
        let mut out = String::with_capacity(raw.len());
        for b in raw.bytes() {
            if b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'$' | b'\'' | b'-') {
                out.push(b as char);
            } else {
                out.push_str(&format!("%{b:02X}"));
            }
        }
        out
    }

    /// Inverse of [`encode`]; `None` on a malformed escape or non-UTF-8 result.
    pub fn decode(enc: &str) -> Option<String> {
        let bytes = enc.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' {
                let hex = enc.get(i + 1..i + 3)?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 3;
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        }
        String::from_utf8(out).ok()
    }

    /// The `raw_name` for a reference to `slug` of `kind` under `scope`.
    pub fn format(kind: &str, slug: &str, scope: &str) -> String {
        format!("{PREFIX}{}:{}:{scope}", encode(kind), encode(slug))
    }

    /// Parse a `raw_name` written by [`format`]; `None` for any other name.
    pub fn parse(raw_name: &str) -> Option<ApexlangRef> {
        let rest = raw_name.strip_prefix(PREFIX)?;
        let (kind, rest) = rest.split_once(':')?;
        let (slug, scope) = rest.split_once(':')?;
        Some(ApexlangRef {
            kind: decode(kind)?,
            slug: decode(slug)?,
            scope: scope.to_string(),
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn round_trips_names_with_separators() {
            let scope = "apexlang . . . apps/a b/app#page:7#";
            for slug in [
                "contribution-rights",
                "region/accent",
                "a:b",
                "%41",
                "x y",
                "ü",
            ] {
                let raw = format("region", slug, scope);
                let back = parse(&raw).expect("parses");
                assert_eq!(back.slug, slug);
                assert_eq!(back.kind, "region");
                assert_eq!(back.scope, scope);
            }
            assert_eq!(parse("contribution-rights"), None);
        }

        #[test]
        fn encode_is_injective_on_separator_lookalikes() {
            assert_ne!(encode("a/b"), encode("a%2Fb"));
            assert!(!encode("a:b#c/d~e f").contains([':', '#', '/', '~', ' ']));
        }
    }
}

/// A reference captured during extraction but not yet resolved to a target symbol.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnresolvedRef {
    /// The symbol making the reference (the dependent → future edge `source`).
    pub from: SymbolId,
    /// The name as written at the reference site (may be unqualified).
    pub raw_name: String,
    /// The edge kind this reference becomes once resolved.
    pub kind: EdgeKind,
    pub location: Location,
    /// Resolution hints: imports in scope, receiver-type guesses, arity, etc.
    #[serde(default)]
    pub hints: Metadata,
}

impl UnresolvedRef {
    pub fn new(
        from: SymbolId,
        raw_name: impl Into<String>,
        kind: EdgeKind,
        location: Location,
    ) -> Self {
        Self {
            from,
            raw_name: raw_name.into(),
            kind,
            location,
            hints: Metadata::new(),
        }
    }
}

/// The output of an [`crate::Extractor`] for one file: nodes, intra-file edges known at parse
/// time (e.g. `contains`, `defines`), and unresolved refs for the resolver pass.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Extraction {
    pub nodes: Vec<Node>,
    pub local_edges: Vec<Edge>,
    pub refs: Vec<UnresolvedRef>,
}
