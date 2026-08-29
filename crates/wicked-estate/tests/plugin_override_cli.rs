//! End-to-end plugin-override behaviour through the CLI, one FRESH PROCESS per invocation
//! (`CARGO_BIN_EXE_wicked-estate`), so multi-configuration flows are legal here — the in-process
//! registry is a `OnceLock` and could never see a second configuration (ADR-010 test rules).
//!
//! Covers: the loud broken-override fallback that must NOT delete previously-indexed files (the
//! inherited silent-deletion scar), the `plugin_overrides` digest cycle driven by SEMANTIC byte
//! edits (marker + descriptor diff + node-set change + key flip + revert-on-removal), the
//! `plugins list` override states, the label-scoped key, and (cc-gated) the grammar-tier bad
//! query fallback.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use wicked_estate_core::{GraphRead, NodeKind};
use wicked_estate_store::{GraphStoreMutExt, SqliteStore};

const BUILTIN_TS_QUERY: &str =
    include_str!("../../wicked-estate-extract/src/queries/typescript.scm");
const NAMESPACE_PATTERN: &str =
    "\n(internal_module\n  name: (identifier) @code_namespace.name\n) @code_namespace.def\n";
const TS_FIXTURE: &str = "namespace Util {\n  export function f(): void {}\n}\n";

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_wicked-estate")
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ci_plugov_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

