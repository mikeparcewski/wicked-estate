//! Task C — Tests for W11.1 (content store) and W11.2 (versioned query cache).
//! Covers both `MemStore` and `SqliteStore` to prove the inherent methods behave
//! identically on both backends.

use wicked_estate_core::{GraphRead, GraphWrite, Language, Location, Node, NodeKind, Span, SymbolId};
use wicked_estate_store::{MemStore, SqliteStore};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_node(file: &str, start_byte: u32, end_byte: u32) -> Node {
    let span = Span {
        start_byte,
        end_byte,
        start_line: 0,
        start_col: 0,
        end_line: 0,
        end_col: 0,
    };
    Node::new(
        SymbolId("test::sym".into()),
        NodeKind::Function,
        "sym",
        Language::new("rust"),
        Location::new(file, span),
    )
}

fn make_zero_span_node(file: &str) -> Node {
    make_node(file, 0, 0)
}

// ---------------------------------------------------------------------------
// W11.1 — MemStore content store
// ---------------------------------------------------------------------------

#[test]
fn mem_set_and_get_file_content() {
    let mut store = MemStore::new();
    assert_eq!(store.file_content("src/foo.rs").unwrap(), None);
    store
        .set_file_content("src/foo.rs", "fn main() {}")
        .unwrap();
    assert_eq!(
        store.file_content("src/foo.rs").unwrap(),
        Some("fn main() {}".to_string())
    );
}

#[test]
fn mem_symbol_source_exact_slice() {
    let src = "fn main() { let x = 42; }";
    let mut store = MemStore::new();
    store.set_file_content("src/foo.rs", src).unwrap();

    // "let x = 42;" is at bytes 12..23 in the string above.
    let node = make_node("src/foo.rs", 12, 23);
    let slice = store.symbol_source(&node).unwrap();
    assert_eq!(slice, Some("let x = 42;".to_string()));
}

#[test]
fn mem_symbol_source_out_of_range_returns_none() {
    let src = "hello";
    let mut store = MemStore::new();
    store.set_file_content("src/bar.rs", src).unwrap();

    // end_byte beyond text length → None
    let node = make_node("src/bar.rs", 0, 100);
    assert_eq!(store.symbol_source(&node).unwrap(), None);
}

#[test]
fn mem_symbol_source_zero_span_returns_none() {
    let mut store = MemStore::new();
    store.set_file_content("src/baz.rs", "fn x() {}").unwrap();

    let node = make_zero_span_node("src/baz.rs");
    assert_eq!(store.symbol_source(&node).unwrap(), None);
}

#[test]
fn mem_symbol_source_missing_content_returns_none() {
    let store = MemStore::new();
    let node = make_node("src/missing.rs", 0, 5);
    assert_eq!(store.symbol_source(&node).unwrap(), None);
}

// ---------------------------------------------------------------------------
// W11.2 — MemStore versioned cache
// ---------------------------------------------------------------------------

#[test]
fn mem_cache_put_then_get_returns_value() {
    let mut store = MemStore::new();
    store.cache_put("blast:main", "result1").unwrap();
    assert_eq!(
        store.cache_get("blast:main").unwrap(),
        Some("result1".to_string())
    );
}

#[test]
fn mem_cache_after_bump_returns_none() {
    let mut store = MemStore::new();
    store.cache_put("blast:main", "result1").unwrap();
    store.bump_version().unwrap();
    assert_eq!(store.cache_get("blast:main").unwrap(), None);
}

#[test]
fn mem_cache_new_put_after_bump_is_retrievable() {
    let mut store = MemStore::new();
    store.cache_put("k", "old").unwrap();
    store.bump_version().unwrap();
    store.cache_put("k", "new").unwrap();
    assert_eq!(store.cache_get("k").unwrap(), Some("new".to_string()));
}

#[test]
fn mem_cache_missing_key_returns_none() {
    let store = MemStore::new();
    assert_eq!(store.cache_get("nonexistent").unwrap(), None);
}

// ---------------------------------------------------------------------------
// W11.1 — SqliteStore content store
// ---------------------------------------------------------------------------

