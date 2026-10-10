//! TS-S2C: the SCIP adapter ([`wicked_estate_resolve::scip_evidence`]) against a REAL
//! scip-typescript 0.4.0 index (`tests/fixtures/scip-typescript-0.4.0/`, see its README) and
//! against hand-built indexes for the cases a real indexer does not produce on demand (typed
//! ranges, malformed ranges, generated and forward occurrences).

use protobuf::Message as _;
use scip::types::{
    Document, Index, Metadata, MultiLineRange, Occurrence, SingleLineRange, SymbolRole, ToolInfo,
};
use wicked_estate_core::evidence::{
    Capability, EvidenceFact, EvidenceSkip, PositionEncoding, ProducerClass,
};
use wicked_estate_resolve::scip_evidence;

const REAL: &[u8] = include_bytes!("fixtures/scip-typescript-0.4.0/index.scip");
const HELPER: &str = "scip-typescript npm scip-ts-sample 0.0.0 src/`util.ts`/helper().";

fn references_to<'a>(facts: &'a [EvidenceFact], symbol: &str) -> Vec<&'a EvidenceFact> {
    facts
        .iter()
        .filter(|f| matches!(f, EvidenceFact::Reference { symbol: s, .. } if s == symbol))
        .collect()
}

#[test]
fn the_real_index_declares_no_calls_and_keeps_the_producer() {
    let t = scip_evidence(REAL, ".:index.scip").unwrap();
    let p = &t.evidence.producer;
    assert_eq!(
        (p.name.as_str(), p.version.as_str()),
        ("scip-typescript", "0.4.0")
    );
    assert_eq!(p.class, ProducerClass::Index);
    assert_eq!(
        p.capabilities.iter().copied().collect::<Vec<_>>(),
        vec![Capability::Definitions, Capability::References],
        "SCIP has no call role: the adapter never vouches for calls"
    );
    assert!(
        t.evidence
            .facts
            .iter()
            .all(|f| !matches!(f, EvidenceFact::Call { .. }))
    );
    t.evidence.validate().unwrap();
}

/// `const f = helper;` (5:12) and `helper(LIMIT)` (6:9) are the same role-less occurrence shape.
/// Both are references; neither is a call.
#[test]
fn a_value_reference_and_a_call_are_both_references() {
    let t = scip_evidence(REAL, ".:index.scip").unwrap();
    let mut sites: Vec<(String, u32, u32)> = references_to(&t.evidence.facts, HELPER)
        .into_iter()
        .map(|f| {
            let s = f.site();
            (s.document.clone(), s.range.start_line, s.range.start_col)
        })
        .collect();
    sites.sort();
    assert_eq!(
        sites,
        vec![
            ("src/main.ts".into(), 0, 9),
            ("src/main.ts".into(), 5, 12),
            ("src/main.ts".into(), 6, 9),
        ]
    );
}

#[test]
fn definitions_are_named_and_locals_are_document_scoped() {
    let t = scip_evidence(REAL, ".:index.scip").unwrap();
    let defs: Vec<(&str, &str)> = t
        .evidence
        .facts
        .iter()
        .filter_map(|f| match f {
            EvidenceFact::Definition { symbol, name, .. } => Some((name.as_str(), symbol.as_str())),
            _ => None,
        })
        .collect();
    for want in ["helper", "LIMIT", "Greeter", "greet", "run", "x", "name"] {
        assert!(defs.iter().any(|(n, _)| *n == want), "{want} in {defs:?}");
    }
    // `local 2` / `local 5` carry no display name in this index, so they are not definitions,
    // and their references are qualified by the document: never a bare `local 2`.
    assert!(defs.iter().all(|(_, s)| !s.starts_with("local ")));
    let locals: Vec<&str> = t
        .evidence
        .facts
        .iter()
        .filter_map(|f| match f {
            EvidenceFact::Reference { symbol, .. } if symbol.contains("local") => {
                Some(symbol.as_str())
            }
            _ => None,
        })
        .collect();
    assert!(!locals.is_empty());
    assert!(
        locals.iter().all(|s| s.contains("src/main.ts")),
        "{locals:?}"
    );
    assert_eq!(t.skipped.get(&EvidenceSkip::UnmappedDefinition), Some(&2));
    // The two module definitions and the `"./util"` module reference.
    assert_eq!(t.skipped.get(&EvidenceSkip::ModuleSymbol), Some(&3));
}

#[test]
fn the_translation_is_deterministic() {
    let a = scip_evidence(REAL, ".:index.scip").unwrap().evidence;
    let b = scip_evidence(REAL, ".:index.scip").unwrap().evidence;
    assert_eq!(a, b);
    assert_eq!(a.snapshot, ".:index.scip");
}

fn occurrence(symbol: &str, roles: i32) -> Occurrence {
    Occurrence {
        symbol: symbol.into(),
        symbol_roles: roles,
        ..Occurrence::default()
    }
}