/// Run the binary with the plugins dir pinned explicitly and the override env var controlled
/// (removed unless `armed` names languages) — every invocation is hermetic.
fn run(cwd: &Path, plugins: &Path, armed: Option<&str>, args: &[&str]) -> Output {
    let mut cmd = Command::new(bin());
    cmd.current_dir(cwd)
        .env("WICKED_ESTATE_PLUGINS", plugins)
        .env_remove("WICKED_ESTATE_PLUGIN_OVERRIDE")
        .args(args);
    if let Some(langs) = armed {
        cmd.env("WICKED_ESTATE_PLUGIN_OVERRIDE", langs);
    }
    cmd.output().expect("spawn wicked-estate")
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn write_ts_repo(root: &Path) -> PathBuf {
    let repo = root.join("repo");
    fs::create_dir_all(repo.join("src")).unwrap();
    fs::write(repo.join("src/a.ts"), TS_FIXTURE).unwrap();
    repo
}

fn write_query_override(plugins: &Path, dir: &str, lang: &str, query: &str) -> PathBuf {
    let d = plugins.join(dir);
    fs::create_dir_all(&d).unwrap();
    fs::write(
        d.join("plugin.toml"),
        format!("name = \"{dir}\"\nquery = \"q.scm\"\noverride_query = \"{lang}\"\n"),
    )
    .unwrap();
    fs::write(d.join("q.scm"), query).unwrap();
    d
}

/// Is a node of `kind` named `name` in the store? Opens and DROPS the store (the next subprocess
/// needs the file unlocked).
fn node_present(db: &Path, kind: &NodeKind, name: &str) -> bool {
    let store = SqliteStore::open(db).expect("open db");
    GraphRead::all_nodes(&store)
        .unwrap()
        .iter()
        .any(|n| &n.kind == kind && n.name == name)
}

fn meta(db: &Path, key: &str) -> Option<String> {
    let store = SqliteStore::open(db).expect("open db");
    store.meta_get_key(key)
}

// ── (1) Broken query-only override: loud fallback, NO deletions ──────────────────────────────

#[test]
fn broken_query_override_falls_back_loudly_and_deletes_nothing() {
    let root = scratch("broken_q");
    let repo = write_ts_repo(&root);
    let db = root.join("g.db");
    let empty = root.join("plugins-empty");
    fs::create_dir_all(&empty).unwrap();

    // Run 1: no override — built-in extraction indexes the file.
    let out = run(
        &root,
        &empty,
        None,
        &[
            "index",
            repo.to_str().unwrap(),
            "--db",
            db.to_str().unwrap(),
        ],
    );
    assert!(out.status.success(), "stderr={}", stderr_of(&out));
    assert!(node_present(&db, &NodeKind::Function, "f"));

    // Run 2: a BROKEN override for typescript. Must exit 0, warn loudly, keep the language
    // alive on the built-in query, and delete nothing — the pre-existing broken-query failure
    // mode (silent purge of previously-indexed files) must not be inherited.
    let plugins = root.join("plugins-broken");
    write_query_override(
        &plugins,
        "ts-broken",
        "typescript",
        "(no_such_node_kind) @code_function.def",
    );
    let out = run(
        &root,
        &plugins,
        None,
        &[
            "index",
            repo.to_str().unwrap(),
            "--db",
            db.to_str().unwrap(),
        ],
    );
    assert!(
        out.status.success(),
        "a broken override must not fail the run"
    );
    let err = stderr_of(&out);
    assert!(
        err.contains("QUERY-OVERRIDE:") && err.contains("failed to compile"),
        "the fallback must be loud; stderr={err}"
    );
    // A failed override is OUT of the effective set — same empty descriptor as run 1, so no
    // state change fires and nothing re-extracts, let alone deletes.
    assert!(
        !err.contains("PLUGIN-OVERRIDE state changed"),
        "a failed override must not enter the effective set; stderr={err}"
    );
    assert!(
        node_present(&db, &NodeKind::Function, "f"),
        "previously-indexed typescript must survive a broken override"
    );
    {
        let store = SqliteStore::open(&db).unwrap();
        assert!(
            store
                .indexed_files()
                .unwrap()
                .iter()
                .any(|f| f.ends_with("a.ts")),
            "the file row must survive"
        );
    }
}

// ── (2) Digest cycle: semantic edits force honest re-extraction ──────────────────────────────

#[test]
fn semantic_override_edits_force_reextraction_with_marker_and_descriptor_diff() {
    let root = scratch("digest_cycle");
    let repo = write_ts_repo(&root);
    let db = root.join("g.db");
    let plugins = root.join("plugins");
    let ovdir = write_query_override(
        &plugins,
        "ts-superset",
        "typescript",
        &format!("{BUILTIN_TS_QUERY}{NAMESPACE_PATTERN}"),
    );
    let index_args = [
        "index",
        repo.to_str().unwrap(),
        "--db",
        db.to_str().unwrap(),
    ];

    // Run 1: override active — announced per run, namespace captured, audit key stamped.
    let out = run(&root, &plugins, None, &index_args);
    assert!(out.status.success(), "stderr={}", stderr_of(&out));
    assert!(
        stderr_of(&out).contains("query override active: typescript <- ts-superset"),
        "every index run announces the active override; stderr={}",
        stderr_of(&out)
    );
    assert!(node_present(&db, &NodeKind::Namespace, "Util"));
    let key1 = meta(&db, "plugin_overrides").expect("audit key written at run end");
    assert!(
        key1.starts_with("typescript|query|ts-superset|"),
        "descriptor shape; got {key1}"
    );

    // Run 2: SEMANTIC byte edit (drop the namespace pattern — never a bare touch): the gate must
    // fire with the marker AND the old->new descriptor diff, and the node set must change even
    // though no source file changed.
    fs::write(ovdir.join("q.scm"), BUILTIN_TS_QUERY).unwrap();
    let out = run(&root, &plugins, None, &index_args);
    assert!(out.status.success());
    let err = stderr_of(&out);
    assert!(
        err.contains("PLUGIN-OVERRIDE state changed: forcing full re-extraction"),
        "stderr={err}"
    );
    assert!(
        err.lines()
            .any(|l| l.starts_with("- typescript|query|ts-superset|"))
            && err
                .lines()
                .any(|l| l.starts_with("+ typescript|query|ts-superset|")),
        "the old->new descriptor-line diff must print; stderr={err}"
    );
    assert!(
        !node_present(&db, &NodeKind::Namespace, "Util"),
        "re-extraction under the edited override must drop the namespace node"
    );
    let key2 = meta(&db, "plugin_overrides").expect("key rewritten");
    assert_ne!(key1, key2, "a semantic edit must flip the audit key");

    // Run 3: re-add the pattern — the gate re-fires, the namespace comes back.
    fs::write(
        ovdir.join("q.scm"),
        format!("{BUILTIN_TS_QUERY}{NAMESPACE_PATTERN}"),
    )
    .unwrap();
    let out = run(&root, &plugins, None, &index_args);
    assert!(out.status.success());
    assert!(stderr_of(&out).contains("PLUGIN-OVERRIDE state changed"));
    assert!(node_present(&db, &NodeKind::Namespace, "Util"));
    assert_eq!(
        meta(&db, "plugin_overrides").as_deref(),
        Some(key1.as_str())
    );

    // Run 4: remove the override dir — reverts to the built-in node set, no stale
    // override-minted nodes, empty descriptor stamped.
    fs::remove_dir_all(&ovdir).unwrap();
    let out = run(&root, &plugins, None, &index_args);
    assert!(out.status.success());
    assert!(stderr_of(&out).contains("PLUGIN-OVERRIDE state changed"));
    assert!(
        !node_present(&db, &NodeKind::Namespace, "Util"),
        "override removal must purge override-minted nodes"
    );
    assert!(
        node_present(&db, &NodeKind::Function, "f"),
        "the built-in extraction is back"
    );
    assert_eq!(
        meta(&db, "plugin_overrides").as_deref(),
        Some(""),
        "no active override -> empty descriptor"
    );
}

// ── (3) plugins list states ───────────────────────────────────────────────────────────────────

#[test]
fn plugins_list_shows_override_states() {
    let root = scratch("list");
    let plugins = root.join("plugins");
    // Active override, FAILED override (different language — not a duplicate), and a duplicate
    // pair — all query-only, no dylib needed.
    write_query_override(
        &plugins,
        "ts-superset",
        "typescript",
        &format!("{BUILTIN_TS_QUERY}{NAMESPACE_PATTERN}"),
    );
    write_query_override(&plugins, "tsx-broken", "tsx", "(no_such_node_kind) @x");
    let py_query =
        "(function_definition name: (identifier) @code_function.name) @code_function.def";
    write_query_override(&plugins, "pydup-a", "python", py_query);
    write_query_override(&plugins, "pydup-b", "python", py_query);

    let out = run(&root, &plugins, None, &["plugins", "list"]);
    assert!(out.status.success(), "stderr={}", stderr_of(&out));
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        stdout.contains("override=query(typescript)"),
        "active state missing; stdout={stdout}"
    );
    assert!(
        stdout.contains("override=query(tsx) FAILED") && stdout.contains("built-in in use"),
        "FAILED state missing; stdout={stdout}"
    );
    assert!(
        stdout.contains("override=query(python) DISABLED: duplicate of pydup-b")
            && stdout.contains("override=query(python) DISABLED: duplicate of pydup-a"),
        "DISABLED-duplicate states missing; stdout={stdout}"
    );

    // Grammar states need a real dylib (cc-gated, plugin_loader.rs skip pattern). Each list
    // invocation is a fresh process, so a separate plugins root avoids a cross-mode duplicate.
    let Some(gram) = build_nginx_override(&root.join("plugins-grammar")) else {
        eprintln!("SKIP: no `cc` available — grammar list states not exercised");
        return;
    };
    let out = run(&root, &gram, Some("typescript"), &["plugins", "list"]);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        stdout.contains("override=grammar(typescript) [armed]"),
        "armed state missing; stdout={stdout}"
    );
    let out = run(&root, &gram, None, &["plugins", "list"]);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        stdout.contains("override=grammar(typescript) [INERT"),
        "INERT state missing; stdout={stdout}"
    );
}

