//! Oracle APEXlang (`.apx`) grammar + extractor (estate#310, #311, #312, #313).
//!
//! Fixtures: `tests/fixtures/apexlang/` — a byte-for-byte slice of Oracle's UPL-1.0 example
//! applications (see its MANIFEST.tsv and LICENSE-UPL). The full-corpus gate at the bottom runs when
//! `APEXLANG_CORPUS_DIR` points at a corpus fetched by `scripts/apexlang-corpus.py fetch`; CI sets
//! it with `APEXLANG_CORPUS_REQUIRED=1`, so a missing corpus fails there instead of skipping.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use wicked_estate_core::refs::apexlang_ref;
use wicked_estate_core::{EdgeKind, Extraction, Extractor, Language, NodeKind, SourceFile};
use wicked_estate_extract::apexlang::{
    ApexlangData, ApexlangExtractor, IDENTITY_METADATA_KEY, KIND_METADATA_KEY, app_layout,
};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/apexlang");

fn extract_with(ex: &ApexlangExtractor, path: &str, text: &str) -> Extraction {
    ex.extract(&SourceFile {
        path: path.to_string(),
        language: Language::new("apexlang"),
        text: text.to_string(),
    })
    .expect("apexlang extraction succeeds")
}

fn extract(path: &str, text: &str) -> Extraction {
    extract_with(ApexlangExtractor::shared(), path, text)
}

fn apx_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if p.file_name().is_some_and(|n| n != ".git") {
                    stack.push(p);
                }
            } else if p.extension().is_some_and(|x| x == "apx") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// Repo-relative path of a fixture file (what the indexer would hand the extractor).
fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/")
}

fn parse(text: &str) -> tree_sitter::Tree {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&wicked_estate_extract::apexlang::GRAMMAR.into())
        .unwrap();
    parser.parse(text, None).unwrap()
}

/// Every ERROR / MISSING node in a parse, as `line:col kind`.
fn parse_problems(tree: &tree_sitter::Tree) -> Vec<String> {
    let mut out = Vec::new();
    let mut cursor = tree.walk();
    let mut stack = vec![tree.root_node()];
    while let Some(n) = stack.pop() {
        if n.is_error() || n.is_missing() {
            let p = n.start_position();
            out.push(format!("{}:{} {}", p.row + 1, p.column + 1, n.kind()));
        }
        if n.has_error() {
            stack.extend(n.children(&mut cursor));
        }
    }
    out
}

fn count_kind(tree: &tree_sitter::Tree, kind: &str) -> usize {
    let mut n = 0;
    let mut cursor = tree.walk();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        if node.kind() == kind {
            n += 1;
        }
        stack.extend(node.children(&mut cursor));
    }
    n
}

fn kind_of(n: &wicked_estate_core::Node) -> &str {
    n.metadata
        .get(KIND_METADATA_KEY)
        .and_then(|v| v.as_str())
        .unwrap_or("")
}

#[test]
fn apx_dispatches_to_apexlang_and_salesforce_apex_keeps_cls_and_trigger() {
    use wicked_estate_extract::treesitter::extractor_for_extension;
    let lang = |ext: &str| {
        extractor_for_extension(ext)
            .unwrap_or_else(|| panic!(".{ext} must be wired"))
            .languages()
    };
    assert_eq!(lang("apx"), vec![Language::new("apexlang")]);
    assert_eq!(lang("cls"), vec![Language::new("apex")]);
    assert_eq!(lang("trigger"), vec![Language::new("apex")]);
    let reg = wicked_estate_extract::registry();
    let row = reg
        .iter()
        .find(|l| l.name == "apexlang")
        .expect("manifest row");
    assert_eq!(row.ext, vec!["apx".to_string()]);
}

