//! APEXlang through `index_path` (estate#312, #313): components, `contains` edges and resolved
//! `@references` in the real resolver slice, on the fixture slice of Oracle's UPL-1.0 example
//! applications (`crates/wicked-estate-extract/tests/fixtures/apexlang`).
//!
//! The full-corpus resolution gate at the bottom runs when `APEXLANG_CORPUS_DIR` points at a corpus
//! fetched by `scripts/apexlang-corpus.py fetch` (CI's gate job sets it with
//! `APEXLANG_CORPUS_REQUIRED=1`, so a missing corpus fails there instead of skipping).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use wicked_estate_core::refs::apexlang_ref;
use wicked_estate_core::{
    Edge, EdgeKind, Extractor, GraphRead, Language, Provenance, SourceFile, TraversalSpec,
};
use wicked_estate_extract::ApexlangExtractor;
use wicked_estate_store::SqliteStore;

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../wicked-estate-extract/tests/fixtures/apexlang"
);

fn copy_tree(from: &Path, to: &Path, skip: &dyn Fn(&Path) -> bool) {
    for e in fs::read_dir(from).unwrap().flatten() {
        let p = e.path();
        if skip(&p) {
            continue;
        }
        let dst = to.join(e.file_name());
        if p.is_dir() {
            fs::create_dir_all(&dst).unwrap();
            copy_tree(&p, &dst, skip);
        } else {
            fs::copy(&p, &dst).unwrap();
        }
    }
}

fn fresh(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("apexlang_index_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

/// The `apexlang-ref` edges of a store, as comparable `(source, target, file, start_byte)` rows.
fn apexlang_edges(store: &dyn GraphRead) -> BTreeSet<(String, String, String, u32)> {
    store
        .all_edges()
        .unwrap()
        .into_iter()
        .filter(|e| e.resolved_by == "apexlang-ref")
        .map(|e| {
            let loc = e.location.clone().expect("a resolved ref keeps its site");
            (e.source.0, e.target.0, loc.file, loc.span.start_byte)
        })
        .collect()
}

/// The application prefix (`apexlang . . . <root>/app#`) of an APEXlang SymbolId.
fn app_of(id: &str) -> &str {
    &id[..id
        .find("/app#")
        .map(|i| i + 5)
        .expect("apexlang id has an app")]
}

fn edge_to<'a>(edges: &'a [Edge], source: &str, target_suffix: &str) -> Option<&'a Edge> {
    edges
        .iter()
        .find(|e| e.source.as_str() == source && e.target.as_str().ends_with(target_suffix))
}

#[test]
fn fixture_slice_resolves_references_inside_their_own_application() {
    let mut store = SqliteStore::in_memory().unwrap();
    wicked_estate::index_path(&mut store, Path::new(FIXTURES)).unwrap();
    let edges = store.all_edges().unwrap();
    let resolved: Vec<&Edge> = edges
        .iter()
        .filter(|e| e.resolved_by == "apexlang-ref")
        .collect();
    assert!(
        resolved.len() > 100,
        "only {} reference edges",
        resolved.len()
    );
    for e in &resolved {
        let target = store.get_node(&e.target).unwrap().expect("target exists");
        assert_eq!(target.language.as_str(), "apexlang");
        assert_eq!(
            app_of(e.source.as_str()),
            app_of(e.target.as_str()),
            "{e:?}"
        );
        assert_eq!(e.kind, EdgeKind::References);
        assert!(matches!(
            e.provenance,
            Provenance::Parsed | Provenance::Heuristic
        ));
        assert!(e.metadata.contains_key("via"));
    }

    // `breadcrumb` is a region, a breadcrumb and a breadcrumbTemplate in example-1: each property
    // binds its own kind, and a slug-only lookup would have been ambiguous.
    let ex1 = "apexlang . . . example-1/app#";
    let region = format!("{ex1}page:5#region:breadcrumb#");
    let list = edge_to(&edges, &region, "app#breadcrumb:breadcrumb#")
        .expect("source.breadcrumb binds the breadcrumb component");
    assert_eq!(list.provenance, Provenance::Parsed);
    edge_to(
        &edges,
        &region,
        "app#theme:universal-theme#breadcrumbTemplate:breadcrumb#",
    )
    .expect("breadcrumbTemplate binds the theme's template");
    assert!(
        edges
            .iter()
            .filter(|e| e.source.as_str() == region && e.resolved_by == "apexlang-ref")
            .all(|e| !e.target.as_str().ends_with("region:breadcrumb#")),
        "no reference of the breadcrumb region binds the region itself"
    );

    // Both applications declare `authorization contribution-rights`; each page binds its own.
    let ex2 = "apexlang . . . example-2/app#";
    for app in [ex1, ex2] {
        let auth = format!("{app}authorization:contribution-rights#");
        let users: Vec<_> = edges
            .iter()
            .filter(|e| e.target.as_str() == auth && e.resolved_by == "apexlang-ref")
            .collect();
        assert!(!users.is_empty(), "{auth} has dependents");
        assert!(users.iter().all(|e| e.source.as_str().starts_with(app)));
    }
}