// ── (4) Label-scoped audit key ────────────────────────────────────────────────────────────────

#[test]
fn labelled_run_writes_the_scoped_key_only() {
    let root = scratch("label");
    let repo = write_ts_repo(&root);
    let db = root.join("g.db");
    let plugins = root.join("plugins");
    write_query_override(
        &plugins,
        "ts-superset",
        "typescript",
        &format!("{BUILTIN_TS_QUERY}{NAMESPACE_PATTERN}"),
    );

    let out = run(
        &root,
        &plugins,
        None,
        &[
            "index",
            repo.to_str().unwrap(),
            "--db",
            db.to_str().unwrap(),
            "--repo",
            "va",
        ],
    );
    assert!(out.status.success(), "stderr={}", stderr_of(&out));
    assert!(
        meta(&db, "repo:va:plugin_overrides").is_some(),
        "a labelled run writes the label-scoped key"
    );
    assert!(
        meta(&db, "plugin_overrides").is_none(),
        "a labelled run must not write the shared key — one repo's override state would answer \
         for every other repo's gate"
    );
}

// ── (5) Grammar-tier bad query: loud fallback, NO deletions (cc-gated) ───────────────────────

#[test]
fn grammar_tier_bad_query_falls_back_loudly_and_deletes_nothing() {
    let root = scratch("broken_g");
    let repo = write_ts_repo(&root);
    let db = root.join("g.db");
    let empty = root.join("plugins-empty");
    fs::create_dir_all(&empty).unwrap();

    // Run 1: built-in baseline.
    let out = run(
        &root,
        &empty,
        None,
        &[
            "index",
            repo.to_str().unwrap(),
            "--db",
            db.to_str().unwrap(),
        ],
    );
    assert!(out.status.success());
    assert!(node_present(&db, &NodeKind::Function, "f"));

    // Run 2: ARMED grammar override whose query does not compile against the plugin grammar —
    // must disarm loudly, keep the built-in pair, delete nothing.
    let Some(gram) = build_nginx_override(&root.join("plugins-grammar")) else {
        eprintln!("SKIP: no `cc` available — grammar-tier bad-query leg not built");
        return;
    };
    fs::write(
        gram.join("tsov").join("nginx.scm"),
        "(no_such_node_kind) @code_function.def",
    )
    .unwrap();
    let out = run(
        &root,
        &gram,
        Some("typescript"),
        &[
            "index",
            repo.to_str().unwrap(),
            "--db",
            db.to_str().unwrap(),
        ],
    );
    assert!(
        out.status.success(),
        "a broken grammar override must not fail the run"
    );
    let err = stderr_of(&out);
    assert!(
        err.contains("GRAMMAR-OVERRIDE:") && err.contains("failed to compile"),
        "the disarm must be loud; stderr={err}"
    );
    assert!(
        node_present(&db, &NodeKind::Function, "f"),
        "previously-indexed typescript must survive a broken grammar override"
    );
}

