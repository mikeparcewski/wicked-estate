//! End-to-end: drop-in extra-edge rules (`.wicked-estate-extractors/*.toml`) discovered and run
//! by `index_path`. Exercises the archetype→playbook shape (the wicked-garden ADR-0005 port):
//! a hidden JSON catalog (invisible to the main hidden-filtered walk), synthetic archetype nodes,
//! edges landing on LITERAL playbook file nodes, the dangling-edge prune acting as the
//! file-existence guard, and a rules-edit forcing a full re-extract.

use std::fs;
use std::path::{Path, PathBuf};
use wicked_estate_core::{EdgeKind, GraphRead, NodeKind};
use wicked_estate_store::SqliteStore;

fn fresh_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ci_xedge_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join(".wicked-estate-extractors")).unwrap();
    fs::create_dir_all(d.join(".claude-plugin")).unwrap();
    fs::create_dir_all(d.join("skills/archetype/refs")).unwrap();
    d
}

const ARCHETYPE_RULES: &str = r#"
[[rule]]
name      = "archetype-declare"
file_glob = ".claude-plugin/archetypes.json"
pattern   = '(?m)^ {4}"(?P<name>[a-z][a-z0-9_-]*)":\s*\{'

[rule.emit_node]
id_template   = "archetype:{name}"
label_capture = "name"
kind          = "other:archetype"
node_scheme   = "archetype"

[rule.emit_edge]
kind               = "contains"
target_id_template = "archetype:{name}"
target_node_scheme = "archetype"

[[rule]]
name      = "archetype-playbook"
file_glob = ".claude-plugin/archetypes.json"
pattern   = '(?m)^ {4}"(?P<name>[a-z][a-z0-9_-]*)":\s*\{'

[rule.emit_edge]
kind               = "references"
source_id_template = "archetype:{name}"
source_node_scheme = "archetype"
target_kind        = "file"
target_id_template = "skills/archetype/refs/{name}.md"
"#;

const CATALOG: &str = r#"{
  "archetypes": {
    "triage": {
      "phases": ["classify"]
    },
    "build": {
      "phases": ["plan", "implement", "test", "review"]
    }
  }
}"#;

/// Write the standard fixture: rules + hidden catalog + ONE playbook (triage). `build`'s playbook
/// is deliberately missing — its edge must be pruned, not fabricated.
fn write_fixture(dir: &Path) {
    fs::write(
        dir.join(".wicked-estate-extractors/archetype.toml"),
        ARCHETYPE_RULES,
    )
    .unwrap();
    fs::write(dir.join(".claude-plugin/archetypes.json"), CATALOG).unwrap();
    fs::write(
        dir.join("skills/archetype/refs/triage.md"),
        "# triage playbook\n\nClassify the work and route.\n",
    )
    .unwrap();
}

fn archetype_nodes(store: &SqliteStore) -> Vec<String> {
    GraphRead::all_nodes(store)
        .unwrap()
        .into_iter()
        .filter(|n| n.kind == NodeKind::Other("archetype".to_string()))
        .map(|n| n.name)
        .collect()
}

#[test]
fn hidden_catalog_produces_archetype_nodes_and_playbook_file_edges() {
    let dir = fresh_dir("basic");
    write_fixture(&dir);

    let mut store = SqliteStore::in_memory().expect("open sqlite");
    wicked_estate::index_path(&mut store, &dir).expect("index_path");

    // Both archetype keys became synthetic nodes — the hidden catalog WAS seen.
    let mut names = archetype_nodes(&store);
    names.sort();
    assert_eq!(names, vec!["build", "triage"], "one node per catalog key");

    let edges = GraphRead::all_edges(&store).unwrap();

    // The catalog file contains the archetype nodes.
    assert!(
        edges.iter().any(|e| e.kind == EdgeKind::Contains
            && e.source.as_str().contains("archetypes.json")
            && e.target.as_str().contains("archetype:triage")),
        "catalog → archetype Contains edge must land"
    );

    // triage's playbook exists on disk → the References edge lands on the LITERAL file node.
    assert!(
        edges.iter().any(|e| e.kind == EdgeKind::References
            && e.source.as_str().contains("archetype:triage")
            && e.target
                .as_str()
                .contains("skills/archetype/refs/triage.md")),
        "archetype:triage → playbook FILE edge must land, got {:?}",
        edges
            .iter()
            .filter(|e| e.kind == EdgeKind::References)
            .map(|e| (e.source.as_str(), e.target.as_str()))
            .collect::<Vec<_>>()
    );

    // build's playbook does NOT exist → its edge dangles and MUST be pruned (the
    // file-existence guard: flag via the node, never fabricate the relationship).
    assert!(
        !edges.iter().any(
            |e| e.kind == EdgeKind::References && e.source.as_str().contains("archetype:build")
        ),
        "missing playbook must not produce a References edge"
    );
}