#[test]
fn fence_body_is_exactly_the_text_between_the_fences() {
    let src = "process p (\n    source {\n        plsqlCode:\n            ```plsql\n            :P1_X := 1;\n            x := '```';\n            ```\n    }\n)\n";
    let tree = parse(src);
    assert!(
        parse_problems(&tree).is_empty(),
        "{:?}",
        parse_problems(&tree)
    );
    let mut cursor = tree.walk();
    let mut stack = vec![tree.root_node()];
    let mut seen = 0;
    while let Some(n) = stack.pop() {
        if n.kind() == "fenced_block" {
            seen += 1;
            let lang = n.child_by_field_name("language").unwrap();
            assert_eq!(&src[lang.byte_range()], "plsql");
            let body = n.child_by_field_name("body").unwrap();
            assert_eq!(
                &src[body.byte_range()],
                "            :P1_X := 1;\n            x := '```';\n"
            );
        }
        stack.extend(n.children(&mut cursor));
    }
    assert_eq!(seen, 1);
}

#[test]
fn fixture_slice_parses_without_error_and_covers_the_required_constructs() {
    let root = Path::new(FIXTURES);
    let files = apx_files(root);
    assert_eq!(files.len(), 23, "MANIFEST.tsv lists 23 .apx files");
    let mut kinds: HashSet<String> = HashSet::new();
    let mut tags: HashSet<String> = HashSet::new();
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap();
        let tree = parse(&text);
        let problems = parse_problems(&tree);
        assert!(problems.is_empty(), "{}: {problems:?}", f.display());
        for n in extract(&rel(root, f), &text).nodes {
            kinds.insert(kind_of(&n).to_string());
        }
        for line in text.lines() {
            if let Some(tag) = line.trim().strip_prefix("```") {
                tags.insert(tag.to_string());
            }
        }
    }
    for k in [
        "app",
        "page",
        "region",
        "pageItem",
        "button",
        "process",
        "validation",
        "dynamicAction",
        "authorization",
        "lov",
        "list",
        "theme",
    ] {
        assert!(kinds.contains(k), "fixture slice lacks a `{k}` component");
    }
    for t in ["sql", "plsql", "html", "css", "javascript-browser"] {
        assert!(tags.contains(t), "fixture slice lacks a ```{t} block");
    }
}

#[test]
fn fixture_slice_manifest_lists_every_file_with_its_size() {
    let manifest = std::fs::read_to_string(Path::new(FIXTURES).join("MANIFEST.tsv")).unwrap();
    let mut rows = 0;
    for line in manifest.lines().filter(|l| !l.starts_with('#')) {
        let cols: Vec<&str> = line.split('\t').collect();
        assert_eq!(cols.len(), 6, "bad manifest row {line}");
        let bytes = std::fs::read(Path::new(FIXTURES).join(cols[3])).unwrap();
        assert_eq!(bytes.len().to_string(), cols[5], "{} size", cols[3]);
        assert!(!cols[2].contains("docs.oracle.com"));
        rows += 1;
    }
    assert_eq!(
        rows, 25,
        "23 .apx + 2 apexlang.json (the licence is LICENSE-UPL)"
    );
    assert!(Path::new(FIXTURES).join("LICENSE-UPL").exists());
}

/// Every component becomes exactly one node, under its application, held by a `contains` edge.
#[test]
fn every_component_is_one_node_with_a_path_derived_id_and_a_container() {
    let root = Path::new(FIXTURES);
    let mut ids: HashMap<String, String> = HashMap::new();
    for f in apx_files(root) {
        let path = rel(root, &f);
        let text = std::fs::read_to_string(&f).unwrap();
        let components = count_kind(&parse(&text), "component");
        let ex = extract(&path, &text);
        let comps: Vec<_> = ex
            .nodes
            .iter()
            .filter(|n| n.kind != NodeKind::File)
            .collect();
        assert_eq!(comps.len(), components, "{path}: one node per component");
        let app_root = path.split('/').next().unwrap();
        let prefix = format!("apexlang . . . {app_root}/app#");
        for n in &comps {
            assert!(
                n.symbol.as_str().starts_with(&prefix),
                "{path}: {} is not under {prefix}",
                n.symbol
            );
            if let Some(prev) = ids.insert(n.symbol.0.clone(), path.clone()) {
                panic!("{} minted twice ({prev} and {path})", n.symbol);
            }
            // Held by the file, and by its parent component unless it is top level.
            let holders: Vec<_> = ex
                .local_edges
                .iter()
                .filter(|e| e.kind == EdgeKind::Contains && e.target == n.symbol)
                .collect();
            assert!(
                !holders.is_empty() && holders.len() <= 2,
                "{path}: {}",
                n.symbol
            );
        }
    }
    assert!(ids.len() > 300, "slice has {} components", ids.len());
}