/// Build the nginx example grammar as an ARMED-shape override plugin dir (`tsov/`) under `root`:
/// dylib + nginx.scm + a manifest naming typescript with `override = true`. Returns the plugins
/// root, or `None` when no `cc` is available.
fn build_nginx_override(plugins_root: &Path) -> Option<PathBuf> {
    if Command::new("cc").arg("--version").output().is_err() {
        return None;
    }
    let example = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/plugins/nginx")
        .canonicalize()
        .expect("example plugin dir exists");
    let dir = plugins_root.join("tsov");
    fs::create_dir_all(&dir).unwrap();
    let lib_out = dir.join(format!("libnginx{}", std::env::consts::DLL_SUFFIX));
    let status = Command::new("cc")
        .args(["-shared", "-fPIC", "-O2", "-w", "-I"])
        .arg(example.join("src"))
        .arg("-o")
        .arg(&lib_out)
        .arg(example.join("src/parser.c"))
        .status()
        .expect("run cc");
    assert!(status.success(), "cc failed to build {lib_out:?}");
    fs::copy(example.join("nginx.scm"), dir.join("nginx.scm")).unwrap();
    fs::write(
        dir.join("plugin.toml"),
        "name = \"typescript\"\nlibrary = \"libnginx\"\nsymbol = \"tree_sitter_nginx\"\nextensions = [\"ts\"]\nquery = \"nginx.scm\"\noverride = true\n",
    )
    .unwrap();
    Some(plugins_root.to_path_buf())
}