#[test]
fn blast_radius_on_playbook_surfaces_the_archetype() {
    let dir = fresh_dir("blast");
    write_fixture(&dir);

    let mut store = SqliteStore::in_memory().expect("open sqlite");
    wicked_estate::index_path(&mut store, &dir).expect("index_path");

    // The user-facing promise: "what breaks if I change the playbook?" reaches the archetype
    // (and, transitively, the catalog) — the relationship no grep can see.
    let deps = wicked_estate::blast_radius_by_name(&store, "skills/archetype/refs/triage.md", 4)
        .expect("blast radius");
    let names: Vec<&str> = deps.iter().map(|n| n.name.as_str()).collect();
    assert!(
        names.contains(&"triage"),
        "archetype must be a dependent of its playbook, got {names:?}"
    );
    assert!(
        names.iter().any(|n| n.contains("archetypes.json")),
        "catalog file must be a transitive dependent, got {names:?}"
    );
}

#[test]
fn catalog_edit_reindex_drops_removed_archetype() {
    let dir = fresh_dir("edit");
    write_fixture(&dir);

    let mut store = SqliteStore::in_memory().expect("open sqlite");
    wicked_estate::index_path(&mut store, &dir).expect("first index");
    assert_eq!(archetype_nodes(&store).len(), 2);

    // Remove `build` from the catalog and re-index (incremental — only the catalog changed).
    fs::write(
        dir.join(".claude-plugin/archetypes.json"),
        r#"{
  "archetypes": {
    "triage": {
      "phases": ["classify"]
    }
  }
}"#,
    )
    .unwrap();
    wicked_estate::index_path(&mut store, &dir).expect("second index");

    let names = archetype_nodes(&store);
    assert_eq!(names, vec!["triage"], "removed archetype must disappear");
    let edges = GraphRead::all_edges(&store).unwrap();
    assert!(
        !edges
            .iter()
            .any(|e| e.source.as_str().contains("archetype:build")
                || e.target.as_str().contains("archetype:build")),
        "no edge may still reference the removed archetype"
    );
}

#[test]
fn rules_edit_forces_full_reextract_and_replaces_old_edges() {
    let dir = fresh_dir("rules_edit");
    write_fixture(&dir);

    let mut store = SqliteStore::in_memory().expect("open sqlite");
    wicked_estate::index_path(&mut store, &dir).expect("first index");
    assert!(
        GraphRead::all_edges(&store)
            .unwrap()
            .iter()
            .any(|e| e.kind == EdgeKind::References),
        "first rule set emits References edges"
    );

    // Edit ONLY the rules (the catalog is unchanged): switch the playbook edge kind.
    fs::write(
        dir.join(".wicked-estate-extractors/archetype.toml"),
        ARCHETYPE_RULES.replace(
            "kind               = \"references\"",
            "kind               = \"other:steers\"",
        ),
    )
    .unwrap();
    wicked_estate::index_path(&mut store, &dir).expect("second index");

    let edges = GraphRead::all_edges(&store).unwrap();
    assert!(
        edges
            .iter()
            .any(|e| e.kind == EdgeKind::Other("steers".to_string())),
        "new rule kind must be applied even though no source file changed"
    );
    assert!(
        !edges
            .iter()
            .any(|e| e.kind == EdgeKind::References && e.source.as_str().contains("archetype:")),
        "edges from the OLD rule set must be purged"
    );
}

#[test]
fn repo_without_rules_is_unaffected() {
    let dir = fresh_dir("no_rules");
    // No .wicked-estate-extractors dir at all; a hidden catalog alone must stay invisible.
    fs::remove_dir_all(dir.join(".wicked-estate-extractors")).unwrap();
    fs::write(dir.join(".claude-plugin/archetypes.json"), CATALOG).unwrap();
    fs::write(dir.join("skills/archetype/refs/triage.md"), "# triage\n").unwrap();

    let mut store = SqliteStore::in_memory().expect("open sqlite");
    wicked_estate::index_path(&mut store, &dir).expect("index_path");

    assert!(
        archetype_nodes(&store).is_empty(),
        "no rules → no synthetic archetype nodes"
    );
    assert!(
        !GraphRead::all_nodes(&store)
            .unwrap()
            .iter()
            .any(|n| n.name.contains("archetypes.json")),
        "hidden files stay un-indexed when no rule targets them"
    );
}