fn index(tool: &str, encoding: scip::types::PositionEncoding, occs: Vec<Occurrence>) -> Vec<u8> {
    let mut idx = Index::new();
    let mut meta = Metadata::new();
    let mut info = ToolInfo::new();
    info.name = tool.into();
    meta.tool_info = Some(info).into();
    idx.metadata = Some(meta).into();
    let mut doc = Document::new();
    doc.relative_path = "./pkg/a.java".into();
    doc.position_encoding = encoding.into();
    doc.occurrences = occs;
    idx.documents.push(doc);
    idx.write_to_bytes().unwrap()
}

#[test]
fn typed_ranges_win_and_malformed_ranges_are_counted_not_clamped() {
    let sym = "semanticdb maven . . pkg/A#run().";
    let mut typed = occurrence(sym, 0);
    typed.set_single_line_range(SingleLineRange {
        line: 3,
        start_character: 4,
        end_character: 7,
        ..Default::default()
    });
    typed.range = vec![9, 9, 9]; // the deprecated form loses to the typed one
    let mut multi = occurrence(sym, 0);
    multi.set_multi_line_range(MultiLineRange {
        start_line: 5,
        start_character: 0,
        end_line: 6,
        end_character: 2,
        ..Default::default()
    });
    let mut legacy = occurrence(sym, SymbolRole::Definition as i32);
    legacy.range = vec![1, 10, 13];
    let mut negative = occurrence(sym, 0);
    negative.range = vec![-1, 0, 2];
    let mut arity = occurrence(sym, 0);
    arity.range = vec![1, 2];
    let mut backwards = occurrence(sym, 0);
    backwards.range = vec![4, 5, 3, 0];
    let bytes = index(
        "scip-java",
        scip::types::PositionEncoding::UTF16CodeUnitOffsetFromLineStart,
        vec![typed, multi, legacy, negative, arity, backwards],
    );
    let t = scip_evidence(&bytes, "r:index.scip").unwrap();
    assert_eq!(t.evidence.producer.name, "scip-java");
    assert_eq!(t.evidence.producer.version, "unknown");
    assert_eq!(t.evidence.documents.len(), 1);
    assert_eq!(t.evidence.documents[0].path, "pkg/a.java", "./ is dropped");
    assert_eq!(
        t.evidence.documents[0].position_encoding,
        PositionEncoding::Utf16
    );
    let mut ranges: Vec<(u32, u32, u32, u32)> = t
        .evidence
        .facts
        .iter()
        .map(|f| {
            let r = f.site().range;
            (r.start_line, r.start_col, r.end_line, r.end_col)
        })
        .collect();
    ranges.sort();
    assert_eq!(ranges, vec![(1, 10, 1, 13), (3, 4, 3, 7), (5, 0, 6, 2)]);
    assert_eq!(t.skipped.get(&EvidenceSkip::MalformedRange), Some(&3));
}

#[test]
fn generated_and_forward_occurrences_are_dropped_and_counted() {
    let sym = "scip-dotnet nuget . . App/Thing#Do().";
    let with = |roles: i32| {
        let mut o = occurrence(sym, roles);
        o.range = vec![2, 0, 2];
        o
    };
    let bytes = index(
        "",
        scip::types::PositionEncoding::UnspecifiedPositionEncoding,
        vec![
            with(SymbolRole::Generated as i32),
            with(SymbolRole::Definition as i32 | SymbolRole::Generated as i32),
            with(SymbolRole::ForwardDefinition as i32),
            with(SymbolRole::ReadAccess as i32 | SymbolRole::WriteAccess as i32),
            occurrence("", 0),
        ],
    );
    let t = scip_evidence(&bytes, "r:index.scip").unwrap();
    assert_eq!(
        t.evidence.producer.name, "scip",
        "an empty tool name falls back"
    );
    assert_eq!(t.skipped.get(&EvidenceSkip::Generated), Some(&2));
    assert_eq!(t.skipped.get(&EvidenceSkip::ForwardDefinition), Some(&1));
    assert_eq!(t.skipped.get(&EvidenceSkip::UnknownTarget), Some(&1));
    assert_eq!(t.evidence.facts.len(), 1);
    let EvidenceFact::Reference { roles, .. } = &t.evidence.facts[0] else {
        panic!("{:?}", t.evidence.facts);
    };
    assert_eq!(
        serde_json::to_value(roles).unwrap(),
        serde_json::json!(["read", "write"])
    );
}

#[test]
fn a_document_path_the_envelope_cannot_carry_is_dropped_whole() {
    let mut idx = Index::new();
    for path in ["../escape.ts", "C:\\win\\a.ts", "/abs.ts"] {
        let mut doc = Document::new();
        doc.relative_path = path.into();
        let mut o = occurrence("scip-typescript npm p 1 a/f().", 0);
        o.range = vec![0, 0, 1];
        doc.occurrences.push(o);
        idx.documents.push(doc);
    }
    let t = scip_evidence(&idx.write_to_bytes().unwrap(), "r:i").unwrap();
    assert!(t.evidence.documents.is_empty());
    assert!(t.evidence.facts.is_empty());
    assert_eq!(t.skipped.get(&EvidenceSkip::DocumentNotInGraph), Some(&3));
    t.evidence.validate().unwrap();
}

#[test]
fn garbage_bytes_are_a_decode_error() {
    assert!(scip_evidence(b"\xff\xff\xff not protobuf", "r:i").is_err());
}