#[test]
fn app_root_comes_from_the_layout() {
    let d = ApexlangData::builtin();
    let l = app_layout("x/customers/pages/p00007-a.apx", d);
    assert_eq!(l.root, "x/customers");
    let l = app_layout(
        "x/customers/shared-components/themes/universal-theme/list-templates/a.apx",
        d,
    );
    assert_eq!(l.root, "x/customers");
    assert_eq!(
        l.containers,
        vec![("theme".to_string(), "universal-theme".to_string())]
    );
    let l = app_layout(
        "x/c/shared-components/plugins/item/star/custom-attributes.apx",
        d,
    );
    assert_eq!(
        l.containers,
        vec![("plugin".to_string(), "star".to_string())]
    );
    assert_eq!(
        app_layout("x/customers/application.apx", d).root,
        "x/customers"
    );
    let l = app_layout("x/c/supporting-objects/substitutions.apx", d);
    assert_eq!(
        l.containers,
        vec![("supportingObject".to_string(), String::new())]
    );
    assert_eq!(app_layout("application.apx", d).root, ".");
    // A `pages` directory that is not the file's parent is not the app's pages dir.
    assert_eq!(app_layout("pages/app1/pages/p1.apx", d).root, "pages/app1");
}

const PAGE: &str = r#"page 4 (
    name: Customer
    security {
        authorizationScheme: @contribution-rights
    }

    region customer (
        name: Customer
        type: form
        executionMappingIdentifier: 3073526297829089
    )

    region orders (
        name: Orders
        layout {
            parentRegion: @customer
        }
        column ID (
            type: hidden
        )
    )

    region lines (
        column ID (
            type: hidden
        )
    )

    pageItem P4_NAME (
        type: textField
        layout {
            sequence: 10
            region: @customer
        }
        help {
            helpText: Mail @admin or see ```this```
        }
    )
)
"#;

fn ids(ex: &Extraction) -> Vec<String> {
    let mut v: Vec<String> = ex
        .nodes
        .iter()
        .filter(|n| n.kind != NodeKind::File)
        .map(|n| n.symbol.0.clone())
        .collect();
    v.sort();
    v
}

/// Reformatting, reordering properties and changing generated numbers leave every id unchanged.
#[test]
fn ids_are_stable_across_reformat_property_reorder_and_generated_numbers() {
    let base = ids(&extract("app/pages/p00004-customer.apx", PAGE));
    let reformatted = PAGE
        .lines()
        .map(|l| {
            let t = l.trim_start();
            let depth = (l.len() - t.len()) / 4;
            format!("{}{t}\n\n", "\t".repeat(depth))
        })
        .collect::<String>();
    assert_eq!(
        ids(&extract("app/pages/p00004-customer.apx", &reformatted)),
        base
    );
    let reordered = PAGE.replace(
        "        name: Customer\n        type: form\n",
        "        type: form\n        name: Customer\n",
    );
    assert_ne!(reordered, PAGE);
    assert_eq!(
        ids(&extract("app/pages/p00004-customer.apx", &reordered)),
        base
    );
    let renumbered = PAGE.replace("3073526297829089", "9999000011112222");
    assert_eq!(
        ids(&extract("app/pages/p00004-customer.apx", &renumbered)),
        base
    );
    // The page file's name (its alias) is not identity either.
    assert_eq!(ids(&extract("app/pages/p00004-renamed.apx", PAGE)), base);
}