/// Pages -> regions -> items -> processes: blast radius and path run over the new edges.
#[test]
fn blast_radius_and_path_cross_pages_regions_items_and_processes() {
    let mut store = SqliteStore::in_memory().unwrap();
    wicked_estate::index_path(&mut store, Path::new(FIXTURES)).unwrap();
    let edges = store.all_edges().unwrap();
    let page = "apexlang . . . example-1/app#page:9#";
    // A page item sits in a region by reference; a process names its form region.
    let item_to_region = edges
        .iter()
        .find(|e| {
            e.resolved_by == "apexlang-ref"
                && e.source.as_str().starts_with(&format!("{page}pageItem:"))
                && e.target.as_str().starts_with(&format!("{page}region:"))
        })
        .expect("a page-9 item references a page-9 region");
    let region = item_to_region.target.clone();
    let process_to_region = edges.iter().find(|e| {
        e.resolved_by == "apexlang-ref"
            && e.source.as_str().starts_with(&format!("{page}process:"))
            && e.target.as_str().starts_with(&format!("{page}region:"))
    });
    let process = process_to_region.expect("a page-9 process references a region");

    // Blast radius of the process's region: the page that contains it, the process that names it,
    // and every item placed in it.
    let dependents_of = |id: &wicked_estate_core::SymbolId| -> BTreeSet<String> {
        store
            .traverse(id, &TraversalSpec::blast_radius(2))
            .unwrap()
            .nodes
            .into_iter()
            .map(|n| n.symbol.0)
            .collect()
    };
    let dependents = dependents_of(&process.target);
    assert!(
        dependents.contains(page),
        "the page contains the region: {dependents:?}"
    );
    assert!(
        dependents.contains(process.source.as_str()),
        "{dependents:?}"
    );
    assert!(
        dependents_of(&region).contains(item_to_region.source.as_str()),
        "the item placed in the region depends on it"
    );

    // page -> item (contains) -> region (layout.region): a two-hop route over both edge kinds.
    let path = wicked_estate_core::path_between(
        &store,
        item_to_region.source.as_str(),
        region.as_str(),
        4,
        10_000,
    )
    .unwrap();
    assert!(path.found, "the item reaches its region");
    assert_eq!(path.hops.len(), 1);
    assert_eq!(path.hops[0].kind, EdgeKind::References);
}

/// Parked refs keep everything needed to resolve: an incremental run that adds the shared
/// components later, on a reopened on-disk store, ends with the same edges as one full pass.
#[test]
fn stored_refs_re_resolve_to_the_same_edges_after_reopen() {
    let full_dir = fresh("full");
    copy_tree(Path::new(FIXTURES), &full_dir, &|_| false);
    let mut full = SqliteStore::in_memory().unwrap();
    wicked_estate::index_path(&mut full, &full_dir).unwrap();
    let want = apexlang_edges(&full);

    let dir = fresh("incr");
    let db = dir.join("graph.db");
    let src = dir.join("src");
    fs::create_dir_all(&src).unwrap();
    copy_tree(Path::new(FIXTURES), &src, &|p| {
        p.file_name().is_some_and(|n| n == "shared-components")
    });
    {
        let mut store = SqliteStore::open(&db).unwrap();
        wicked_estate::index_path(&mut store, &src).unwrap();
        assert!(
            apexlang_edges(&store).len() < want.len(),
            "shared refs park first"
        );
    }
    copy_tree(Path::new(FIXTURES), &src, &|_| false);
    let mut reopened = SqliteStore::open(&db).unwrap();
    wicked_estate::index_path(&mut reopened, &src).unwrap();
    assert_eq!(apexlang_edges(&reopened), want);
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_dir_all(&full_dir);
}

