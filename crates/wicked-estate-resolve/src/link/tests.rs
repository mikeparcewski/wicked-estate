//! BUILD-GATE tests of the linker's computation (DES-S1 §4: G1.1, G1.2, G1.9-style, G1.10, G1.11;
//! cargo and python path links; identity normalization). Fixtures are in-memory label inputs.

use super::*;

fn label(name: &str, remote: Option<&str>, files: &[(&str, &str)], code: &[&str]) -> LabelInput {
    let mut li = LabelInput {
        label: name.into(),
        root: Some(format!("/w/{name}")),
        remote: remote.map(str::to_string),
        ..Default::default()
    };
    for (p, t) in files {
        let path = format!("{name}/{p}");
        li.files.insert(path.clone());
        li.manifests.insert(path, t.to_string());
    }
    for p in code {
        li.files.insert(format!("{name}/{p}"));
    }
    li
}

fn import(li: &mut LabelInput, file: &str, raw: &str) {
    let f = format!("{}/{file}", li.label);
    li.files.insert(f.clone());
    li.imports.push(ImportRef {
        file: f,
        raw: raw.into(),
    });
}

const PROV_REMOTE: &str = "git@github.com:acme/prov.git";

fn prov() -> LabelInput {
    label(
        "prov",
        Some(PROV_REMOTE),
        &[(
            "package.json",
            r#"{"name":"acme-types","version":"1.2.0","types":"src/index.ts","repository":{"url":"git+https://github.com/acme/prov.git"}}"#,
        )],
        &["src/index.ts"],
    )
}

fn lock(pkg_key: &str, version: &str, resolved: &str, integrity: &str) -> String {
    format!(
        r#"{{"lockfileVersion":3,"packages":{{"":{{}},"node_modules/{pkg_key}":{{"version":"{version}","resolved":"{resolved}","integrity":"{integrity}"}}}}}}"#
    )
}

fn has_edge(out: &LinkOutput, from: &str, to: &str) -> Option<Identity> {
    out.edges
        .iter()
        .find(|e| e.from_file == from && e.to_file == to)
        .map(|e| e.identity)
}

fn boundary_of<'a>(out: &'a LinkOutput, label: &str) -> Option<&'a Boundary> {
    out.boundaries.iter().find(|b| b.consumer_label == label)
}