#[test]
fn same_id_under_different_parents_or_apps_never_collides() {
    let a = ids(&extract("one/pages/p00004-customer.apx", PAGE));
    assert!(a.contains(&"apexlang . . . one/app#page:4#region:orders#column:ID#".to_string()));
    assert!(a.contains(&"apexlang . . . one/app#page:4#region:lines#column:ID#".to_string()));
    let b = ids(&extract("two/pages/p00004-customer.apx", PAGE));
    assert!(
        a.iter().all(|id| !b.contains(id)),
        "two applications share no id"
    );
}

#[test]
fn qualifiers_keyless_kinds_and_positional_fallback_keep_siblings_apart() {
    let src = r#"templateOptionGroup display-mode (
    templateType: page
)

templateOptionGroup display-mode (
    templateType: region
)

savedReport primary (
    sort (
        column: @ENAME
    )
    sort (
        column: @SAL
    )
    branch (
        type: redirect
    )
    branch (
        type: redirect
    )
)
"#;
    let ex = extract(
        "app/shared-components/themes/ut/template-option-groups.apx",
        src,
    );
    let by_id: HashMap<&str, &wicked_estate_core::Node> =
        ex.nodes.iter().map(|n| (n.symbol.as_str(), n)).collect();
    let base = "apexlang . . . app/app#theme:ut#";
    for id in [
        format!("{base}templateOptionGroup:page%2Fdisplay-mode#"),
        format!("{base}templateOptionGroup:region%2Fdisplay-mode#"),
        format!("{base}savedReport:primary#sort:ENAME#"),
        format!("{base}savedReport:primary#sort:SAL#"),
        format!("{base}savedReport:primary#branch#"),
        format!("{base}savedReport:primary#branch~2#"),
    ] {
        assert!(
            by_id.contains_key(id.as_str()),
            "missing {id}: {:?}",
            by_id.keys()
        );
    }
    let n = by_id[format!("{base}templateOptionGroup:region%2Fdisplay-mode#").as_str()];
    assert_eq!(
        n.name, "region/display-mode",
        "the key is what `@region/...` refs spell"
    );
    let positional = by_id[format!("{base}savedReport:primary#branch~2#").as_str()];
    assert_eq!(
        positional
            .metadata
            .get(IDENTITY_METADATA_KEY)
            .and_then(|v| v.as_str()),
        Some("positional")
    );
}

/// References come only from `@reference` values; their raw name carries the target kind + scope.
#[test]
fn references_carry_target_kind_and_scope_and_come_only_from_reference_tokens() {
    let ex = extract("app/pages/p00004-customer.apx", PAGE);
    let decoded: Vec<_> = ex
        .refs
        .iter()
        .map(|r| {
            let p = apexlang_ref::parse(&r.raw_name).expect("apexlang-ref name");
            (r.from.0.clone(), p.kind, p.slug, p.scope)
        })
        .collect();
    let page = "apexlang . . . app/app#page:4#".to_string();
    let app = "apexlang . . . app/app#".to_string();
    assert_eq!(
        decoded,
        vec![
            (
                page.clone(),
                "authorization".into(),
                "contribution-rights".into(),
                app
            ),
            (
                format!("{page}region:orders#"),
                "region".into(),
                "customer".into(),
                page.clone()
            ),
            (
                format!("{page}pageItem:P4_NAME#"),
                "region".into(),
                "customer".into(),
                page.clone()
            ),
        ],
        "the `@admin` in help text is text, not a reference"
    );
    for r in &ex.refs {
        assert_eq!(r.kind, EdgeKind::References);
        assert_eq!(
            &PAGE[r.location.span.start_byte as usize..r.location.span.end_byte as usize]
                .chars()
                .next(),
            &Some('@')
        );
    }
}

