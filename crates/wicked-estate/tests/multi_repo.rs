//! End-to-end: MANY repos in ONE graph.
//!
//! The bug these tests pin: two repos that both have `src/index.ts` mint identical `files.path`
//! rows AND identical SymbolIds (the relative path is embedded in the id), so indexing the second
//! into the first's db silently destroyed it — no error, no warning, `query alpha` → 0 matches.
//!
//! What is asserted here: labelled indexing co-locates them without collision, an un-labelled
//! second index is REFUSED instead of destroying anything, and an un-labelled index is otherwise
//! unchanged — same paths, same ids, same meta keys as before the label existed.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use wicked_estate::repo_scope;
use wicked_estate_core::{EdgeKind, GraphRead};
use wicked_estate_store::{GraphStoreMutExt, SqliteStore};

/// Two repos with the SAME relative path, one shared function name, one unique function name.
/// This is the exact collide reproduction from the bug report.
fn make_repo(dir: &Path, unique: &str) {
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("src/index.ts"),
        format!("export function {unique}() {{ return shared(); }}\nexport function shared() {{ return 1; }}\n"),
    )
    .unwrap();
}

fn fresh_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ci_multirepo_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn names(store: &SqliteStore, name: &str) -> Vec<String> {
    wicked_estate::search(store, name)
        .unwrap()
        .into_iter()
        .map(|n| n.location.file)
        .collect()
}

fn indexed(store: &SqliteStore) -> BTreeSet<String> {
    store.indexed_files().unwrap().into_iter().collect()
}

#[test]
fn labelled_repos_coexist_without_collision() {
    let root = fresh_dir("coexist");
    make_repo(&root.join("repoA"), "alpha");
    make_repo(&root.join("repoB"), "beta");
    let mut store = SqliteStore::open(root.join("shared.db")).unwrap();

    wicked_estate::index_path_as(&mut store, &root.join("repoA"), Some("repoa")).unwrap();
    let stats =
        wicked_estate::index_path_as(&mut store, &root.join("repoB"), Some("repob")).unwrap();

    // Before the fix: files=1, nodes=3 — repoA overwritten.
    assert_eq!(stats.file_count, 2, "both repos' files must survive");
    assert_eq!(
        indexed(&store),
        BTreeSet::from([
            "repoa/src/index.ts".to_string(),
            "repob/src/index.ts".to_string()
        ])
    );

    // Both repos' unique symbols are queryable.
    assert_eq!(names(&store, "alpha"), vec!["repoa/src/index.ts"]);
    assert_eq!(names(&store, "beta"), vec!["repob/src/index.ts"]);

    // The COLLIDING name resolves to two distinct symbols, one per repo.
    let shared = wicked_estate::search(&store, "shared").unwrap();
    assert_eq!(shared.len(), 2, "shared() must be two symbols, not one");
    let ids: BTreeSet<&str> = shared.iter().map(|n| n.symbol.as_str()).collect();
    assert_eq!(ids.len(), 2, "SymbolIds must differ per repo: {ids:?}");
}

#[test]
fn per_repo_provenance_does_not_clobber() {
    let root = fresh_dir("provenance");
    make_repo(&root.join("repoA"), "alpha");
    make_repo(&root.join("repoB"), "beta");
    let mut store = SqliteStore::open(root.join("shared.db")).unwrap();
    wicked_estate::index_path_as(&mut store, &root.join("repoA"), Some("repoa")).unwrap();
    wicked_estate::index_path_as(&mut store, &root.join("repoB"), Some("repob")).unwrap();

    let reg = repo_scope::registry(&store);
    assert_eq!(reg.len(), 2, "both repos must be registered: {reg:?}");
    assert_eq!(reg[0].label, "repoa");
    assert_eq!(reg[1].label, "repob");
    assert!(reg[0].root.ends_with("repoA"), "{:?}", reg[0].root);
    assert!(reg[1].root.ends_with("repoB"), "{:?}", reg[1].root);

    // The singular repo_* keys stay untouched in a multi-repo graph — there is no ONE commit to
    // report, and reporting the last-indexed repo's is what clobbering looked like.
    assert!(
        store.repo_info().unwrap().is_none(),
        "singular repo_info must not be written for labelled indexes"
    );
}

