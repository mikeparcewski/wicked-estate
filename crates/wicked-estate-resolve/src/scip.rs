//! SCIP adapter (TS-S2C): a SCIP `index.scip` payload → one [`SemanticEvidence`] envelope.
//!
//! The adapter only *translates*; correlation and projection are the engine's
//! ([`wicked_estate_core::evidence::project_evidence`]), so every producer is held to one rule.
//!
//! * **Producer profile.** `name` = `metadata.tool_info.name` (`scip` when empty), `version` =
//!   `tool_info.version`, class `index`, capabilities `definitions` + `references` — **never
//!   `calls`**: SCIP's `SymbolRole` has no call role, so a role-less occurrence of a function is a
//!   reference, whether the source wrote `f()` or `const g = f`.
//! * **Ranges.** The typed `single_line_range` / `multi_line_range` (SCIP 0.9+) win over the
//!   deprecated `repeated int32 range`, as the schema says. A malformed range (wrong arity,
//!   negative, ending before it starts) is counted ([`EvidenceSkip::MalformedRange`]), never
//!   clamped to a zero span.
//! * **Roles.** `Generated` occurrences and bare `ForwardDefinition`s are counted and dropped;
//!   `Definition` becomes a definition fact named after the symbol's last descriptor (or the
//!   document's `display_name` for a local); anything else is a reference carrying its
//!   `Import` / `ReadAccess` / `WriteAccess` / `Test` roles. Module symbols (a trailing `/`) are
//!   counted and dropped: the base plane's `Imports` already link files.
//! * **Identity.** `local N` symbols are document-scoped in SCIP, so they are qualified by their
//!   document; every other symbol string is kept byte for byte. A fact id is an injective tuple of
//!   (fact kind, document, symbol, range, roles), so re-emitting the same index yields the same ids.

use std::collections::{BTreeMap, BTreeSet};

use protobuf::Message as _;
use scip::types::{Index, Occurrence, PositionEncoding as ScipEncoding, SymbolRole, occurrence};
use wicked_estate_core::evidence::{
    Capability, EvidenceDocument, EvidenceFact, EvidenceRange, EvidenceSite, EvidenceSkip,
    PositionEncoding, ProducerClass, ProducerProfile, ReferenceRole,
    SEMANTIC_EVIDENCE_SCHEMA_VERSION, SemanticEvidence, is_document_path, opaque_tuple_id,
};
use wicked_estate_core::{Error, Result};

/// The envelope one SCIP index translates to, and what the adapter itself had to drop.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ScipEvidence {
    pub evidence: SemanticEvidence,
    pub skipped: BTreeMap<EvidenceSkip, usize>,
}

fn range_of(occ: &Occurrence) -> Option<EvidenceRange> {
    let (sl, sc, el, ec) = match &occ.typed_range {
        Some(occurrence::Typed_range::SingleLineRange(r)) => {
            (r.line, r.start_character, r.line, r.end_character)
        }
        Some(occurrence::Typed_range::MultiLineRange(r)) => {
            (r.start_line, r.start_character, r.end_line, r.end_character)
        }
        None => match *occ.range.as_slice() {
            [sl, sc, ec] => (sl, sc, sl, ec),
            [sl, sc, el, ec] => (sl, sc, el, ec),
            _ => return None,
        },
        // The generated oneof is `#[non_exhaustive]`: a range form a later schema adds is
        // unreadable here, and is counted as malformed rather than guessed at.
        Some(_) => return None,
    };
    let [sl, sc, el, ec] = [sl, sc, el, ec].map(u32::try_from);
    let r = EvidenceRange {
        start_line: sl.ok()?,
        start_col: sc.ok()?,
        end_line: el.ok()?,
        end_col: ec.ok()?,
    };
    ((r.start_line, r.start_col) <= (r.end_line, r.end_col)).then_some(r)
}

fn encoding_of(e: ScipEncoding) -> PositionEncoding {
    match e {
        ScipEncoding::UTF8CodeUnitOffsetFromLineStart => PositionEncoding::Utf8,
        ScipEncoding::UTF16CodeUnitOffsetFromLineStart => PositionEncoding::Utf16,
        ScipEncoding::UTF32CodeUnitOffsetFromLineStart => PositionEncoding::Utf32,
        ScipEncoding::UnspecifiedPositionEncoding => PositionEncoding::Unspecified,
    }
}

/// A SCIP `relative_path` as the envelope's repository-relative form (`./` prefixes dropped). A
/// path the envelope cannot carry (absolute, `..`, Windows separators) drops its document's
/// occurrences, counted as [`EvidenceSkip::DocumentNotInGraph`] — never guessed at.
fn document_path(raw: &str) -> String {
    let mut p = raw;
    while let Some(rest) = p.strip_prefix("./") {
        p = rest;
    }
    p.to_string()
}

fn has(roles: i32, role: SymbolRole) -> bool {
    roles & (role as i32) != 0
}

