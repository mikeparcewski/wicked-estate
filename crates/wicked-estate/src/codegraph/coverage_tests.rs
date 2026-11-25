//! estate#595: the gate fixture (`tests/fixtures/codegraph/coverage`) is real codegraph 1.6.4
//! output for one file per language estate hands over. It holds every edge kind that release
//! emits for them, a `size_exceeded` file and a stylesheet import. These tests pin that claim, so
//! a regenerated fixture that loses a shape fails here, and they check that the import writes each
//! shape into the store.

use super::*;
use std::collections::BTreeSet;
use wicked_estate_core::GraphRead;
use wicked_estate_store::SqliteStore;

fn coverage_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codegraph/coverage")
}

fn coverage_sql() -> String {
    let path = coverage_dir().join("codegraph-1.6.4.sql");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("the gate fixture {}: {e}", path.display()))
}

/// `src/ts/big.ts`, past codegraph's 1 MiB source cap: the bytes
/// `scripts/codegraph-fixture-store.py grow` writes for regen.sh and the gate store. The recorded
/// files row's size pins the two to each other.
pub(crate) fn big_file() -> String {
    let line = "// codegraph caps a source file at 1 MiB; this one is bigger.\n";
    let n = (1024 * 1024) / line.len() + 1;
    format!("export const big = 0;\n{}", line.repeat(n))
}

/// A copy of the gate fixture's repo with `big.ts` written in, as regen.sh records it.
pub(crate) fn coverage_copy(tmp: &Path) -> PathBuf {
    fn copy(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap() {
            let e = e.unwrap();
            if e.file_type().unwrap().is_dir() {
                copy(&e.path(), &to.join(e.file_name()));
            } else {
                std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
            }
        }
    }
    let root = tmp.join("repo");
    copy(&coverage_dir().join("repo"), &root);
    std::fs::write(root.join("src/ts/big.ts"), big_file()).unwrap();
    root
}

fn rows(conn: &rusqlite::Connection, sql: &str) -> BTreeSet<String> {
    let mut stmt = conn.prepare(sql).unwrap();
    stmt.query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

/// The edge kinds codegraph 1.6.4 emits for the languages estate hands over, all in the fixture.
const EMITTED: &[&str] = &[
    "calls",
    "contains",
    "decorates",
    "extends",
    "implements",
    "imports",
    "instantiates",
    "navigates",
    "references",
];

/// Kinds estate maps but codegraph 1.6.4 never emits for those languages. Its JS layer has no
/// emitter for them, and none of the ten bake-off corpora (#584) held one. A regenerated fixture
/// that has one moves it to `EMITTED`, with a store assertion below.
const NOT_EMITTED: &[&str] = &["exports", "overrides", "returns", "type_of"];

#[test]
fn the_gate_fixture_records_every_language_and_edge_kind_codegraph_emits_595() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(&coverage_sql()).unwrap();

    let languages = rows(&conn, "SELECT DISTINCT language FROM files");
    // codegraph names `.tsx` files apart from `typescript`; the language switch does not.
    let want: BTreeSet<String> = DEFAULT_LANGUAGES
        .iter()
        .chain(&["tsx"])
        .map(|l| l.to_string())
        .collect();
    assert_eq!(languages, want, "one file per language estate hands over");

    let kinds = rows(&conn, "SELECT DISTINCT kind FROM edges");
    let want: BTreeSet<String> = EMITTED.iter().map(|k| k.to_string()).collect();
    assert_eq!(kinds, want, "every kind codegraph emits");
    for k in NOT_EMITTED {
        assert!(
            map_edge_kind(k).is_some(),
            "{k} is a kind estate maps, listed as not emitted"
        );
    }

    // VB's Calls-to-Field: an indexed array read `items(0)` is a call site.
    let vb_field_calls = rows(
        &conn,
        "SELECT s.name || '>' || t.name FROM edges e JOIN nodes s ON s.id = e.source \
         JOIN nodes t ON t.id = e.target \
         WHERE e.kind = 'calls' AND t.kind = 'field' AND s.language = 'vbnet'",
    );
    assert!(
        vb_field_calls.contains("Current>items"),
        "{vb_field_calls:?}"
    );

    // A real size_exceeded row, for exactly the bytes the tests write.
    let big = big_file();
    // codegraph hashes a stamp for an oversized file, not its bytes, so the size is what pins
    // regen.sh's file to the one the tests write.
    let (size, errors): (i64, String) = conn
        .query_row(
            "SELECT size, errors FROM files WHERE path = 'src/ts/big.ts'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!(errors.contains("\"size_exceeded\""), "{errors}");
    assert_eq!(size as usize, big.len(), "regen.sh and big_file() agree");

    // The stylesheet import, which codegraph leaves unresolved.
    let css = rows(
        &conn,
        "SELECT reference_name FROM unresolved_refs WHERE reference_kind = 'imports'",
    );
    assert!(css.contains("./theme.css"), "{css:?}");
}