#[test]
fn unlabelled_second_repo_is_refused_and_nothing_is_lost() {
    let root = fresh_dir("guard");
    make_repo(&root.join("repoA"), "alpha");
    make_repo(&root.join("repoB"), "beta");
    let mut store = SqliteStore::open(root.join("shared.db")).unwrap();
    wicked_estate::index_path(&mut store, &root.join("repoA")).unwrap();

    let err = wicked_estate::index_path(&mut store, &root.join("repoB"))
        .expect_err("the silent overwrite must now be a refusal");
    let msg = err.to_string();
    assert!(msg.contains("REPO COLLISION"), "{msg}");
    assert!(msg.contains("--repo"), "the error must name the fix: {msg}");

    // The refusal is a no-op on the graph: repoA is exactly as it was.
    assert_eq!(names(&store, "alpha"), vec!["src/index.ts"]);
    assert!(names(&store, "beta").is_empty());
    assert_eq!(store.stats().unwrap().file_count, 1);
}

#[test]
fn label_cannot_be_reused_for_a_different_repo() {
    let root = fresh_dir("labelreuse");
    make_repo(&root.join("repoA"), "alpha");
    make_repo(&root.join("repoB"), "beta");
    let mut store = SqliteStore::open(root.join("shared.db")).unwrap();
    wicked_estate::index_path_as(&mut store, &root.join("repoA"), Some("shared-name")).unwrap();

    let err = wicked_estate::index_path_as(&mut store, &root.join("repoB"), Some("shared-name"))
        .expect_err("a label already bound to another repo must be refused");
    assert!(err.to_string().contains("already bound"), "{err}");
    assert_eq!(names(&store, "alpha"), vec!["shared-name/src/index.ts"]);
}

#[test]
fn labelled_index_into_an_unlabelled_graph_is_refused() {
    let root = fresh_dir("mix");
    make_repo(&root.join("repoA"), "alpha");
    make_repo(&root.join("repoB"), "beta");
    let mut store = SqliteStore::open(root.join("shared.db")).unwrap();
    wicked_estate::index_path(&mut store, &root.join("repoA")).unwrap();

    let err = wicked_estate::index_path_as(&mut store, &root.join("repoB"), Some("repob"))
        .expect_err("mixing labelled and un-labelled content must be refused");
    assert!(err.to_string().contains("un-labelled content"), "{err}");
}

#[test]
fn re_indexing_one_repo_leaves_the_others_alone() {
    let root = fresh_dir("sweep");
    make_repo(&root.join("repoA"), "alpha");
    make_repo(&root.join("repoB"), "beta");
    let mut store = SqliteStore::open(root.join("shared.db")).unwrap();
    wicked_estate::index_path_as(&mut store, &root.join("repoA"), Some("repoa")).unwrap();
    wicked_estate::index_path_as(&mut store, &root.join("repoB"), Some("repob")).unwrap();

    // Add a file to repoA and drop its original: the delete-sweep must stay inside `repoa/`.
    fs::write(
        root.join("repoA/src/other.ts"),
        "export function gamma() {}\n",
    )
    .unwrap();
    fs::remove_file(root.join("repoA/src/index.ts")).unwrap();
    wicked_estate::index_path_as(&mut store, &root.join("repoA"), Some("repoa")).unwrap();

    assert_eq!(
        indexed(&store),
        BTreeSet::from([
            "repoa/src/other.ts".to_string(),
            "repob/src/index.ts".to_string()
        ]),
        "repoB's rows must survive repoA's re-index"
    );
    assert_eq!(names(&store, "beta"), vec!["repob/src/index.ts"]);
    assert!(
        names(&store, "alpha").is_empty(),
        "repoA's deleted file swept"
    );
}