/// Resolution report for one corpus root: resolved / unresolved by class, every unresolved site.
fn resolution_report(
    root: &Path,
    edges_out: Option<&Path>,
) -> (BTreeMap<String, usize>, Vec<String>, usize) {
    // `APEXLANG_DB=<file>` keeps the corpus graph on disk for inspection.
    let mut store = match std::env::var_os("APEXLANG_DB").filter(|_| edges_out.is_some()) {
        Some(db) => {
            let _ = fs::remove_file(&db);
            SqliteStore::open(&db).unwrap()
        }
        None => SqliteStore::in_memory().unwrap(),
    };
    wicked_estate::index_path(&mut store, root).unwrap();
    let edges = store.all_edges().unwrap();
    // Every target is a real component of the same application.
    for e in edges.iter().filter(|e| e.resolved_by == "apexlang-ref") {
        let t = store
            .get_node(&e.target)
            .unwrap()
            .expect("resolved target exists");
        assert_eq!(t.language.as_str(), "apexlang");
        assert_eq!(app_of(e.source.as_str()), app_of(e.target.as_str()));
    }
    let mut classes: BTreeMap<String, usize> = BTreeMap::new();
    let mut unresolved = Vec::new();
    // raw name -> parked (file, start byte) sites, one store query per distinct name.
    let mut parked: BTreeMap<String, BTreeSet<(String, u32)>> = BTreeMap::new();
    // site -> (property, written reference), for the resolved-edge export below.
    let mut sites: BTreeMap<(String, u32), (String, String)> = BTreeMap::new();
    let mut total = 0;
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                if p.file_name().is_some_and(|n| n != ".git") {
                    stack.push(p);
                }
                continue;
            }
            if p.extension().is_none_or(|x| x != "apx") {
                continue;
            }
            let rel = p
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let text = fs::read_to_string(&p).unwrap();
            let ex = ApexlangExtractor::shared()
                .extract(&SourceFile {
                    path: rel.clone(),
                    language: Language::new("apexlang"),
                    text: text.clone(),
                })
                .unwrap();
            for r in ex.refs {
                total += 1;
                sites.insert(
                    (rel.clone(), r.location.span.start_byte),
                    (
                        r.hints
                            .get("apexlang_property")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        text[r.location.span.start_byte as usize
                            ..r.location.span.end_byte as usize]
                            .to_string(),
                    ),
                );
                let parsed = apexlang_ref::parse(&r.raw_name).unwrap();
                let theme = text[r.location.span.start_byte as usize..].starts_with("@/");
                let class = match (parsed.kind.as_str(), theme) {
                    ("-", _) => "never-bind",
                    ("*", _) => "untyped",
                    (_, true) => "typed theme-relative (@/)",
                    _ => "typed",
                };
                // Unresolved = still parked in the store at this exact site (two sites that bind the
                // same source -> target pair share one deduplicated edge, so edges alone undercount).
                let parked_sites = parked.entry(r.raw_name.clone()).or_insert_with(|| {
                    store
                        .unresolved_refs_for_name(&r.raw_name)
                        .unwrap()
                        .into_iter()
                        .map(|p| (p.location.file, p.location.span.start_byte))
                        .collect()
                });
                let ok = !parked_sites.contains(&(rel.clone(), r.location.span.start_byte));
                *classes
                    .entry(format!(
                        "{class} {}",
                        if ok { "resolved" } else { "unresolved" }
                    ))
                    .or_default() += 1;
                if !ok {
                    unresolved.push(format!(
                        "{rel}:{}:{}\t{}\t{}\t{}",
                        r.location.span.start_line + 1,
                        r.location.span.start_col + 1,
                        class,
                        r.hints
                            .get("apexlang_property")
                            .and_then(|v| v.as_str())
                            .unwrap_or(""),
                        &text[r.location.span.start_byte as usize
                            ..r.location.span.end_byte as usize],
                    ));
                }
            }
        }
    }
    unresolved.sort();
    // Resolved-edge export for the precision oracle (oracle/apexlang/label.py).
    if let Some(out) = edges_out {
        let label = |id: &wicked_estate_core::SymbolId| {
            store.get_node(id).unwrap().map(|n| {
                let meta = |k: &str| {
                    n.metadata
                        .get(k)
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string()
                };
                (
                    meta("apexlang_kind"),
                    n.name.clone(),
                    meta("apexlang_label"),
                )
            })
        };
        let mut rows = vec!["file\tline\tproperty\twritten\tvia\tsource\tsource_kind\tsource_key\tsource_label\ttarget\ttarget_kind\ttarget_key\ttarget_label".to_string()];
        for e in edges.iter().filter(|e| e.resolved_by == "apexlang-ref") {
            let loc = e.location.as_ref().unwrap();
            let (prop, written) = sites
                .get(&(loc.file.clone(), loc.span.start_byte))
                .cloned()
                .unwrap_or_default();
            let (sk, skey, slab) = label(&e.source).unwrap_or_default();
            let (tk, tkey, tlab) = label(&e.target).unwrap_or_default();
            let via = e.metadata.get("via").and_then(|v| v.as_str()).unwrap_or("");
            rows.push(format!(
                "{}\t{}\t{prop}\t{written}\t{via}\t{}\t{sk}\t{skey}\t{slab}\t{}\t{tk}\t{tkey}\t{tlab}",
                loc.file,
                loc.span.start_line + 1,
                e.source,
                e.target
            ));
        }
        fs::write(out, rows.join("\n") + "\n").unwrap();
    }
    (classes, unresolved, total)
}

