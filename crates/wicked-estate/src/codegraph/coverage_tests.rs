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

/// The gate store's input: the recording, then the synthetic rows for the kinds codegraph never
/// emits (`synthetic-kinds.sql`, labelled `resolvedBy: synthetic`), as
/// `scripts/codegraph-fixture-store.py store` applies them.
fn gate_sql() -> String {
    let path = coverage_dir().join("synthetic-kinds.sql");
    let synthetic = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("the gate fixture {}: {e}", path.display()));
    format!("{}\n{synthetic}", coverage_sql())
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

    assert!(
        EMITTED.iter().all(|k| !NOT_EMITTED.contains(k)),
        "a kind is either emitted or not"
    );
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
    TEST_DATABASE_SQL.with(|t| *t.borrow_mut() = Some(gate_sql()));
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
        // From synthetic-kinds.sql: the kinds codegraph 1.6.4 never emits.
        EdgeKind::Overrides,
        EdgeKind::Returns,
        EdgeKind::HasType,
        EdgeKind::Other("exports".into()),
    ] {
        assert!(kinds.contains(&format!("{k:?}")), "{k:?} in {kinds:?}");
    }
    assert!(
        bound
            .iter()
            .filter(|e| e.resolved_by == "codegraph:synthetic")
            .count()
            == 4,
        "each synthetic row lands once, labelled"
    );
    assert!(
        !edges
            .iter()
            .any(|e| e.kind == EdgeKind::Other("navigates".into())),
        "navigates points at a route node, which is not imported"
    );
    // Route nodes are not imported, so every fact with a route endpoint counts as not imported.
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(&coverage_sql()).unwrap();
    let route_facts: usize = conn
        .query_row(
            "SELECT count(*) FROM edges e JOIN nodes s ON s.id = e.source \
             JOIN nodes t ON t.id = e.target WHERE s.kind = 'route' OR t.kind = 'route'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap() as usize;
    assert!(route_facts >= 2, "navigates and the route's reference");
    assert!(
        dropped >= route_facts,
        "not imported {dropped} counts the {route_facts} route facts"
    );

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

#[test]
fn a_minted_csharp_record_in_a_block_namespace_gets_estates_id_591() {
    // estate#591: estate's C# ids never nest under a `namespace X { … }` block (csharp.scm
    // captures no namespace), so a minted id must not either. `Money` is a record (estate's C#
    // extraction has no records) in `namespace Cov { … }`, next to estate's own class `Credit`.
    let tmp = tempfile::tempdir().unwrap();
    let (store, _) = index_coverage(tmp.path());
    let nodes = store.all_nodes().unwrap();
    let in_ledger = |name: &str| {
        nodes
            .iter()
            .find(|n| n.location.file == "src/csharp/Ledger.cs" && n.name == name)
            .unwrap_or_else(|| panic!("{name} is a node in Ledger.cs"))
    };
    let credit = in_ledger("Credit");
    assert!(
        credit.metadata.get("origin").is_none(),
        "Credit is estate's own node"
    );
    assert!(
        !credit.symbol.0.contains("Cov#"),
        "estate's C# id does not nest under the namespace: {}",
        credit.symbol.0
    );
    let money = in_ledger("Money");
    assert_eq!(
        money.metadata.get("origin").and_then(|v| v.as_str()),
        Some("codegraph"),
        "Money is minted"
    );
    assert_eq!(
        money.symbol.0,
        credit.symbol.0.replace("Credit#", "Money#"),
        "the minted id follows estate's scheme"
    );
}

/// Round-1 fold (codex F10, P1): the state dir sits under the indexed root by default
/// (`.wicked-estate/graph.db.codegraph`), so a checkout can carry a symlink there. The mirror
/// writes followed it: a link at `tree/src` took the repo's (redacted) sources to wherever it
/// pointed, and a linked `.wicked-estate` moved the whole state dir. A run must refuse a symlink
/// anywhere on the state dir's path below the root or inside it, write nothing through it, and
/// fall back to estate's pipeline.
#[cfg(unix)]
#[test]
fn a_symlink_on_the_state_dirs_path_is_never_written_through() {
    for planted in [
        ".wicked-estate",
        ".wicked-estate/graph.db.codegraph/tree/src",
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let root = coverage_copy(tmp.path());
        let canary = tmp.path().join("canary");
        std::fs::create_dir_all(&canary).unwrap();
        let link = root.join(planted);
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&canary, &link).unwrap();
        let mut store = SqliteStore::open(tmp.path().join("g.db")).unwrap();
        let cfg = CodegraphConfig {
            bundle: tmp.path().join("no-bundle"),
            state_dir: root.join(".wicked-estate/graph.db.codegraph"),
            languages: Vec::new(),
            timeout: Duration::from_secs(5),
        };
        TEST_DATABASE_SQL.with(|t| *t.borrow_mut() = Some(gate_sql()));
        crate::index_path_with(&mut store, &root, None, Some(&cfg)).unwrap();
        TEST_DATABASE_SQL.with(|t| *t.borrow_mut() = None);
        let written: Vec<_> = walkdir_files(&canary);
        assert!(
            written.is_empty(),
            "{planted}: nothing is written through the link: {written:?}"
        );
        assert!(
            !store
                .all_edges()
                .unwrap()
                .iter()
                .any(|e| e.resolved_by.starts_with("codegraph:")),
            "{planted}: the run fell back to estate's pipeline"
        );
        assert!(
            store
                .all_nodes()
                .unwrap()
                .iter()
                .any(|n| n.location.file == "src/csharp/Ledger.cs"),
            "{planted}: estate's pipeline indexed the repo"
        );
    }
}