/// The source spelling of a global SCIP symbol: its last descriptor's name.
fn global_name(symbol: &str) -> Option<String> {
    let parsed = scip::symbol::parse_symbol(symbol).ok()?;
    let name = parsed.descriptors.last()?.name.clone();
    (!name.is_empty()).then_some(name)
}

/// Translate one SCIP index. `snapshot` is the support owner's snapshot (the stable unit, e.g.
/// `<repo label>:<index path>`), not derived from the index contents.
pub fn scip_evidence(index_bytes: &[u8], snapshot: &str) -> Result<ScipEvidence> {
    let index = Index::parse_from_bytes(index_bytes)
        .map_err(|e| Error::Resolution(format!("scip: protobuf decode error: {e}")))?;
    let tool = index.metadata.tool_info.clone().unwrap_or_default();
    let name = if tool.name.is_empty() {
        "scip".to_string()
    } else {
        tool.name.clone()
    };
    let version = if tool.version.is_empty() {
        "unknown".to_string()
    } else {
        tool.version.clone()
    };

    let mut skipped: BTreeMap<EvidenceSkip, usize> = BTreeMap::new();
    let mut skip = |r: EvidenceSkip| *skipped.entry(r).or_default() += 1;
    let mut documents: BTreeMap<String, PositionEncoding> = BTreeMap::new();
    let mut facts: BTreeSet<EvidenceFact> = BTreeSet::new();

    for doc in &index.documents {
        let path = document_path(&doc.relative_path);
        let encoding = encoding_of(
            doc.position_encoding
                .enum_value()
                .unwrap_or(ScipEncoding::UnspecifiedPositionEncoding),
        );
        if !is_document_path(&path) {
            for _ in &doc.occurrences {
                skip(EvidenceSkip::DocumentNotInGraph);
            }
            continue;
        }
        documents.entry(path.clone()).or_insert(encoding);
        let local_names: BTreeMap<&str, &str> = doc
            .symbols
            .iter()
            .filter(|s| s.symbol.starts_with("local ") && !s.display_name.is_empty())
            .map(|s| (s.symbol.as_str(), s.display_name.as_str()))
            .collect();

        for occ in &doc.occurrences {
            let raw = occ.symbol.as_str();
            if raw.is_empty() {
                skip(EvidenceSkip::UnknownTarget);
                continue;
            }
            if raw.ends_with('/') {
                skip(EvidenceSkip::ModuleSymbol);
                continue;
            }
            let Some(range) = range_of(occ) else {
                skip(EvidenceSkip::MalformedRange);
                continue;
            };
            if has(occ.symbol_roles, SymbolRole::Generated) {
                skip(EvidenceSkip::Generated);
                continue;
            }
            let local = raw.starts_with("local ");
            let symbol = if local {
                opaque_tuple_id(&["local", &path, raw])
            } else {
                raw.to_string()
            };
            let site = EvidenceSite {
                document: path.clone(),
                range,
            };
            let range_key = format!(
                "{}:{}-{}:{}",
                range.start_line, range.start_col, range.end_line, range.end_col
            );
            if has(occ.symbol_roles, SymbolRole::Definition) {
                let name = if local {
                    local_names.get(raw).map(|s| s.to_string())
                } else {
                    global_name(raw)
                };
                let Some(name) = name else {
                    skip(EvidenceSkip::UnmappedDefinition);
                    continue;
                };
                facts.insert(EvidenceFact::Definition {
                    fact_id: opaque_tuple_id(&["definition", &path, &symbol, &range_key]),
                    symbol,
                    name,
                    site,
                });
                continue;
            }
            if has(occ.symbol_roles, SymbolRole::ForwardDefinition) {
                skip(EvidenceSkip::ForwardDefinition);
                continue;
            }
            let mut roles = Vec::new();
            for (role, word) in [
                (SymbolRole::Import, ReferenceRole::Import),
                (SymbolRole::ReadAccess, ReferenceRole::Read),
                (SymbolRole::WriteAccess, ReferenceRole::Write),
                (SymbolRole::Test, ReferenceRole::Test),
            ] {
                if has(occ.symbol_roles, role) {
                    roles.push(word);
                }
            }
            let roles_key = occ.symbol_roles.to_string();
            facts.insert(EvidenceFact::Reference {
                fact_id: opaque_tuple_id(&["reference", &path, &symbol, &range_key, &roles_key]),
                symbol,
                site,
                roles,
            });
        }
    }

    let evidence = SemanticEvidence {
        schema_version: SEMANTIC_EVIDENCE_SCHEMA_VERSION,
        producer: ProducerProfile {
            name,
            version,
            class: ProducerClass::Index,
            capabilities: [Capability::Definitions, Capability::References]
                .into_iter()
                .collect(),
        },
        snapshot: snapshot.to_string(),
        generation: None,
        documents: documents
            .into_iter()
            .map(|(path, position_encoding)| EvidenceDocument {
                path,
                position_encoding,
            })
            .collect(),
        facts: facts.into_iter().collect(),
    };
    Ok(ScipEvidence { evidence, skipped })
}
