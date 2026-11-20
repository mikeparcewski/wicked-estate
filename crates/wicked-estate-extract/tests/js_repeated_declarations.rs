//! estate#505: repeated declarations and calls must not multiply mutable-scope scans.

use std::time::{Duration, Instant};
use wicked_estate_core::{EdgeKind, Extractor, Language, SourceFile};
use wicked_estate_extract::TreeSitterExtractor;

/// The call sits in a bare block inside the function: the block-found path (`js_mutable_scope`)
/// that rescanned every declaration per call. A function body takes the hoisted-`var` path.
#[test]
fn repeated_var_declarations_in_a_block_share_mutable_scope_lookup() {
    repeated("function Page() {{\n{{\n{}{}}}\n}}\n");
}

#[test]
fn repeated_var_declarations_in_a_function_body_stay_linear() {
    repeated("function Page() {{\n{}{}}}\n");
}

fn repeated(shape: &str) {
    const N: usize = 3_000;
    const DECL: &str = "var Card;\n";
    const CALL: &str = "Card();\n";

    let extractor = TreeSitterExtractor::for_language("javascript").unwrap();
    let file = |n| SourceFile {
        path: "src/repeated.js".into(),
        language: Language::new("javascript"),
        text: shape
            .replacen("{}", &DECL.repeat(n), 1)
            .replacen("{}", &CALL.repeat(n), 1)
            .replace("{{", "{")
            .replace("}}", "}"),
    };
    let small = extractor.extract(&file(1)).unwrap();
    assert_eq!(small.refs.len(), 1);
    let template = &small.refs[0];
    assert_eq!(template.kind, EdgeKind::Calls);
    assert_eq!(template.raw_name, "local-binding:Card");
    assert_eq!(
        template.from,
        small
            .nodes
            .iter()
            .find(|n| n.name == "Page")
            .unwrap()
            .symbol
    );

    let large_file = file(N);
    let start = Instant::now();
    let mut large = extractor.extract(&large_file).unwrap();
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(2),
        "{N} repeated declarations and calls took {elapsed:?}; mutable-scope lookup must be shared"
    );

    // Every ref must match the small equivalent, including caller, parked name, kind and hints.
    // Only its site moves past the extra declarations and preceding calls.
    let expected: Vec<_> = (0..N)
        .map(|i| {
            let mut reference = template.clone();
            let bytes = ((N - 1) * DECL.len() + i * CALL.len()) as u32;
            let lines = (N - 1 + i) as u32;
            reference.location.span.start_byte += bytes;
            reference.location.span.end_byte += bytes;
            reference.location.span.start_line += lines;
            reference.location.span.end_line += lines;
            reference
        })
        .collect();
    large
        .refs
        .sort_unstable_by_key(|r| r.location.span.start_byte);
    assert_eq!(large.refs, expected);
}