/// G1.1: path (link), registry-source (link), no lockfile (unverified), undeclared, stale install.
#[test]
fn g1_1_provider_identity_rules() {
    let mut cons = label(
        "cons",
        None,
        &[(
            "package.json",
            r#"{"name":"cons","dependencies":{"acme-types":"file:../prov"}}"#,
        )],
        &[],
    );
    import(&mut cons, "src/app.ts", "'acme-types'");

    let reg = "https://registry.npmjs.org/acme-types/-/acme-types-1.2.0.tgz";
    let mut cons3 = label(
        "cons3",
        None,
        &[
            (
                "package.json",
                r#"{"name":"cons3","dependencies":{"acme-types":"^1.0.0"}}"#,
            ),
            (
                "package-lock.json",
                &lock("acme-types", "1.2.0", reg, "sha512-AAA"),
            ),
        ],
        &[],
    );
    cons3.artifacts.push(DepArtifact {
        manifest_dir: "cons3".into(),
        package: "acme-types".into(),
        name: Some("acme-types".into()),
        tree_record: true,
        version: Some("1.2.0".into()),
        repository: Some("git+https://github.com/acme/prov.git".into()),
        directory: None,
        tree_version: Some("1.2.0".into()),
        tree_resolved: Some(reg.into()),
        tree_integrity: Some("sha512-AAA".into()),
    });
    import(&mut cons3, "src/a.ts", "\"acme-types\"");

    let mut cons4 = label(
        "cons4",
        None,
        &[(
            "package.json",
            r#"{"name":"cons4","dependencies":{"acme-types":"^1.0.0"}}"#,
        )],
        &[],
    );
    import(&mut cons4, "src/a.ts", "'acme-types'");

    let mut cons2 = label(
        "cons2",
        None,
        &[("package.json", r#"{"name":"cons2"}"#)],
        &[],
    );
    import(&mut cons2, "src/b.ts", "'acme-types'");

    let mut cons5 = cons3.clone();
    cons5.label = "cons5".into();
    cons5.manifests = cons3
        .manifests
        .iter()
        .map(|(k, v)| (k.replacen("cons3", "cons5", 1), v.clone()))
        .collect();
    cons5.files = cons3
        .files
        .iter()
        .map(|k| k.replacen("cons3", "cons5", 1))
        .collect();
    cons5.imports = vec![ImportRef {
        file: "cons5/src/a.ts".into(),
        raw: "'acme-types'".into(),
    }];
    cons5.artifacts[0].manifest_dir = "cons5".into();
    cons5.artifacts[0].tree_integrity = Some("sha512-STALE".into());

    let out = link(&[prov(), cons, cons3, cons4, cons2, cons5]);
    assert_eq!(
        has_edge(&out, "cons/src/app.ts", "prov/src/index.ts"),
        Some(Identity::Path)
    );
    assert_eq!(
        has_edge(&out, "cons3/src/a.ts", "prov/src/index.ts"),
        Some(Identity::RegistrySource)
    );
    assert!(
        out.edges
            .iter()
            .all(|e| !["cons2", "cons4", "cons5"].contains(&e.consumer_label.as_str())),
        "{:?}",
        out.edges
    );
    assert_eq!(
        boundary_of(&out, "cons4").map(|b| b.reason),
        Some(BoundaryReason::ProviderUnverified)
    );
    assert_eq!(
        boundary_of(&out, "cons5").map(|b| b.reason),
        Some(BoundaryReason::ProviderUnverified)
    );
    // Every possible item says why, so a person can act on it.
    assert_eq!(
        boundary_of(&out, "cons5").map(|b| b.why.clone()),
        Some(vec![
            "registry: the installed copy is not the one the lockfile resolves (stale install)"
                .to_string()
        ])
    );
    assert_eq!(
        boundary_of(&out, "cons4").map(|b| b.why.clone()),
        Some(vec![
            "registry: no lockfile entry for the dependency".to_string()
        ])
    );
    assert_eq!(
        boundary_of(&out, "cons2").map(|b| b.reason),
        Some(BoundaryReason::Undeclared)
    );
    let e = out
        .edges
        .iter()
        .find(|e| e.consumer_label == "cons3")
        .unwrap();
    assert_eq!(e.consumer_version.as_deref(), Some("1.2.0"));
    assert!(!e.skew);
}

/// No installed-tree record: registry-source(version-only).
#[test]
fn registry_source_without_tree_record_is_version_only() {
    let reg = "https://registry.npmjs.org/acme-types/-/acme-types-1.1.0.tgz";
    let mut c = label(
        "c",
        None,
        &[
            (
                "package.json",
                r#"{"name":"c","dependencies":{"acme-types":"^1.0.0"}}"#,
            ),
            (
                "package-lock.json",
                &lock("acme-types", "1.1.0", reg, "sha512-B"),
            ),
        ],
        &[],
    );
    c.artifacts.push(DepArtifact {
        manifest_dir: "c".into(),
        package: "acme-types".into(),
        name: Some("acme-types".into()),
        version: Some("1.1.0".into()),
        repository: Some("https://github.com/acme/prov".into()),
        ..Default::default()
    });
    import(&mut c, "x.ts", "'acme-types'");
    let out = link(&[prov(), c]);
    let e = out
        .edges
        .iter()
        .find(|e| e.consumer_label == "c")
        .expect("linked");
    assert_eq!(e.identity, Identity::RegistrySourceVersionOnly);
    assert!(e.skew, "1.1.0 consumer vs 1.2.0 provider");
}

/// G1.2: two labels provide the same name; an unverified dependency lists both, no edge.
#[test]
fn g1_2_two_providers_no_edge() {
    let mut prov2 = prov();
    prov2.label = "prov2".into();
    prov2.remote = Some("https://github.com/other/prov2".into());
    prov2.manifests = prov()
        .manifests
        .iter()
        .map(|(k, v)| (k.replacen("prov", "prov2", 1), v.clone()))
        .collect();
    prov2.files = prov()
        .files
        .iter()
        .map(|k| k.replacen("prov", "prov2", 1))
        .collect();
    let mut c = label(
        "c",
        None,
        &[(
            "package.json",
            r#"{"name":"c","dependencies":{"acme-types":"^1.0.0"}}"#,
        )],
        &[],
    );
    import(&mut c, "x.ts", "'acme-types'");
    let out = link(&[prov(), prov2, c]);
    assert!(out.edges.is_empty());
    let b = boundary_of(&out, "c").unwrap();
    assert_eq!(b.candidates.len(), 2, "{b:?}");
}

/// G1.10: npm alias links with a registry chain; a contradicting lockfile git URL, no lockfile,
/// and a same-named private package without a repository chain never link.
#[test]
fn g1_10_alias_and_negative_controls() {
    let reg = "https://registry.npmjs.org/acme-types/-/acme-types-1.2.0.tgz";
    let mut a = label(
        "a",
        None,
        &[
            (
                "package.json",
                r#"{"name":"a","dependencies":{"acme":"npm:acme-types@^1"}}"#,
            ),
            ("package-lock.json", &lock("acme", "1.2.0", reg, "sha512-C")),
        ],
        &[],
    );
    a.artifacts.push(DepArtifact {
        manifest_dir: "a".into(),
        package: "acme".into(),
        name: Some("acme-types".into()),
        tree_record: true,
        version: Some("1.2.0".into()),
        repository: Some("github:acme/prov".into()),
        tree_version: Some("1.2.0".into()),
        tree_resolved: Some(reg.into()),
        tree_integrity: Some("sha512-C".into()),
        ..Default::default()
    });
    import(&mut a, "x.ts", "'acme'");
    let mut g = label(
        "g",
        None,
        &[
            (
                "package.json",
                r#"{"name":"g","dependencies":{"acme-types":"^1.0.0"}}"#,
            ),
            (
                "package-lock.json",
                &lock(
                    "acme-types",
                    "1.2.0",
                    "git+ssh://git@github.com/someone/else.git#abc",
                    "",
                ),
            ),
        ],
        &[],
    );
    import(&mut g, "x.ts", "'acme-types'");
    let mut n = label(
        "n",
        None,
        &[(
            "package.json",
            r#"{"name":"n","dependencies":{"acme-types":"^1.0.0"}}"#,
        )],
        &[],
    );
    import(&mut n, "x.ts", "'acme-types'");
    let reg2 = "https://npm.private.example/acme-types/-/acme-types-1.2.0.tgz";
    let mut pr = label(
        "pr",
        None,
        &[
            (
                "package.json",
                r#"{"name":"pr","dependencies":{"acme-types":"^1.0.0"}}"#,
            ),
            (
                "package-lock.json",
                &lock("acme-types", "1.2.0", reg2, "sha512-D"),
            ),
        ],
        &[],
    );
    pr.artifacts.push(DepArtifact {
        manifest_dir: "pr".into(),
        package: "acme-types".into(),
        name: Some("acme-types".into()),
        tree_record: true,
        version: Some("1.2.0".into()),
        tree_version: Some("1.2.0".into()),
        tree_resolved: Some(reg2.into()),
        tree_integrity: Some("sha512-D".into()),
        ..Default::default()
    });
    import(&mut pr, "x.ts", "'acme-types'");
    let out = link(&[prov(), a, g, n, pr]);
    assert_eq!(
        has_edge(&out, "a/x.ts", "prov/src/index.ts"),
        Some(Identity::RegistrySource)
    );
    assert_eq!(
        boundary_of(&out, "g").map(|b| b.reason),
        Some(BoundaryReason::ProviderContradicted)
    );
    assert_eq!(
        boundary_of(&out, "n").map(|b| b.reason),
        Some(BoundaryReason::ProviderUnverified)
    );
    assert_eq!(
        boundary_of(&out, "pr").map(|b| b.reason),
        Some(BoundaryReason::ProviderUnverified)
    );
    assert_eq!(out.edges.len(), 1);
}

fn built_prov(build: &str) -> LabelInput {
    label(
        "bp",
        None,
        &[
            (
                "package.json",
                &format!(
                    r#"{{"name":"built","version":"2.0.0","types":"dist/index.d.ts","scripts":{{"build":"{build}"}}}}"#
                ),
            ),
            (
                "tsconfig.json",
                r#"{"compilerOptions":{"outDir":"dist","rootDir":"src"}}"#,
            ),
        ],
        &["src/index.ts"],
    )
}

/// G1.9-style: a plain `tsc` build maps `dist/index.d.ts` to `src/index.ts`.
#[test]
fn g1_9_tsconfig_outdir_maps_a_tsc_build() {
    let mut c = label(
        "c",
        None,
        &[(
            "package.json",
            r#"{"name":"c","dependencies":{"built":"file:../bp"}}"#,
        )],
        &[],
    );
    import(&mut c, "x.ts", "'built'");
    let out = link(&[built_prov("tsc -p tsconfig.json"), c]);
    assert_eq!(
        has_edge(&out, "c/x.ts", "bp/src/index.ts"),
        Some(Identity::Path)
    );
}

/// G1.11: a bundler build never maps, even when the rewritten path exists.
#[test]
fn g1_11_bundler_build_is_entry_unmapped() {
    let mut c = label(
        "c",
        None,
        &[(
            "package.json",
            r#"{"name":"c","dependencies":{"built":"file:../bp"}}"#,
        )],
        &[],
    );
    import(&mut c, "x.ts", "'built'");
    let out = link(&[built_prov("tsup src/index.ts --dts"), c]);
    assert!(out.edges.is_empty());
    assert_eq!(
        boundary_of(&out, "c").map(|b| b.reason),
        Some(BoundaryReason::EntryUnmapped)
    );
}

/// npm workspaces: a lock `link: true` entry is path identity; exports subpaths map.
#[test]
fn workspace_link_and_exports_subpath() {
    let mut ws = label(
        "mono",
        None,
        &[
            (
                "package.json",
                r#"{"name":"root","workspaces":["packages/*"]}"#,
            ),
            (
                "package-lock.json",
                r#"{"packages":{"node_modules/@acme/core":{"resolved":"packages/core","link":true}}}"#,
            ),
            (
                "packages/core/package.json",
                r#"{"name":"@acme/core","version":"0.1.0","exports":{".":{"types":"./src/index.ts"},"./util":{"import":"./src/util.ts"}}}"#,
            ),
            (
                "packages/app/package.json",
                r#"{"name":"@acme/app","dependencies":{"@acme/core":"^0.1.0"}}"#,
            ),
        ],
        &["packages/core/src/index.ts", "packages/core/src/util.ts"],
    );
    import(&mut ws, "packages/app/src/a.ts", "'@acme/core'");
    import(&mut ws, "packages/app/src/b.ts", "'@acme/core/util'");
    let out = link(&[ws]);
    assert_eq!(
        has_edge(
            &out,
            "mono/packages/app/src/a.ts",
            "mono/packages/core/src/index.ts"
        ),
        Some(Identity::Path)
    );
    assert_eq!(
        has_edge(
            &out,
            "mono/packages/app/src/b.ts",
            "mono/packages/core/src/util.ts"
        ),
        Some(Identity::Path)
    );
}

/// Cargo: path deps, `package =` renames, `[lib] path`, workspace-inherited deps; registry deps
/// are unverified.
#[test]
fn cargo_path_rename_and_workspace() {
    let mut ws = label(
        "rs",
        None,
        &[
            (
                "Cargo.toml",
                "[workspace]\nmembers = [\"crates/*\"]\n[workspace.dependencies]\ncore-lib = { path = \"crates/core\" }\n",
            ),
            (
                "crates/core/Cargo.toml",
                "[package]\nname = \"core-lib\"\nversion = \"0.3.0\"\n[lib]\nname = \"corelib\"\npath = \"src/top.rs\"\n",
            ),
            (
                "crates/app/Cargo.toml",
                "[package]\nname = \"app\"\n[dependencies]\nmine = { path = \"../core\", package = \"core-lib\" }\ncore-lib = { workspace = true }\nserde = \"1\"\n",
            ),
        ],
        &["crates/core/src/top.rs"],
    );
    import(&mut ws, "crates/app/src/main.rs", "mine::thing");
    import(&mut ws, "crates/app/src/other.rs", "corelib::x");
    // An unrenamed dependency with a custom `[lib] name` is not importable by its package name.
    import(&mut ws, "crates/app/src/wrong.rs", "core_lib::x");
    import(&mut ws, "crates/app/src/s.rs", "serde::Serialize");
    import(&mut ws, "crates/app/src/c.rs", "crate::y");
    let out = link(&[ws]);
    assert_eq!(
        has_edge(
            &out,
            "rs/crates/app/src/main.rs",
            "rs/crates/core/src/top.rs"
        ),
        Some(Identity::Path)
    );
    assert_eq!(
        has_edge(
            &out,
            "rs/crates/app/src/other.rs",
            "rs/crates/core/src/top.rs"
        ),
        Some(Identity::Path)
    );
    assert_eq!(out.edges.len(), 2, "{:?}", out.edges);
}

/// Cargo across labels: a path dep that leaves the label resolves through the recorded roots.
#[test]
fn cargo_path_across_labels() {
    let prov = label(
        "p",
        None,
        &[(
            "Cargo.toml",
            "[package]\nname = \"pcrate\"\nversion = \"1.0.0\"\n",
        )],
        &["src/lib.rs"],
    );
    let mut cons = label(
        "q",
        None,
        &[(
            "Cargo.toml",
            "[package]\nname = \"q\"\n[dependencies]\npcrate = { path = \"../p\" }\n",
        )],
        &[],
    );
    import(&mut cons, "src/main.rs", "pcrate::f");
    let out = link(&[prov, cons]);
    assert_eq!(
        has_edge(&out, "q/src/main.rs", "p/src/lib.rs"),
        Some(Identity::Path)
    );
}

/// Python: uv path sources link to the most specific module; an undeclared import does not.
#[test]
fn python_uv_path_source() {
    let prov = label(
        "core",
        None,
        &[(
            "pyproject.toml",
            "[project]\nname = \"acme-core\"\nversion = \"0.2.0\"\n",
        )],
        &["src/acme_core/__init__.py", "src/acme_core/models.py"],
    );
    let mut app = label(
        "app",
        None,
        &[(
            "pyproject.toml",
            "[project]\nname = \"app\"\ndependencies = [\"acme-core>=0.1\"]\n[tool.uv.sources]\nacme-core = { path = \"../core\" }\n",
        )],
        &[],
    );
    import(&mut app, "app/main.py", "acme_core.models");
    let mut other = label(
        "other",
        None,
        &[("pyproject.toml", "[project]\nname = \"other\"\n")],
        &[],
    );
    import(&mut other, "o.py", "acme_core");
    let out = link(&[prov, app, other]);
    assert_eq!(
        has_edge(&out, "app/app/main.py", "core/src/acme_core/models.py"),
        Some(Identity::Path)
    );
    assert_eq!(
        boundary_of(&out, "other").map(|b| b.reason),
        Some(BoundaryReason::Undeclared)
    );
}

#[test]
fn repo_identity_normalizes_and_refuses_masked() {
    for u in [
        "git+https://github.com/Acme/prov.git",
        "https://user:pw@github.com/Acme/prov",
        "git@github.com:Acme/prov.git",
        "github:Acme/prov",
        "ssh://git@github.com/Acme/prov.git#v1",
    ] {
        assert_eq!(
            repo_identity(u).as_deref(),
            Some("github.com/Acme/prov"),
            "{u}"
        );
    }
    assert_eq!(repo_identity("https://github.com/REDACTEDRE/prov"), None);
    assert_eq!(repo_identity("1.2.3"), None);
}

#[test]
fn fingerprint_is_stable_and_input_sensitive() {
    let a = fingerprint(&[prov()]);
    assert_eq!(a, fingerprint(&[prov()]));
    let mut p = prov();
    p.files.insert("prov/src/new.ts".into());
    assert_ne!(a, fingerprint(&[p]));
}

// ── Review round 1 (X1): one test per finding class ───────────────────────────────────────────

fn reason_of(out: &LinkOutput, label: &str) -> Option<BoundaryReason> {
    boundary_of(out, label).map(|b| b.reason)
}

/// C3: a non-GitHub host keeps the whole repository path (GitLab subgroups); GitHub views truncate.
#[test]
fn repo_identity_keeps_subgroups_and_drops_views() {
    assert_ne!(
        repo_identity("https://gitlab.example/group/sub/a.git"),
        repo_identity("https://gitlab.example/group/sub/b.git")
    );
    assert_eq!(
        repo_identity("https://gitlab.example/group/sub/a/-/tree/main").as_deref(),
        Some("gitlab.example/group/sub/a")
    );
    assert_eq!(
        repo_identity("https://github.com/o/r/tree/main/packages/x").as_deref(),
        Some("github.com/o/r")
    );
}

/// S1: malformed `npm:` aliases declare nothing and never panic.
#[test]
fn malformed_npm_alias_is_ignored_without_panicking() {
    let m: serde_json::Value = serde_json::from_str(
        r#"{"dependencies":{"a":"npm:","b":"npm:@","c":"npm:💥","d":"npm:@s/real@1","e":"npm:real@1"}}"#,
    )
    .unwrap();
    let d = npm_declared(&m);
    assert!(!d.contains_key("a") && !d.contains_key("b"), "{d:?}");
    assert_eq!(d["c"].0, "💥");
    assert_eq!(d["d"].0, "@s/real");
    assert_eq!(d["e"].0, "real");
}

/// C10: `exports` is authoritative; a root import it does not export never falls back to `main`.
#[test]
fn exports_without_root_never_falls_back_to_main() {
    let p = label(
        "p",
        None,
        &[(
            "package.json",
            r#"{"name":"x","exports":{"./sub":"./sub.js"},"main":"./decoy.js"}"#,
        )],
        &["sub.js", "decoy.js"],
    );
    let mut c = label(
        "c",
        None,
        &[(
            "package.json",
            r#"{"name":"c","dependencies":{"x":"file:../p"}}"#,
        )],
        &[],
    );
    import(&mut c, "a.ts", "'x'");
    import(&mut c, "b.ts", "'x/sub'");
    let out = link(&[p, c]);
    assert_eq!(has_edge(&out, "c/a.ts", "p/decoy.js"), None);
    assert_eq!(has_edge(&out, "c/b.ts", "p/sub.js"), Some(Identity::Path));
    assert_eq!(reason_of(&out, "c"), Some(BoundaryReason::EntryUnmapped));
}

/// C8: only one plain `tsc` (after cleanup) is build evidence, and it names its config.
#[test]
fn tsc_build_evidence_is_exact() {
    assert_eq!(tsc_build_config("tsc").as_deref(), Some("tsconfig.json"));
    assert_eq!(
        tsc_build_config("tsc -p tsconfig.build.json").as_deref(),
        Some("tsconfig.build.json")
    );
    assert_eq!(
        tsc_build_config("rimraf dist && tsc -b").as_deref(),
        Some("tsconfig.json")
    );
    assert_eq!(
        tsc_build_config("tsc --build cfg/tsconfig.json").as_deref(),
        Some("cfg/tsconfig.json")
    );
    for bad in [
        "echo tsc",
        "tsc --noEmit",
        "tsc --outDir=lib",
        "tsc && tsup src/index.ts",
        "npm run gen && tsc",
        "tsc; node x",
        "tsc && tsc -p other.json",
    ] {
        assert_eq!(tsc_build_config(bad), None, "{bad}");
    }
}

/// C8/C9: the build's own config is used (not a lexically earlier decoy), and an inherited `outDir`
/// is relative to the config that sets it.
#[test]
fn tsconfig_chosen_by_build_and_extends_resolved_at_its_origin() {
    let prov = label(
        "bp",
        None,
        &[
            (
                "package.json",
                r#"{"name":"built","types":"dist/index.d.ts","scripts":{"build":"tsc -p tsconfig.release.json"}}"#,
            ),
            (
                "tsconfig.a-decoy.json",
                r#"{"compilerOptions":{"outDir":"dist","rootDir":"decoy"}}"#,
            ),
            (
                "tsconfig.release.json",
                r#"{"compilerOptions":{"outDir":"dist","rootDir":"src"}}"#,
            ),
        ],
        &["src/index.ts", "decoy/index.ts"],
    );
    let mut c = label(
        "c",
        None,
        &[(
            "package.json",
            r#"{"name":"c","dependencies":{"built":"file:../bp"}}"#,
        )],
        &[],
    );
    import(&mut c, "x.ts", "'built'");
    let out = link(&[prov, c.clone()]);
    assert_eq!(
        has_edge(&out, "c/x.ts", "bp/src/index.ts"),
        Some(Identity::Path)
    );

    // `extends ../base/tsconfig.json` whose outDir is `dist`: that is base/dist, not bp/dist.
    let based = label(
        "bp",
        None,
        &[
            (
                "package.json",
                r#"{"name":"built","types":"dist/index.d.ts","scripts":{"build":"tsc"}}"#,
            ),
            ("tsconfig.json", r#"{"extends":"./base/tsconfig.json"}"#),
            (
                "base/tsconfig.json",
                r#"{"compilerOptions":{"outDir":"dist","rootDir":"src"}}"#,
            ),
        ],
        &["src/index.ts"],
    );
    let out = link(&[based, c]);
    assert!(out.edges.is_empty(), "{:?}", out.edges);
    assert_eq!(reason_of(&out, "c"), Some(BoundaryReason::EntryUnmapped));
}

/// No `rootDir`: the common directory of the package's sources (what `tsc` uses).
#[test]
fn tsc_without_root_dir_uses_the_common_source_root() {
    let prov = label(
        "bp",
        None,
        &[
            (
                "package.json",
                r#"{"name":"built","types":"dist/index.d.ts","scripts":{"build":"tsc"}}"#,
            ),
            ("tsconfig.json", r#"{"compilerOptions":{"outDir":"dist"}}"#),
        ],
        &["src/index.ts", "src/util/a.ts"],
    );
    let mut c = label(
        "c",
        None,
        &[(
            "package.json",
            r#"{"name":"c","dependencies":{"built":"file:../bp"}}"#,
        )],
        &[],
    );
    import(&mut c, "x.ts", "'built'");
    let out = link(&[prov, c]);
    assert_eq!(
        has_edge(&out, "c/x.ts", "bp/src/index.ts"),
        Some(Identity::Path)
    );
}

/// C2: `workspace:` links only a MEMBER of the consumer's workspace; a same-named package outside
/// the member patterns (an example) is not one.
#[test]
fn workspace_protocol_requires_membership() {
    let mono = |root_pkg: &str| {
        let mut l = label(
            "m",
            None,
            &[
                ("package.json", root_pkg),
                (
                    "packages/app/package.json",
                    r#"{"name":"app","dependencies":{"x":"workspace:*"}}"#,
                ),
                (
                    "packages/x/package.json",
                    r#"{"name":"x","main":"index.js"}"#,
                ),
                (
                    "examples/x/package.json",
                    r#"{"name":"x","main":"index.js"}"#,
                ),
            ],
            &["packages/x/index.js", "examples/x/index.js"],
        );
        import(&mut l, "packages/app/a.ts", "'x'");
        l
    };
    let out = link(&[mono(r#"{"name":"root","workspaces":["packages/*"]}"#)]);
    assert_eq!(
        has_edge(&out, "m/packages/app/a.ts", "m/packages/x/index.js"),
        Some(Identity::Path)
    );
    assert_eq!(out.edges.len(), 1, "{:?}", out.edges);
    // Both match a `**` pattern: ambiguous, no edge.
    let out = link(&[mono(r#"{"name":"root","workspaces":["**"]}"#)]);
    assert!(out.edges.is_empty());
    assert_eq!(
        reason_of(&out, "m"),
        Some(BoundaryReason::ProviderAmbiguous)
    );
    // No workspace root at all: unverified.
    let out = link(&[mono(r#"{"name":"root"}"#)]);
    assert!(out.edges.is_empty());
    assert_eq!(
        reason_of(&out, "m"),
        Some(BoundaryReason::ProviderUnverified)
    );
}

/// C6: a git spec and a lockfile git URL that disagree are contradicted; a lockfile link to a
/// differently named package is contradicted. C4: npm installs a git dependency's TOP-LEVEL package.
#[test]
fn npm_contradicting_evidence_and_git_top_level() {
    let mut a = label(
        "a",
        None,
        &[
            (
                "package.json",
                r#"{"name":"a","dependencies":{"acme-types":"github:acme/prov"}}"#,
            ),
            (
                "package-lock.json",
                &lock(
                    "acme-types",
                    "1.2.0",
                    "git+ssh://git@github.com/other/fork.git#abc",
                    "x",
                ),
            ),
        ],
        &[],
    );
    import(&mut a, "x.ts", "'acme-types'");
    let out = link(&[prov(), a]);
    assert!(out.edges.is_empty());
    assert_eq!(
        reason_of(&out, "a"),
        Some(BoundaryReason::ProviderContradicted)
    );

    // A nested package of the remote's repository is not what `npm i github:o/r` installs.
    let nested = label(
        "mono",
        Some("https://github.com/acme/mono.git"),
        &[
            ("package.json", r#"{"name":"mono-root","private":true}"#),
            ("packages/t/package.json", r#"{"name":"t","main":"i.js"}"#),
        ],
        &["packages/t/i.js"],
    );
    let mut b = label(
        "b",
        None,
        &[(
            "package.json",
            r#"{"name":"b","dependencies":{"t":"github:acme/mono"}}"#,
        )],
        &[],
    );
    import(&mut b, "x.ts", "'t'");
    let out = link(&[nested, b]);
    assert!(out.edges.is_empty());
    assert_eq!(
        reason_of(&out, "b"),
        Some(BoundaryReason::ProviderContradicted)
    );
}

/// C7: registry-source needs a registry tarball in the lock, the installed manifest's own name, and
/// (when the installed-tree record exists) its entry.
#[test]
fn registry_source_needs_tarball_name_and_tree_entry() {
    let reg = "https://registry.npmjs.org/acme-types/-/acme-types-1.2.0.tgz";
    let mk = |label_name: &str, resolved: &str, art: DepArtifact| {
        let mut c = label(
            label_name,
            None,
            &[
                (
                    "package.json",
                    r#"{"name":"c","dependencies":{"acme-types":"^1.0.0"}}"#,
                ),
                (
                    "package-lock.json",
                    &lock("acme-types", "1.2.0", resolved, "sha512-A"),
                ),
            ],
            &[],
        );
        c.artifacts.push(DepArtifact {
            manifest_dir: label_name.into(),
            ..art
        });
        import(&mut c, "x.ts", "'acme-types'");
        c
    };
    let good = DepArtifact {
        package: "acme-types".into(),
        name: Some("acme-types".into()),
        version: Some("1.2.0".into()),
        repository: Some("https://github.com/acme/prov".into()),
        ..Default::default()
    };
    let out = link(&[
        prov(),
        mk("ok", reg, good.clone()),
        mk("nores", "", good.clone()),
        mk(
            "named",
            reg,
            DepArtifact {
                name: Some("evil-types".into()),
                ..good.clone()
            },
        ),
        mk(
            "tree",
            reg,
            DepArtifact {
                tree_record: true,
                ..good.clone()
            },
        ),
    ]);
    assert_eq!(
        has_edge(&out, "ok/x.ts", "prov/src/index.ts"),
        Some(Identity::RegistrySourceVersionOnly)
    );
    for l in ["nores", "named", "tree"] {
        assert_eq!(
            reason_of(&out, l),
            Some(BoundaryReason::ProviderUnverified),
            "{l}"
        );
    }
    assert_eq!(out.edges.len(), 1, "{:?}", out.edges);
}

/// C5: npm overrides to an alias/path and cargo [patch] make the dependency a boundary.
#[test]
fn overrides_and_patches_are_boundaries() {
    let mut a = label(
        "a",
        None,
        &[(
            "package.json",
            r#"{"name":"a","dependencies":{"acme-types":"file:../prov"},"overrides":{"acme-types":"npm:evil@1"}}"#,
        )],
        &[],
    );
    import(&mut a, "x.ts", "'acme-types'");
    let out = link(&[prov(), a]);
    assert!(out.edges.is_empty());
    assert_eq!(reason_of(&out, "a"), Some(BoundaryReason::Overridden));

    let core = label(
        "core",
        None,
        &[(
            "Cargo.toml",
            "[package]\nname = \"core-x\"\nversion = \"1.0.0\"\n",
        )],
        &["src/lib.rs"],
    );
    let mut app = label(
        "app",
        None,
        &[(
            "Cargo.toml",
            "[package]\nname = \"app\"\n[dependencies]\ncore-x = \"1.0\"\n[patch.crates-io]\ncore-x = { git = \"https://example.com/fork.git\" }\n",
        )],
        &[],
    );
    import(&mut app, "src/main.rs", "core_x::y");
    let out = link(&[core, app]);
    assert!(out.edges.is_empty());
    assert_eq!(reason_of(&out, "app"), Some(BoundaryReason::Patched));
}

/// C13: target tables naming different sources for one crate are ambiguous, never first-wins.
#[test]
fn conflicting_cargo_target_declarations_are_ambiguous() {
    let core = label(
        "core",
        None,
        &[("Cargo.toml", "[package]\nname = \"foo\"\n")],
        &["src/lib.rs"],
    );
    let mut app = label(
        "app",
        None,
        &[(
            "Cargo.toml",
            "[package]\nname = \"app\"\n[target.'cfg(unix)'.dependencies]\nfoo = { path = \"../core\" }\n[target.'cfg(windows)'.dependencies]\nfoo = { path = \"../winfoo\" }\n",
        )],
        &[],
    );
    import(&mut app, "src/main.rs", "foo::x");
    let out = link(&[core, app]);
    assert!(out.edges.is_empty());
    assert_eq!(
        reason_of(&out, "app"),
        Some(BoundaryReason::ProviderAmbiguous)
    );
}

/// C1: `[tool.uv.sources]` alone declares nothing (D5).
#[test]
fn uv_source_without_declaration_is_undeclared() {
    let core = label(
        "core",
        None,
        &[("pyproject.toml", "[project]\nname = \"acme-core\"\n")],
        &["src/acme_core/__init__.py"],
    );
    let mut app = label(
        "app",
        None,
        &[(
            "pyproject.toml",
            "[project]\nname = \"app\"\n[tool.uv.sources]\nacme-core = { path = \"../core\" }\n",
        )],
        &[],
    );
    import(&mut app, "m.py", "acme_core");
    let out = link(&[core, app]);
    assert!(out.edges.is_empty());
    assert_eq!(reason_of(&out, "app"), Some(BoundaryReason::Undeclared));
}

/// C15: a project import from a file with no manifest above it is a boundary, not silence.
#[test]
fn import_without_a_manifest_is_reported() {
    let mut loose = label("loose", None, &[], &[]);
    import(&mut loose, "x.ts", "'acme-types'");
    let out = link(&[prov(), loose]);
    assert!(out.edges.is_empty());
    assert_eq!(reason_of(&out, "loose"), Some(BoundaryReason::Undeclared));
}

/// pi P2: a path link carries no consumer version (a spec is not a version) and claims no skew.
#[test]
fn path_link_claims_no_version_and_no_skew() {
    let mut c = label(
        "c",
        None,
        &[(
            "package.json",
            r#"{"name":"c","dependencies":{"acme-types":"file:../prov"}}"#,
        )],
        &[],
    );
    import(&mut c, "x.ts", "'acme-types'");
    let out = link(&[prov(), c]);
    let e = &out.edges[0];
    assert_eq!(e.consumer_version, None);
    assert!(!e.skew);
}

/// C14: a Windows drive root resolves `file:../x` across labels.
#[test]
fn windows_roots_resolve_across_labels() {
    let mut p = prov();
    p.root = Some("C:\\w\\prov".into());
    let mut c = label(
        "c",
        None,
        &[(
            "package.json",
            r#"{"name":"c","dependencies":{"acme-types":"file:../prov"}}"#,
        )],
        &[],
    );
    c.root = Some("C:\\w\\c".into());
    import(&mut c, "x.ts", "'acme-types'");
    let out = link(&[p, c]);
    assert_eq!(
        has_edge(&out, "c/x.ts", "prov/src/index.ts"),
        Some(Identity::Path)
    );
}

// ── Review round 2 (X1) ───────────────────────────────────────────────────────────────────────

/// N1: no `rootDir` and a config that picks its own inputs: the source root is not inferred.
#[test]
fn tsc_with_selected_inputs_and_no_root_dir_is_unmapped() {
    let prov = label(
        "bp",
        None,
        &[
            (
                "package.json",
                r#"{"name":"built","types":"dist/index.d.ts","scripts":{"build":"tsc"}}"#,
            ),
            (
                "tsconfig.json",
                r#"{"compilerOptions":{"outDir":"dist"},"files":["src/real/index.ts"]}"#,
            ),
        ],
        &["src/real/index.ts", "src/index.ts"],
    );
    let mut c = label(
        "c",
        None,
        &[(
            "package.json",
            r#"{"name":"c","dependencies":{"built":"file:../bp"}}"#,
        )],
        &[],
    );
    import(&mut c, "x.ts", "'built'");
    let out = link(&[prov, c]);
    assert!(out.edges.is_empty(), "{:?}", out.edges);
    assert_eq!(reason_of(&out, "c"), Some(BoundaryReason::EntryUnmapped));
}

/// N2: two declared crates share a `[lib] name`: the import is not decided by iteration order.
#[test]
fn shared_cargo_lib_name_is_not_guessed() {
    let crate_at = |l: &str, name: &str| {
        label(
            l,
            None,
            &[(
                "Cargo.toml",
                &format!("[package]\nname = \"{name}\"\n[lib]\nname = \"shared\"\n"),
            )],
            &["src/lib.rs"],
        )
    };
    let mut app = label(
        "app",
        None,
        &[(
            "Cargo.toml",
            "[package]\nname = \"app\"\n[dependencies]\na = { path = \"../a\" }\nb = { path = \"../b\", optional = true }\n",
        )],
        &[],
    );
    import(&mut app, "src/main.rs", "shared::Thing");
    let out = link(&[crate_at("a", "a"), crate_at("b", "b"), app]);
    assert!(out.edges.is_empty(), "{:?}", out.edges);
}

/// N5: a pathological workspace pattern matches nothing, at once.
#[test]
fn pathological_workspace_glob_is_bounded() {
    let pats = vec![format!("{}nomatch", "**/".repeat(16))];
    let path = (0..16)
        .map(|i| format!("d{i}"))
        .collect::<Vec<_>>()
        .join("/");
    let t = std::time::Instant::now();
    assert!(!workspace_member(&pats, &path));
    assert!(t.elapsed() < std::time::Duration::from_secs(1));
    assert!(workspace_member(
        &["packages/**".to_string()],
        "packages/a/b"
    ));
}

/// C2: `workspace:<exact version>` must be the member's version; C6: a `file:` spec whose lock
/// entry installs from a registry is contradicted.
#[test]
fn workspace_pin_and_file_spec_lock_contradictions() {
    let mut m = label(
        "m",
        None,
        &[
            (
                "package.json",
                r#"{"name":"root","workspaces":["packages/*"]}"#,
            ),
            (
                "packages/app/package.json",
                r#"{"name":"app","dependencies":{"x":"workspace:2.0.0"}}"#,
            ),
            (
                "packages/x/package.json",
                r#"{"name":"x","version":"1.0.0","main":"index.js"}"#,
            ),
        ],
        &["packages/x/index.js"],
    );
    import(&mut m, "packages/app/a.ts", "'x'");
    let out = link(&[m]);
    assert!(out.edges.is_empty());
    assert_eq!(
        reason_of(&out, "m"),
        Some(BoundaryReason::ProviderContradicted)
    );

    let reg = "https://registry.npmjs.org/acme-types/-/acme-types-1.2.0.tgz";
    let mut c = label(
        "c",
        None,
        &[
            (
                "package.json",
                r#"{"name":"c","dependencies":{"acme-types":"file:../prov"}}"#,
            ),
            (
                "package-lock.json",
                &lock("acme-types", "1.2.0", reg, "sha512-A"),
            ),
        ],
        &[],
    );
    import(&mut c, "x.ts", "'acme-types'");
    let out = link(&[prov(), c]);
    assert!(out.edges.is_empty());
    assert_eq!(
        reason_of(&out, "c"),
        Some(BoundaryReason::ProviderContradicted)
    );
}

/// A local `path` patch of a registry dependency IS its source (tokio's workspace layout); a patch
/// never applies to a `path` dependency.
#[test]
fn path_patch_of_a_registry_dependency_links_as_a_path() {
    let mut ws = label(
        "t",
        None,
        &[
            (
                "Cargo.toml",
                "[workspace]\nmembers = [\"core\", \"util\"]\n[patch.crates-io]\ncore-x = { path = \"core\" }\n",
            ),
            (
                "core/Cargo.toml",
                "[package]\nname = \"core-x\"\nversion = \"1.2.0\"\n",
            ),
            (
                "util/Cargo.toml",
                "[package]\nname = \"util\"\n[dependencies]\ncore-x = { version = \"1.0\", features = [\"a\"] }\n",
            ),
        ],
        &["core/src/lib.rs"],
    );
    import(&mut ws, "util/src/lib.rs", "core_x::Thing");
    let out = link(&[ws]);
    assert_eq!(
        has_edge(&out, "t/util/src/lib.rs", "t/core/src/lib.rs"),
        Some(Identity::Path),
        "{:?}",
        out.boundaries
    );
}