/// The compatibility claim, proved rather than asserted: with no label, the pipeline stores the
/// same paths, the same SymbolIds, and the same meta keys it did before labels existed — and a
/// labelled index of the SAME tree differs from it in exactly one way: the `<label>/` prefix.
#[test]
fn unlabelled_indexing_is_unchanged() {
    let root = fresh_dir("compat");
    make_repo(&root.join("repoA"), "alpha");

    let mut plain = SqliteStore::open(root.join("plain.db")).unwrap();
    let plain_stats = wicked_estate::index_path(&mut plain, &root.join("repoA")).unwrap();
    let mut labelled = SqliteStore::open(root.join("labelled.db")).unwrap();
    let labelled_stats =
        wicked_estate::index_path_as(&mut labelled, &root.join("repoA"), Some("repoa")).unwrap();

    // Same graph shape either way.
    assert_eq!(plain_stats.node_count, labelled_stats.node_count);
    assert_eq!(plain_stats.edge_count, labelled_stats.edge_count);
    assert_eq!(plain_stats.file_count, labelled_stats.file_count);

    // Un-labelled paths are bare; labelled ones are the same paths under the label.
    assert_eq!(
        indexed(&plain),
        BTreeSet::from(["src/index.ts".to_string()])
    );
    let expected: BTreeSet<String> = indexed(&plain)
        .iter()
        .map(|p| format!("repoa/{p}"))
        .collect();
    assert_eq!(indexed(&labelled), expected);

    // Same for the ids: prefixing the path inside each SymbolId maps one set onto the other.
    let plain_ids: BTreeSet<String> = plain
        .all_nodes()
        .unwrap()
        .iter()
        .map(|n| n.symbol.as_str().replace("src/index", "repoa/src/index"))
        .collect();
    let labelled_ids: BTreeSet<String> = labelled
        .all_nodes()
        .unwrap()
        .iter()
        .map(|n| n.symbol.as_str().to_string())
        .collect();
    assert_eq!(plain_ids, labelled_ids);

    // Meta: the un-labelled db keeps the singular repo layout and gains NO multi-repo keys.
    assert!(
        repo_scope::registry(&plain).is_empty(),
        "an un-labelled index must not register a repo"
    );
    assert!(plain.meta_get_key("repo_labels").is_none());
    assert_eq!(
        plain.meta_get_key("indexed_version").as_deref(),
        Some(env!("CARGO_PKG_VERSION")),
        "the singular indexed_version key must still be written"
    );
    assert!(plain.meta_get_key("indexed_root").is_some());
    assert!(plain.meta_get_key("extra_rules_digest").is_some());

    // Re-indexing the un-labelled db is still allowed and still incremental.
    let again = wicked_estate::index_path(&mut plain, &root.join("repoA")).unwrap();
    assert_eq!(again.node_count, plain_stats.node_count);
}

/// Repo-scoped RESOLUTION, proved by the edge it saves. `sa` calls `uniqHelper`, defined in a
/// DIFFERENT directory of `sa`; `sb` defines a `uniqHelper` too. Only `NameResolver` can wire this
/// (not same-file, not same-dir), and it resolves unique candidates only — so if the resolver's
/// index saw the whole graph instead of just `sa/`, the name would be ambiguous and the call edge
/// would silently vanish. `sb` is indexed FIRST so its node is already present when `sa` resolves.
#[test]
fn resolution_is_scoped_to_the_indexed_repo() {
    let root = fresh_dir("resolvescope");
    fs::create_dir_all(root.join("sa/src/a")).unwrap();
    fs::create_dir_all(root.join("sa/src/b")).unwrap();
    fs::create_dir_all(root.join("sb/lib/x")).unwrap();
    fs::write(
        root.join("sa/src/b/helper.ts"),
        "export function uniqHelper() { return 1; }\n",
    )
    .unwrap();
    fs::write(
        root.join("sa/src/a/caller.ts"),
        "export function callerA() { return uniqHelper(); }\n",
    )
    .unwrap();
    fs::write(
        root.join("sb/lib/x/helper.ts"),
        "export function uniqHelper() { return 2; }\n",
    )
    .unwrap();

    let mut store = SqliteStore::open(root.join("shared.db")).unwrap();
    wicked_estate::index_path_as(&mut store, &root.join("sb"), Some("sb")).unwrap();
    wicked_estate::index_path_as(&mut store, &root.join("sa"), Some("sa")).unwrap();

    let wired = store.all_edges().unwrap().into_iter().any(|e| {
        e.source.as_str().contains("sa/src/a/caller")
            && e.target.as_str().contains("sa/src/b/helper")
    });
    assert!(
        wired,
        "callerA → uniqHelper must resolve inside `sa`; an unscoped resolver sees sb's \
         same-name symbol, calls it ambiguous, and drops the edge"
    );
}