/// Round-2 fold (codex r2 F1, F2): the state dir's path spelled through another alias of the
/// root (here `--db` under a symlinked spelling of the repo) skipped the lexical root check, and
/// a directory the scan cannot list was skipped. Either way a link planted in the checkout took
/// the writes out of the state dir. Both must refuse and fall back.
#[cfg(unix)]
#[test]
fn a_state_dir_link_behind_an_alias_or_an_unlistable_dir_is_refused() {
    use std::os::unix::fs::PermissionsExt;
    for case in ["alias", "unlistable"] {
        let tmp = tempfile::tempdir().unwrap();
        let root = coverage_copy(tmp.path());
        let canary = tmp.path().join("canary");
        std::fs::create_dir_all(&canary).unwrap();
        let mut locked = None;
        let state_dir = match case {
            "alias" => {
                let alias = tmp.path().join("alias");
                std::os::unix::fs::symlink(&root, &alias).unwrap();
                std::os::unix::fs::symlink(&canary, root.join(".wicked-estate")).unwrap();
                alias.join(".wicked-estate/graph.db.codegraph")
            }
            _ => {
                let state = root.join(".wicked-estate/graph.db.codegraph");
                let src = state.join("tree/src");
                std::fs::create_dir_all(&src).unwrap();
                std::os::unix::fs::symlink(&canary, src.join("c")).unwrap();
                // Writable and searchable, not listable.
                std::fs::set_permissions(&src, std::fs::Permissions::from_mode(0o300)).unwrap();
                locked = Some(src);
                state
            }
        };
        let mut store = SqliteStore::open(tmp.path().join("g.db")).unwrap();
        let cfg = CodegraphConfig {
            bundle: tmp.path().join("no-bundle"),
            state_dir,
            languages: Vec::new(),
            timeout: Duration::from_secs(5),
        };
        TEST_DATABASE_SQL.with(|t| *t.borrow_mut() = Some(gate_sql()));
        let result = crate::index_path_with(&mut store, &root, None, Some(&cfg));
        TEST_DATABASE_SQL.with(|t| *t.borrow_mut() = None);
        if let Some(src) = &locked {
            std::fs::set_permissions(src, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        result.unwrap();
        let written: Vec<_> = walkdir_files(&canary);
        assert!(
            written.is_empty(),
            "{case}: nothing is written through the link: {written:?}"
        );
        assert!(
            !store
                .all_edges()
                .unwrap()
                .iter()
                .any(|e| e.resolved_by.starts_with("codegraph:")),
            "{case}: the run fell back to estate's pipeline"
        );
    }
}

/// estate#657 (R-B codex-check on #584, W20): one run reads a switched file three times
/// (classification, mirror, extraction). A save between them paired one revision's codegraph
/// facts with another's definitions, and a save before the mirror, undone before the next run,
/// left `mirror.json` naming the old digest for the new text, so codegraph kept reading the stale
/// copy. A file that changes during a run keeps estate's pipeline for that run, and the next run
/// mirrors it as it is.
#[test]
fn a_file_that_changes_during_a_run_keeps_estates_pipeline_and_is_mirrored_again_657() {
    let ledger = "src/csharp/Ledger.cs";
    let minted_in_ledger = |store: &SqliteStore| {
        store.all_nodes().unwrap().iter().any(|n| {
            n.location.file == ledger
                && n.metadata.get("origin").and_then(|v| v.as_str()) == Some("codegraph")
        })
    };
    for at in ["before-mirror", "after-mirror"] {
        let tmp = tempfile::tempdir().unwrap();
        let root = coverage_copy(tmp.path());
        let original = std::fs::read_to_string(root.join(ledger)).unwrap();
        let mut store = SqliteStore::open(tmp.path().join("g.db")).unwrap();
        let cfg = CodegraphConfig {
            bundle: tmp.path().join("no-bundle"),
            state_dir: tmp.path().join("g.db.codegraph"),
            languages: Vec::new(),
            timeout: Duration::from_secs(5),
        };
        let (path, saved) = (root.join(ledger), format!("{original}// saved mid-run\n"));
        TEST_MIRROR_HOOK.with(|h| {
            *h.borrow_mut() = Some(Box::new(move |now: &str| {
                if now == at {
                    std::fs::write(&path, &saved).unwrap();
                }
            }))
        });
        TEST_DATABASE_SQL.with(|t| *t.borrow_mut() = Some(gate_sql()));
        let first = crate::index_path_with(&mut store, &root, None, Some(&cfg));
        TEST_MIRROR_HOOK.with(|h| *h.borrow_mut() = None);
        first.unwrap();
        assert!(
            !minted_in_ledger(&store),
            "{at}: the file that changed keeps estate's pipeline this run"
        );
        // The save is undone before the next run.
        std::fs::write(root.join(ledger), &original).unwrap();
        let second = crate::index_path_with(&mut store, &root, None, Some(&cfg));
        TEST_DATABASE_SQL.with(|t| *t.borrow_mut() = None);
        second.unwrap();
        let mirrored = std::fs::read_to_string(cfg.mirror_root().join(ledger)).unwrap();
        assert_eq!(
            mirrored, original,
            "{at}: the mirror holds the file as it is"
        );
        assert!(
            minted_in_ledger(&store),
            "{at}: the next run hands the file to codegraph again"
        );
    }
}

#[cfg(unix)]
fn walkdir_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let e = e.unwrap();
            if e.file_type().unwrap().is_dir() {
                stack.push(e.path());
            } else {
                out.push(e.path());
            }
        }
    }
    out
}