#[test]
fn sqlite_set_and_get_file_content() {
    let mut store = SqliteStore::in_memory().unwrap();
    assert_eq!(store.file_content("src/foo.rs").unwrap(), None);
    store
        .set_file_content("src/foo.rs", "fn main() {}")
        .unwrap();
    assert_eq!(
        store.file_content("src/foo.rs").unwrap(),
        Some("fn main() {}".to_string())
    );
}

#[test]
fn sqlite_set_file_content_replaces_on_conflict() {
    let mut store = SqliteStore::in_memory().unwrap();
    store.set_file_content("src/foo.rs", "v1").unwrap();
    store.set_file_content("src/foo.rs", "v2").unwrap();
    assert_eq!(
        store.file_content("src/foo.rs").unwrap(),
        Some("v2".to_string())
    );
}

#[test]
fn sqlite_symbol_source_exact_slice() {
    let src = "fn main() { let x = 42; }";
    let mut store = SqliteStore::in_memory().unwrap();
    store.set_file_content("src/foo.rs", src).unwrap();

    // "let x = 42;" is at bytes 12..23 in the string above.
    let node = make_node("src/foo.rs", 12, 23);
    let slice = store.symbol_source(&node).unwrap();
    assert_eq!(slice, Some("let x = 42;".to_string()));
}

#[test]
fn sqlite_symbol_source_out_of_range_returns_none() {
    let src = "hello";
    let mut store = SqliteStore::in_memory().unwrap();
    store.set_file_content("src/bar.rs", src).unwrap();

    let node = make_node("src/bar.rs", 0, 100);
    assert_eq!(store.symbol_source(&node).unwrap(), None);
}

#[test]
fn sqlite_symbol_source_zero_span_returns_none() {
    let mut store = SqliteStore::in_memory().unwrap();
    store.set_file_content("src/baz.rs", "fn x() {}").unwrap();

    let node = make_zero_span_node("src/baz.rs");
    assert_eq!(store.symbol_source(&node).unwrap(), None);
}

#[test]
fn sqlite_symbol_source_missing_content_returns_none() {
    let store = SqliteStore::in_memory().unwrap();
    let node = make_node("src/missing.rs", 0, 5);
    assert_eq!(store.symbol_source(&node).unwrap(), None);
}

// ---------------------------------------------------------------------------
// W11.2 — SqliteStore versioned cache
// ---------------------------------------------------------------------------

#[test]
fn sqlite_cache_put_then_get_returns_value() {
    let mut store = SqliteStore::in_memory().unwrap();
    store.cache_put("blast:main", "result1").unwrap();
    assert_eq!(
        store.cache_get("blast:main").unwrap(),
        Some("result1".to_string())
    );
}

#[test]
fn sqlite_cache_after_bump_returns_none() {
    let mut store = SqliteStore::in_memory().unwrap();
    store.cache_put("blast:main", "result1").unwrap();
    store.bump_version().unwrap();
    assert_eq!(store.cache_get("blast:main").unwrap(), None);
}

#[test]
fn sqlite_cache_new_put_after_bump_is_retrievable() {
    let mut store = SqliteStore::in_memory().unwrap();
    store.cache_put("k", "old").unwrap();
    store.bump_version().unwrap();
    store.cache_put("k", "new").unwrap();
    assert_eq!(store.cache_get("k").unwrap(), Some("new".to_string()));
}

#[test]
fn sqlite_cache_missing_key_returns_none() {
    let store = SqliteStore::in_memory().unwrap();
    assert_eq!(store.cache_get("nonexistent").unwrap(), None);
}

#[test]
fn sqlite_bump_version_multiple_times() {
    let mut store = SqliteStore::in_memory().unwrap();
    store.cache_put("x", "v0").unwrap();
    store.bump_version().unwrap();
    store.bump_version().unwrap();
    assert_eq!(store.cache_get("x").unwrap(), None);
    store.cache_put("x", "v2").unwrap();
    assert_eq!(store.cache_get("x").unwrap(), Some("v2".to_string()));
}