/// Drop-in extra-edge rules through a LABELLED index — the ids the rules mint are rewritten into
/// the repo's namespace. The label here is `docs`, which is ALSO a top-level directory of the
/// repo: the rule's target `docs/triage.md` must become `docs/docs/triage.md` to match the file
/// row, and an id-namespacer that skipped ids "already" starting with `docs/` silently produced a
/// dangling edge that the prune then deleted.
#[test]
fn extra_edges_survive_a_label_that_collides_with_a_directory_name() {
    const RULES: &str = r#"
[[rule]]
name      = "archetype-playbook"
file_glob = ".claude-plugin/archetypes.json"
pattern   = '(?m)^ {4}"(?P<name>[a-z][a-z0-9_-]*)":\s*\{'

[rule.emit_node]
id_template   = "archetype:{name}"
label_capture = "name"
kind          = "other:archetype"
node_scheme   = "archetype"

[rule.emit_edge]
kind               = "references"
source_id_template = "archetype:{name}"
source_node_scheme = "archetype"
target_kind        = "file"
target_id_template = "docs/{name}.md"
"#;
    const CATALOG: &str =
        "{\n  \"archetypes\": {\n    \"triage\": {\n      \"phases\": []\n    }\n  }\n}";

    let root = fresh_dir("xedgelabel");
    let repo = root.join("repoA");
    fs::create_dir_all(repo.join(".wicked-estate-extractors")).unwrap();
    fs::create_dir_all(repo.join(".claude-plugin")).unwrap();
    fs::create_dir_all(repo.join("docs")).unwrap();
    fs::write(repo.join(".wicked-estate-extractors/archetype.toml"), RULES).unwrap();
    fs::write(repo.join(".claude-plugin/archetypes.json"), CATALOG).unwrap();
    fs::write(repo.join("docs/triage.md"), "# triage\n").unwrap();

    let mut store = SqliteStore::open(root.join("shared.db")).unwrap();
    wicked_estate::index_path_as(&mut store, &repo, Some("docs")).unwrap();

    // The rule's file target is namespaced like every other path this run stored.
    assert!(
        indexed(&store).contains("docs/docs/triage.md"),
        "labelled file rows: {:?}",
        indexed(&store)
    );
    let refs: Vec<String> = store
        .all_edges()
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == EdgeKind::References)
        .map(|e| format!("{} -> {}", e.source.as_str(), e.target.as_str()))
        .collect();
    assert_eq!(
        refs,
        vec!["archetype synthetic docs/archetype:triage: -> file . docs/docs/triage.md:"],
        "the rule-emitted edge must survive the prune, namespaced on BOTH ends"
    );
}