fn file_of(e: &Edge) -> &str {
    e.location.as_ref().map_or("", |l| l.file.as_str())
}

fn index_coverage(tmp: &Path) -> (SqliteStore, usize) {
    let root = coverage_copy(tmp);
    let mut store = SqliteStore::open(tmp.join("g.db")).unwrap();
    let cfg = CodegraphConfig {
        bundle: tmp.join("no-bundle"),
        state_dir: tmp.join("cg"),
        languages: Vec::new(),
        timeout: Duration::from_secs(5),
    };
    TEST_DATABASE_SQL.with(|t| *t.borrow_mut() = Some(coverage_sql()));
    crate::index_path_with(&mut store, &root, None, Some(&cfg)).unwrap();
    TEST_DATABASE_SQL.with(|t| *t.borrow_mut() = None);
    let dropped = LAST_DROPPED.with(|c| c.get()).expect("the run counted");
    (store, dropped)
}

#[test]
fn the_gate_store_holds_each_shape_the_import_writes_595() {
    let tmp = tempfile::tempdir().unwrap();
    let (store, dropped) = index_coverage(tmp.path());
    let nodes: HashMap<String, Node> = store
        .all_nodes()
        .unwrap()
        .into_iter()
        .map(|n| (n.symbol.0.clone(), n))
        .collect();
    let edges = store.all_edges().unwrap();
    let bound: Vec<&Edge> = edges
        .iter()
        .filter(|e| e.resolved_by.starts_with("codegraph:"))
        .collect();

    // Every kind codegraph emits lands, except `contains` (estate's own tree) and `navigates`
    // (its target is a route node, which the import leaves out: counted as not imported).
    let kinds: BTreeSet<String> = bound.iter().map(|e| format!("{:?}", e.kind)).collect();
    for k in [
        EdgeKind::Calls,
        EdgeKind::Imports,
        EdgeKind::Extends,
        EdgeKind::Implements,
        EdgeKind::References,
        EdgeKind::Instantiates,
        EdgeKind::Other("decorates".into()),
    ] {
        assert!(kinds.contains(&format!("{k:?}")), "{k:?} in {kinds:?}");
    }
    assert!(
        !edges
            .iter()
            .any(|e| e.kind == EdgeKind::Other("navigates".into())),
        "navigates points at a route node, which is not imported"
    );
    assert!(dropped >= 1, "the navigates fact counts as not imported");

    // Each language's file gets codegraph-bound edges.
    let mut by_ext: BTreeSet<String> = BTreeSet::new();
    for e in &bound {
        if let Some(ext) = Path::new(file_of(e)).extension() {
            by_ext.insert(ext.to_string_lossy().into_owned());
        }
    }
    for ext in [
        "ts", "tsx", "js", "java", "kt", "cs", "scala", "swift", "vb", "c", "cpp", "php",
    ] {
        assert!(
            by_ext.contains(ext),
            "a codegraph edge in a .{ext} file: {by_ext:?}"
        );
    }

    // VB's Calls-to-Field.
    assert!(
        bound.iter().any(|e| e.kind == EdgeKind::Calls
            && nodes
                .get(&e.target.0)
                .is_some_and(|n| n.kind == NodeKind::Field && n.location.file.ends_with(".vb"))),
        "a VB call to a field"
    );

    // big.ts fell back: estate's own pipeline extracted it, and no codegraph fact came from it.
    assert!(
        nodes
            .values()
            .any(|n| n.location.file == "src/ts/big.ts" && n.name == "big"),
        "estate extracted the oversized file"
    );
    assert!(
        !bound.iter().any(|e| file_of(e) == "src/ts/big.ts"),
        "no codegraph fact from a size_exceeded file"
    );

    // The stylesheet import is estate's, through the fall-through.
    assert!(
        edges.iter().any(|e| e.kind == EdgeKind::Imports
            && file_of(e) == "src/ts/shapes.ts"
            && e.target.0.contains("theme.css")
            && !e.resolved_by.starts_with("codegraph:")),
        "shapes.ts imports theme.css through estate's resolvers: {:?}",
        edges
            .iter()
            .filter(|e| file_of(e) == "src/ts/shapes.ts" && e.kind == EdgeKind::Imports)
            .map(|e| (&e.target.0, &e.resolved_by))
            .collect::<Vec<_>>()
    );
}