#[test]
fn theme_relative_and_unknown_properties() {
    let src = "region r (\n    appearance {\n        template: @/standard\n    }\n    misc {\n        foo: @/bar\n        baz: @qux\n    }\n)\n";
    let ex = extract("app/pages/p00001-x.apx", src);
    let kinds: Vec<String> = ex
        .refs
        .iter()
        .map(|r| apexlang_ref::parse(&r.raw_name).unwrap().kind)
        .collect();
    assert_eq!(kinds, vec!["regionTemplate", "-", "*"]);
}

#[test]
fn a_page_scoped_property_outside_a_page_never_widens_to_the_app() {
    let src = "list l (\n    entry e (\n        layout {\n            region: @main\n        }\n    )\n)\n";
    let ex = extract("app/shared-components/lists/l.apx", src);
    let p = apexlang_ref::parse(&ex.refs[0].raw_name).unwrap();
    assert_eq!(p.kind, apexlang_ref::NO_BIND);
}

/// The property table is data: a new row changes resolution input with no Rust change.
#[test]
fn property_table_rows_are_data() {
    let src = "region r (\n    custom {\n        target: @thing\n    }\n)\n";
    let builtin = extract("app/pages/p00001-x.apx", src);
    assert_eq!(
        apexlang_ref::parse(&builtin.refs[0].raw_name).unwrap().kind,
        "*"
    );
    let toml = format!(
        "{}\n[[ref]]\nowner = \"region\"\nproperty = \"custom.target\"\ntarget = \"lov\"\nscope = \"app\"\n",
        include_str!("../apexlang.toml")
    );
    let ex = ApexlangExtractor::with_data(ApexlangData::parse(&toml).unwrap());
    let added = extract_with(&ex, "app/pages/p00001-x.apx", src);
    assert_eq!(
        apexlang_ref::parse(&added.refs[0].raw_name).unwrap().kind,
        "lov"
    );
}

/// Full-corpus gate (estate#311): every pinned `.apx` parses with zero ERROR and zero MISSING
/// nodes, and every component of an application gets a distinct id.
#[test]
fn full_pinned_corpus_parses_cleanly_and_mints_distinct_ids() {
    let Some(dir) = std::env::var_os("APEXLANG_CORPUS_DIR") else {
        assert!(
            std::env::var_os("APEXLANG_CORPUS_REQUIRED").is_none(),
            "APEXLANG_CORPUS_REQUIRED is set but APEXLANG_CORPUS_DIR is not: fetch the corpus with \
             scripts/apexlang-corpus.py fetch <dir>"
        );
        eprintln!("SKIP full APEXlang corpus gate: APEXLANG_CORPUS_DIR is not set");
        return;
    };
    let root = PathBuf::from(dir);
    let files = apx_files(&root);
    assert_eq!(
        files.len(),
        3160,
        "pinned corpus = 2,961 oracle/apex + 199 oracle/skills"
    );
    let mut failures = Vec::new();
    let mut seen: HashMap<String, String> = HashMap::new();
    let mut components = 0usize;
    let mut refs = 0usize;
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap();
        let path = rel(&root, f);
        let problems = parse_problems(&parse(&text));
        if !problems.is_empty() {
            failures.push(format!("{path}: {problems:?}"));
        }
        let ex = extract(&path, &text);
        refs += ex.refs.len();
        for n in ex.nodes.iter().filter(|n| n.kind != NodeKind::File) {
            components += 1;
            if let Some(prev) = seen.insert(n.symbol.0.clone(), path.clone()) {
                failures.push(format!("{} minted by {prev} and {path}", n.symbol));
            }
        }
    }
    eprintln!(
        "APEXlang corpus: {} files, {components} components, {refs} references, {} failures",
        files.len(),
        failures.len()
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