/// SCIP correlation matches a document's REPO-relative path against `nodes.file`. In a labelled
/// graph those differ by the `<label>/` prefix, so an un-labelled ingest silently correlates
/// nothing. (The fixture is the resolve crate's — one `.scip` file, not two.)
#[test]
fn scip_ingest_correlates_against_a_labelled_repo() {
    use std::io::Write as _;
    use wicked_estate_core::{
        Descriptor, GraphWrite, Language, Location, Node, NodeKind, Span, Symbol,
    };

    let fixture: &[u8] =
        include_bytes!("../../wicked-estate-resolve/tests/fixtures/sample-ts.scip");
    let root = fresh_dir("scip");
    let scip_path = root.join("index.scip");
    let mut f = fs::File::create(&scip_path).unwrap();
    f.write_all(fixture).unwrap();
    drop(f);

    // Two nodes as the labelled indexer would have stored them: paths under `repoa/`.
    let node = |name: &str, file: &str, l0: u32, l1: u32| {
        Node::new(
            Symbol::global(
                "ci-test",
                None,
                vec![Descriptor::method(format!("{file}::{name}"), None)],
            )
            .id(),
            NodeKind::Function,
            name,
            Language::new("typescript"),
            Location::new(
                file,
                Span {
                    start_byte: 0,
                    end_byte: 0,
                    start_line: l0,
                    start_col: 0,
                    end_line: l1,
                    end_col: 80,
                },
            ),
        )
    };
    let mut store = SqliteStore::open(root.join("scip.db")).unwrap();
    store
        .upsert_nodes(&[
            node("helper", "repoa/src/util.ts", 0, 0),
            node("run", "repoa/src/main.ts", 1, 2),
        ])
        .unwrap();

    let blind = wicked_estate::ingest_scip(&mut store, &root, &scip_path).unwrap();
    assert_eq!(
        blind, 0,
        "un-labelled correlation cannot match `repoa/…` nodes"
    );
    let scoped =
        wicked_estate::ingest_scip_as(&mut store, &root, &scip_path, Some("repoa")).unwrap();
    assert!(
        scoped > 0,
        "labelled correlation must find the precise edges"
    );
}

// ── The monorepo hole, and the two halves of repo identity ────────────────────────────────────
//
// Repo identity used to be the git `origin` remote alone. Every package of a monorepo shares one
// `origin` — and two of them both mint `src/index.ts` — so the guard read `mono/pkgB` as
// `mono/pkgA` re-indexed, allowed it, and the graph-wide delete sweep removed pkgA. Exit 0, no
// warning, `query uniqPkgA` → 0 matches: the exact failure the guard exists to stop, reached
// through the commonest layout there is.
//
// Identity is now (remote, position inside the work tree). The pair of tests below pin BOTH
// directions, because a predicate that just answers "different" more often would pass the first
// one while breaking every re-index.

/// `git init` a tree with an `origin` that is not a real host, so nothing here touches a network.
fn git_repo(dir: &Path, origin: &str) {
    let run = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .expect("git");
        assert!(out.status.success(), "git {args:?}: {out:?}");
    };
    fs::create_dir_all(dir).unwrap();
    run(&["init", "-q", "."]);
    run(&["remote", "add", "origin", origin]);
}

#[test]
fn two_packages_of_one_monorepo_are_not_the_same_repo() {
    let root = fresh_dir("monorepo");
    let mono = root.join("mono");
    git_repo(&mono, "git@github.com:acme/mono.git");
    make_repo(&mono.join("pkgA"), "uniqPkgA");
    make_repo(&mono.join("pkgB"), "uniqPkgB");

    let mut store = SqliteStore::open(root.join("mono.db")).unwrap();
    wicked_estate::index_path_as(&mut store, &mono.join("pkgA"), None).unwrap();

    let err = wicked_estate::index_path_as(&mut store, &mono.join("pkgB"), None)
        .expect_err("pkgB shares `src/index.ts` with pkgA — indexing it un-labelled destroys pkgA");
    assert!(
        err.to_string().contains("REPO COLLISION"),
        "must be the loud refusal, got: {err}"
    );
    assert_eq!(
        names(&store, "uniqPkgA"),
        vec!["src/index.ts"],
        "a refusal must leave the graph exactly as it was"
    );

    // Labelled, the same two packages are two repos and co-exist.
    let mut labelled = SqliteStore::open(root.join("mono-labelled.db")).unwrap();
    wicked_estate::index_path_as(&mut labelled, &mono.join("pkgA"), Some("pkga")).unwrap();
    wicked_estate::index_path_as(&mut labelled, &mono.join("pkgB"), Some("pkgb"))
        .expect("sibling packages of one monorepo are different trees and must both be indexable");
    assert_eq!(names(&labelled, "uniqPkgA"), vec!["pkga/src/index.ts"]);
    assert_eq!(names(&labelled, "uniqPkgB"), vec!["pkgb/src/index.ts"]);
}

