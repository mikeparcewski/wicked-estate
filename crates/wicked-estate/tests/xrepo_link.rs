//! End-to-end BUILD-GATEs for slice X1 (DES-S1 §4 G1.1/G1.7, DES-S2 §8 G2.1/G2.2/G2.5/G2.7/
//! G2.9/G2.10/G2.11): repos indexed with `--repo` into one project db, `wicked-estate link`, and the
//! crossings `blast-radius` reports.

use std::path::Path;
use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_wicked-estate")
}

fn run(args: &[&str]) -> (bool, String, String) {
    let out = Command::new(bin())
        .args(args)
        .output()
        .expect("run wicked-estate");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn git_init(dir: &Path, remote: Option<&str>) {
    let g = |args: &[&str]| {
        let ok = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .map(|o| o.status.success());
        assert!(ok.unwrap_or(false), "git {args:?}");
    };
    g(&["init", "-q"]);
    if let Some(r) = remote {
        g(&["remote", "add", "origin", r]);
    }
}

const PROV_REMOTE: &str = "https://github.com/acme/prov.git";

/// A fake credential, assembled at runtime so no scanner reads it as a committed secret.
fn planted() -> String {
    ["PLANTED", "xrepo", "0f9e8d7c6b5a4f3e"].concat()
}
const REG: &str = "https://registry.npmjs.org/acme-types/-/acme-types-1.2.0.tgz";

/// The G1.1 fixture: a provider, a `file:` consumer, an installed registry consumer, an undeclared
/// importer.
fn fixture(base: &Path) {
    let prov = base.join("prov");
    write(
        &prov,
        "package.json",
        r#"{"name":"acme-types","version":"1.2.0","types":"src/index.ts","repository":{"type":"git","url":"git+https://github.com/acme/prov.git"}}"#,
    );
    write(
        &prov,
        "src/index.ts",
        "export interface Order { id: string }\nexport function makeOrder(id: string): Order { return { id }; }\n",
    );
    git_init(&prov, Some(PROV_REMOTE));

    let cons = base.join("cons");
    write(
        &cons,
        "package.json",
        r#"{"name":"cons","dependencies":{"acme-types":"file:../prov"}}"#,
    );
    write(
        &cons,
        "src/app.ts",
        "import { makeOrder } from 'acme-types';\nexport function run() { return makeOrder('1'); }\n",
    );
    git_init(&cons, None);

    let cons3 = base.join("cons3");
    let lock = format!(
        r#"{{"lockfileVersion":3,"packages":{{"":{{}},"node_modules/acme-types":{{"version":"1.2.0","resolved":"{REG}","integrity":"sha512-AAAA"}}}}}}"#
    );
    write(
        &cons3,
        "package.json",
        r#"{"name":"cons3","dependencies":{"acme-types":"^1.0.0","other-dep":"^1.0.0"}}"#,
    );
    write(&cons3, "package-lock.json", &lock);
    write(
        &cons3,
        "node_modules/acme-types/package.json",
        r#"{"name":"acme-types","version":"1.2.0","repository":{"url":"git+https://github.com/acme/prov.git"}}"#,
    );
    write(&cons3, "node_modules/.package-lock.json", &lock);
    // An installed dependency whose manifest carries a (planted, fake) credential in its
    // repository URL: the index reads it to record identity and must store none of it.
    write(
        &cons3,
        "node_modules/other-dep/package.json",
        &format!(
            r#"{{"name":"other-dep","version":"1.0.0","repository":{{"url":"https://bob:{}@github.com/x/y.git"}}}}"#,
            planted()
        ),
    );
    write(&cons3, ".gitignore", "node_modules/\n");
    write(
        &cons3,
        "src/a.ts",
        "import type { Order } from 'acme-types';\nexport const x: Order | null = null;\n",
    );
    git_init(&cons3, None);

    let cons2 = base.join("cons2");
    write(&cons2, "package.json", r#"{"name":"cons2"}"#);
    write(
        &cons2,
        "src/b.ts",
        "import { makeOrder } from 'acme-types';\nexport const y = makeOrder('2');\n",
    );
    git_init(&cons2, None);
}

fn index(base: &Path, db: &str, label: &str, extra: &[&str]) {
    let dir = base.join(label);
    let mut args = vec!["index", dir.to_str().unwrap(), "--db", db, "--repo", label];
    args.extend_from_slice(extra);
    let (ok, out, err) = run(&args);
    assert!(ok, "index {label}: {out}{err}");
}

fn link(db: &str) -> serde_json::Value {
    let (ok, out, err) = run(&[
        "link",
        "--db",
        db,
        "--project",
        "p",
        "--labels",
        "prov,cons,cons3,cons2",
        "--json",
    ]);
    assert!(ok, "link: {out}{err}");
    serde_json::from_str(&out).unwrap_or_else(|e| panic!("link json {e}: {out}"))
}

fn blast(db: &str, name: &str) -> serde_json::Value {
    let (ok, out, err) = run(&["blast-radius", name, "--db", db, "--json"]);
    assert!(ok, "blast-radius: {out}{err}");
    serde_json::from_str(&out).unwrap_or_else(|e| panic!("blast json {e}: {out}"))
}

/// What the index recorded for a label's linker input (failure messages only).
fn evidence_dump(db: &str, label: &str) -> String {
    let conn = rusqlite::Connection::open(db).unwrap();
    let meta = |k: String| -> Option<String> {
        conn.query_row("SELECT v FROM meta WHERE k = ?1", [&k], |r| r.get(0))
            .ok()
    };
    let mut files = conn
        .prepare("SELECT path FROM files WHERE path LIKE ?1")
        .unwrap();
    let files: Vec<String> = files
        .query_map([format!("{label}/%")], |r| r.get(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    format!(
        "files={files:?}\ndep_locks={:?}\ndep_artifacts={:?}\nmeta_keys={:?}",
        meta(format!("repo:{label}:dep_locks")),
        meta(format!("repo:{label}:dep_artifacts")),
        conn.prepare("SELECT k FROM meta WHERE k LIKE 'repo:%'")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect::<Vec<_>>()
    )
}

fn dependent_files(v: &serde_json::Value) -> Vec<String> {
    v["dependents"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["file"].as_str().map(str::to_string))
        .collect()
}

#[test]
fn package_links_cross_repos_with_evidence_and_retract() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path();
    fixture(base);
    let db = base.join("g.db");
    let db = db.to_str().unwrap();
    for l in ["prov", "cons", "cons3", "cons2"] {
        index(base, db, l, &[]);
    }

    // G1.1: `file:` and registry-source link; unverified/undeclared do not.
    let rep = link(db);
    assert_eq!(rep["links"], 2, "{rep}\n{}", evidence_dump(db, "cons3"));
    assert_eq!(rep["links_by_identity"]["path"], 1, "{rep}");
    assert_eq!(rep["links_by_identity"]["registry-source"], 1, "{rep}");
    assert_eq!(rep["boundaries_by_reason"]["dep:undeclared"], 1, "{rep}");

    // The planted credential in an installed manifest is in no stored byte.
    for f in [db.to_string(), format!("{db}-wal")] {
        if let Ok(bytes) = std::fs::read(&f) {
            let hay = String::from_utf8_lossy(&bytes);
            assert!(!hay.contains(&planted()), "planted value stored in {f}");
        }
    }

    // G1.7 / G2.1: the provider entry's blast radius reaches the consumers, with crossings.
    let br = blast(db, "prov/src/index.ts");
    let files = dependent_files(&br);
    assert!(files.contains(&"cons/src/app.ts".to_string()), "{br}");
    assert!(files.contains(&"cons3/src/a.ts".to_string()), "{br}");
    // G2.10: the undeclared importer shares the import placeholder but never crosses.
    assert!(!files.contains(&"cons2/src/b.ts".to_string()), "{br}");
    let crossings = br["crossings"].as_array().unwrap();
    assert_eq!(crossings.len(), 2, "{br}");
    assert!(crossings.iter().all(|c| c["to"]["repo"] == "prov"));
    assert!(
        crossings
            .iter()
            .any(|c| c["evidence"]["identity"] == "registry-source")
    );
    assert_eq!(br["link"][0]["stale"], false, "{br}");

    // G1.8: an unchanged relink writes nothing new.
    let again = link(db);
    assert_eq!(again["replayed"], true, "{again}");
    assert_eq!(again["generation"], rep["generation"]);

    // G2.5: a member's re-index makes the link stale until relinked.
    index(base, db, "cons", &[]);
    let (_, st, _) = run(&["link", "--db", db, "--project", "p", "--status", "--json"]);
    let st: serde_json::Value = serde_json::from_str(&st).unwrap();
    assert_eq!(st["stale"], true, "{st}");
    assert!(
        st["stale_labels"]
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l == "cons"),
        "{st}"
    );

    // G2.2: links survive a forced provider re-index (read BEFORE relinking: the index alone must
    // not drop them), and the relink after it brings the project back to fresh.
    index(base, db, "prov", &["--force"]);
    assert_eq!(
        blast(db, "prov/src/index.ts")["crossings"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    link(db);

    // G2.9: a removed member, with its link live: relinking without it retracts that link only.
    let (ok, out, err) = run(&[
        "link",
        "--db",
        db,
        "--project",
        "p",
        "--labels",
        "prov,cons,cons2",
        "--json",
    ]);
    assert!(ok, "{out}{err}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["links"], 1, "{v}");
    let br = blast(db, "prov/src/index.ts");
    let crossings = br["crossings"].as_array().unwrap();
    assert_eq!(crossings.len(), 1, "{br}");
    assert_eq!(crossings[0]["from"]["repo"], "cons", "{br}");
    link(db);

    // Deleting the entry and relinking retracts.
    std::fs::remove_file(base.join("prov/src/index.ts")).unwrap();
    write(&base.join("prov"), "src/other.ts", "export const z = 1;\n");
    index(base, db, "prov", &[]);
    let gone = link(db);
    assert_eq!(gone["links"], 0, "{gone}");
    assert!(
        gone["boundaries_by_reason"]["link:entry-unmapped"]
            .as_u64()
            .unwrap_or(0)
            >= 1,
        "{gone}"
    );
}

/// G2.11: a label whose index did not complete is reported incomplete and not linked.
#[test]
fn an_incomplete_member_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path();
    fixture(base);
    let db = base.join("g.db");
    let db = db.to_str().unwrap();
    for l in ["prov", "cons"] {
        index(base, db, l, &[]);
    }
    let link2 = || {
        run(&[
            "link",
            "--db",
            db,
            "--project",
            "p",
            "--labels",
            "prov,cons",
            "--json",
        ])
    };
    let (ok, out, err) = link2();
    assert!(ok, "{out}{err}");
    // Simulate an index killed mid-run: its marker stays set.
    let conn = rusqlite::Connection::open(db).unwrap();
    conn.execute(
        "INSERT OR REPLACE INTO meta(k, v) VALUES('repo:cons:indexing', '99')",
        [],
    )
    .unwrap();
    drop(conn);
    let (ok, out, err) = link2();
    assert!(ok, "{out}{err}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["incomplete"][0], "cons", "{v}");
    assert_eq!(v["links"], 0, "{v}");
    // Not published: the project stays stale (never validated without the member) until the
    // member's index completes and link reruns.
    let (_, st, _) = run(&["link", "--db", db, "--project", "p", "--status", "--json"]);
    let st: serde_json::Value = serde_json::from_str(&st).unwrap();
    assert_eq!(st["stale"], true, "{st}");
    assert!(
        st["last_error"]
            .as_str()
            .is_some_and(|e| e.contains("incomplete")),
        "{st}"
    );
}

/// G2.8 at the command level: while another writer holds the database, `link` waits its budget and
/// is refused with WRITER-BUSY instead of interleaving.
#[test]
fn a_second_writer_command_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path();
    fixture(base);
    let db = base.join("g.db");
    let db = db.to_str().unwrap();
    index(base, db, "prov", &[]);
    let lock = format!(
        "{}.writer.lock",
        std::fs::canonicalize(db).unwrap().to_str().unwrap()
    );
    let holder = rusqlite::Connection::open(&lock).unwrap();
    holder.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let out = Command::new(bin())
        .args(["link", "--db", db, "--project", "p", "--labels", "prov"])
        .env("WICKED_ESTATE_WRITER_WAIT_SECS", "0")
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{err}");
    assert!(err.contains("WRITER-BUSY"), "{err}");
    drop(holder);
}

/// G2.7: a single-repo graph reports `crossings: []`.
#[test]
fn a_single_repo_graph_has_no_crossings() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path();
    fixture(base);
    let db = base.join("one.db");
    let db = db.to_str().unwrap();
    let (ok, out, err) = run(&["index", base.join("prov").to_str().unwrap(), "--db", db]);
    assert!(ok, "{out}{err}");
    let br = blast(db, "src/index.ts");
    assert_eq!(br["crossings"], serde_json::json!([]), "{br}");
}