#[test]
fn fixture_slice_resolution_rate_is_reported_with_every_unresolved_site() {
    let (classes, unresolved, total) = resolution_report(Path::new(FIXTURES), None);
    let resolved: usize = classes
        .iter()
        .filter(|(k, _)| k.ends_with(" resolved"))
        .map(|(_, v)| v)
        .sum();
    eprintln!("APEXlang fixture slice: {resolved}/{total} references resolved");
    for (k, v) in &classes {
        eprintln!("  {v:6} {k}");
    }
    for u in &unresolved {
        eprintln!("  UNRESOLVED {u}");
    }
    assert_eq!(resolved + unresolved.len(), total);
    assert!(resolved * 2 > total, "most fixture references resolve");
}

#[test]
fn full_pinned_corpus_resolution() {
    let Some(dir) = std::env::var_os("APEXLANG_CORPUS_DIR") else {
        assert!(
            std::env::var_os("APEXLANG_CORPUS_REQUIRED").is_none(),
            "APEXLANG_CORPUS_REQUIRED is set but APEXLANG_CORPUS_DIR is not: fetch the corpus with \
             scripts/apexlang-corpus.py fetch <dir>"
        );
        eprintln!("SKIP full APEXlang corpus resolution: APEXLANG_CORPUS_DIR is not set");
        return;
    };
    let (classes, unresolved, total) = resolution_report(
        Path::new(&dir),
        std::env::var_os("APEXLANG_EDGES").as_deref().map(Path::new),
    );
    eprintln!("APEXlang corpus: {total} references");
    for (k, v) in &classes {
        eprintln!("  {v:6} {k}");
    }
    if let Some(out) = std::env::var_os("APEXLANG_REPORT") {
        fs::write(out, unresolved.join("\n") + "\n").unwrap();
    }
    assert_eq!(
        classes.values().sum::<usize>(),
        total,
        "every reference is accounted for"
    );
    // Regression floor at the pinned corpus (2026-11-10: 20,936 typed resolved, 40 typed
    // unresolved — ambiguous `masterColumn @ID`, `operation @put`, legacy data-load definitions).
    let get = |k: &str| classes.get(k).copied().unwrap_or(0);
    assert!(get("typed resolved") >= 20_900, "{classes:?}");
    assert!(get("typed unresolved") <= 40, "{classes:?}");
}