/// The other direction: a checkout that MOVED (or was re-cloned elsewhere) is still one repo, and
/// re-indexing it must stay an ordinary incremental run. Same remote, same position in the work
/// tree, different path on disk.
#[test]
fn a_moved_checkout_is_still_the_same_repo() {
    let root = fresh_dir("moved");
    let here = root.join("here");
    let there = root.join("there");
    git_repo(&here, "git@github.com:acme/solo.git");
    git_repo(&there, "https://github.com/acme/solo");
    make_repo(&here, "uniqSolo");
    make_repo(&there, "uniqSolo");

    let mut store = SqliteStore::open(root.join("solo.db")).unwrap();
    wicked_estate::index_path_as(&mut store, &here, None).unwrap();
    wicked_estate::index_path_as(&mut store, &there, None)
        .expect("the same repo at a new path must re-index, not refuse");

    let mut labelled = SqliteStore::open(root.join("solo-labelled.db")).unwrap();
    wicked_estate::index_path_as(&mut labelled, &here, Some("solo")).unwrap();
    wicked_estate::index_path_as(&mut labelled, &there, Some("solo"))
        .expect("same label, same repo, new path — an incremental re-index");
    assert_eq!(names(&labelled, "uniqSolo"), vec!["solo/src/index.ts"]);
}

/// Per-repo `indexed_version`. It decides whether a repo's rows are re-extracted after a binary
/// upgrade, and one shared key makes the FIRST repo indexed under the new binary answer for all of
/// them: every other repo keeps rows the old extractor produced and is never told.
#[test]
fn the_binary_version_is_recorded_per_repo() {
    let root = fresh_dir("version");
    make_repo(&root.join("repoA"), "uniqVa");
    make_repo(&root.join("repoB"), "uniqVb");
    let mut store = SqliteStore::open(root.join("v.db")).unwrap();
    wicked_estate::index_path_as(&mut store, &root.join("repoA"), Some("va")).unwrap();
    wicked_estate::index_path_as(&mut store, &root.join("repoB"), Some("vb")).unwrap();

    for label in ["va", "vb"] {
        let key = repo_scope::meta_key(Some(label), "indexed_version");
        assert_eq!(
            store.meta_get_key(&key).as_deref(),
            Some(env!("CARGO_PKG_VERSION")),
            "'{label}' must record its own binary version under {key}"
        );
    }
    assert!(
        store.meta_get_key("indexed_version").is_none(),
        "a labelled run must not write the SHARED key — one repo's version would answer for every \
         other repo's staleness"
    );
    // Same argument, same fix, for the extra-edge rule digest: the rules are read from ONE root.
    assert!(
        store
            .meta_get_key(&repo_scope::meta_key(Some("va"), "extra_rules_digest"))
            .is_some()
            && store.meta_get_key("extra_rules_digest").is_none(),
        "the extra-edge rule digest is a property of one repo's tree, not of the graph"
    );
}

/// The incremental digest skip has to survive a label. The set of paths currently on disk is
/// compared against the set previously indexed, and BOTH sides carry the `<label>/` prefix; if
/// only one did, every file would read as deleted, get swept, and be re-extracted from scratch on
/// every single run — the end state still correct, the work and the change-log churn not.
#[test]
fn a_second_labelled_index_of_an_unchanged_tree_sweeps_nothing() {
    use wicked_estate_core::ChangeOp;

    let root = fresh_dir("incremental");
    make_repo(&root.join("repoA"), "uniqInc");
    let mut store = SqliteStore::open(root.join("inc.db")).unwrap();
    wicked_estate::index_path_as(&mut store, &root.join("repoA"), Some("inc")).unwrap();

    let cursor = store.changes_since(0).unwrap().last().unwrap().seq;
    wicked_estate::index_path_as(&mut store, &root.join("repoA"), Some("inc")).unwrap();

    let after: Vec<_> = store.changes_since(cursor).unwrap();
    assert!(
        after.iter().all(|c| c.op != ChangeOp::Remove),
        "nothing changed on disk, so the second run must remove nothing: {after:?}"
    );
    assert_eq!(names(&store, "uniqInc"), vec!["inc/src/index.ts"]);
}

/// `ingest_scip_as` must validate its label exactly as `index_path_as` does.
///
/// The label becomes the `<label>/` prefix stripped from SCIP's relative paths and re-applied to
/// the edge locations written back. Label validation is the ONE thing that makes path forging
/// unreachable, so a second entry point that skipped it was a hole in that guarantee
/// (Copilot on #117) — reachable from the CLI as `wicked-estate scip <root> --repo ../evil`.
#[test]
fn scip_ingest_rejects_a_forged_label() {
    let root = fresh_dir("scip_label");
    make_repo(&root.join("repo"), "alpha");
    let mut store = SqliteStore::open(root.join("s.db")).unwrap();
    wicked_estate::index_path_as(&mut store, &root.join("repo"), Some("ok")).unwrap();

    // No SCIP file is needed: validation must reject BEFORE any file read or write.
    let missing = root.join("index.scip");
    for bad in ["../evil", "a/b", "/abs", "..", "", "  ", "a\\b"] {
        let err =
            wicked_estate::ingest_scip_as(&mut store, &root.join("repo"), &missing, Some(bad))
                .expect_err(&format!("label {bad:?} must be refused"));
        let msg = err.to_string().to_lowercase();
        assert!(
            !msg.contains("cannot read"),
            "label {bad:?} reached the file read before validation: {msg}"
        );
    }

    // A valid label still gets past validation (and then fails on the absent file, as it should).
    let err = wicked_estate::ingest_scip_as(&mut store, &root.join("repo"), &missing, Some("ok"))
        .expect_err("absent scip file must still error");
    assert!(
        err.to_string().to_lowercase().contains("cannot read"),
        "a VALID label must get past validation to the file read: {err}"
    );
}

/// Lane relative-imports S6: the root guard counts depth below the LABEL prefix, so a labelled
/// repo binds and parks exactly where the plain one does. `./util` binds in both;
/// `../../repoa/src/util` — which under a prefix-blind guard would false-bind the labelled
/// store's own `repoa/src/util.ts` — parks in BOTH. (Uses its own fixture rather than extending
/// `make_repo`: the shared fixture's file set is pinned by the other tests' assertions.)
#[test]
fn labelled_relative_imports_match_plain() {
    let root = fresh_dir("relimp_label");
    let repo = root.join("repoA");
    fs::create_dir_all(repo.join("src")).unwrap();
    fs::write(repo.join("src/util.ts"), "export const u = 1;\n").unwrap();
    fs::write(
        repo.join("src/index.ts"),
        "import './util';\nimport '../../repoa/src/util';\nexport const i = 1;\n",
    )
    .unwrap();

    let mut plain = SqliteStore::open(root.join("plain.db")).unwrap();
    let plain_stats = wicked_estate::index_path(&mut plain, &repo).unwrap();
    let mut labelled = SqliteStore::open(root.join("labelled.db")).unwrap();
    let labelled_stats = wicked_estate::index_path_as(&mut labelled, &repo, Some("repoa")).unwrap();

    assert_eq!(
        plain_stats.edge_count, labelled_stats.edge_count,
        "plain and labelled graphs must have the same edge count"
    );

    let rel_edges = |store: &SqliteStore| -> Vec<(String, String)> {
        store
            .all_edges()
            .unwrap()
            .into_iter()
            .filter(|e| e.resolved_by == "relative-import")
            .map(|e| (e.source.0.clone(), e.target.0.clone()))
            .collect()
    };
    let p = rel_edges(&plain);
    let l = rel_edges(&labelled);
    assert_eq!(p.len(), 1, "plain: only './util' binds: {p:?}");
    assert_eq!(
        l.len(),
        1,
        "labelled: the escaping spec must PARK, not bind repoa/src/util.ts: {l:?}"
    );
    assert!(l[0].1.contains("repoa/src/util.ts"), "{l:?}");

    // The escaping spec is parked in both stores.
    for store in [&plain, &labelled] {
        assert!(
            !store
                .unresolved_refs_for_name("'../../repoa/src/util'")
                .unwrap()
                .is_empty(),
            "escaping spec must be parked"
        );
    }
}

/// Shared Import node across REPOS (incr-integrity lane): the spec-keyed symbol carries no repo
/// label, so repos importing the same spec share ONE node row. Deleting the owning repo's
/// importer must keep the node for the other repo (re-homed across repo prefixes) and leave
/// 0 dangling edges. Ownership itself is DETERMINISTIC under wicked-estate#152 — the MIN(file)
/// contribution across repo prefixes, not the last-indexed repo (the old ownership wart).
#[test]
fn cross_repo_shared_import_survives_owner_repo_deletion() {
    use wicked_estate_core::NodeKind;
    let root = fresh_dir("shared_import");
    let write_importer = |repo: &str| {
        fs::create_dir_all(root.join(repo).join("src")).unwrap();
        fs::write(
            root.join(repo).join("src/index.ts"),
            "import crypto from 'node:crypto';\nexport const v = 1;\n",
        )
        .unwrap();
    };
    write_importer("repoA");
    write_importer("repoB");
    // repoA gets a second file so its post-deletion re-index still has content.
    fs::write(
        root.join("repoA").join("src/other.ts"),
        "export const w = 2;\n",
    )
    .unwrap();

    let mut store = SqliteStore::open(root.join("shared.db")).unwrap();
    wicked_estate::index_path_as(&mut store, &root.join("repoA"), Some("repoa")).unwrap();
    wicked_estate::index_path_as(&mut store, &root.join("repoB"), Some("repob")).unwrap();

    let import_node = |store: &SqliteStore| {
        store
            .all_nodes()
            .unwrap()
            .into_iter()
            .find(|n| n.kind == NodeKind::Import && n.name == "node:crypto")
    };
    // Ownership: deterministic MIN(file) across contributions (wicked-estate#152) — repoa owns
    // the shared, label-less node even though repob indexed LAST (the last-writer wart is dead).
    let node = import_node(&store).expect("shared import node");
    assert_eq!(
        node.location.file, "repoa/src/index.ts",
        "precondition: the repo we delete from is the current owner"
    );

    // Delete repoA's importer; re-index ONLY repoA (deletion-only for that repo).
    fs::remove_file(root.join("repoA").join("src/index.ts")).unwrap();
    wicked_estate::index_path_as(&mut store, &root.join("repoA"), Some("repoa")).unwrap();

    let node = import_node(&store)
        .expect("repo A's deletion must not destroy repo B's shared Import node");
    // Deterministic post-re-home owner: only repob's contribution/edge survives.
    assert_eq!(
        node.location.file, "repob/src/index.ts",
        "kept node re-homed ACROSS the repo prefix to the surviving importer (exact MIN(file))"
    );
    // Repo B's File→Import edge is live.
    let importer_edges: Vec<_> = store
        .all_edges()
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == EdgeKind::Imports && e.target == node.symbol)
        .collect();
    assert_eq!(
        importer_edges.len(),
        1,
        "exactly repo B's edge: {importer_edges:?}"
    );
    assert_eq!(
        store
            .get_node(&importer_edges[0].source)
            .unwrap()
            .expect("live source")
            .location
            .file,
        "repob/src/index.ts"
    );
    // No dangling edges anywhere.
    for e in store.all_edges().unwrap() {
        assert!(
            store.get_node(&e.source).unwrap().is_some()
                && store.get_node(&e.target).unwrap().is_some(),
            "dangling edge: {} -> {} ({:?})",
            e.source.as_str(),
            e.target.as_str(),
            e.kind
        );
    }
}
