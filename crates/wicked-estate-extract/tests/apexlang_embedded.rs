//! Embedded code (estate#314), page-item bind variables (#315) and the export metadata version
//! (#316) in APEXlang, plus the generic embedded-region utility they run on.

use std::path::{Path, PathBuf};

use wicked_estate_core::refs::apexlang_ref;
use wicked_estate_core::{
    EdgeKind, Extraction, Extractor, Language, NodeKind, Provenance, SourceFile, Span, SymbolId,
};
use wicked_estate_extract::apexlang::{
    ApexlangData, ApexlangExtractor, MMD_SUPPORTED_KEY, MMD_VERSION_KEY, apply_export_metadata,
    export_metadata_path,
};
use wicked_estate_extract::embedded::{
    EmbedTable, EmbeddedRegion, LANGUAGE_KEY, PARSED_KEY, REASON_KEY, REGION_KIND, extract_regions,
};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/apexlang");

fn sf(path: &str, text: &str) -> SourceFile {
    SourceFile {
        path: path.to_string(),
        language: Language::new("apexlang"),
        text: text.to_string(),
    }
}

fn extract(path: &str, text: &str) -> Extraction {
    ApexlangExtractor::shared()
        .extract(&sf(path, text))
        .unwrap()
}

fn regions_of(ex: &Extraction) -> Vec<&wicked_estate_core::Node> {
    ex.nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Other(REGION_KIND.into()))
        .collect()
}

const INSTALL: &str = "installScript create-tables (\n    script:\n        ```sql\n        create table emp (id number);\n        ```\n)\n";

/// Symbols extracted from an embedded `sql` block point at their bytes in the `.apx` file.
#[test]
fn embedded_sql_symbols_point_into_the_host_file() {
    let ex = extract("app/supporting-objects/install-scripts.apx", INSTALL);
    let emp = ex
        .nodes
        .iter()
        .find(|n| n.name == "emp" && n.language.as_str() == "sql")
        .expect("the embedded CREATE TABLE is extracted");
    assert_eq!(
        emp.location.file,
        "app/supporting-objects/install-scripts.apx"
    );
    let s = emp.location.span;
    let text = &INSTALL[s.start_byte as usize..s.end_byte as usize];
    assert!(text.starts_with("create table emp"), "{text:?}");
    assert_eq!(s.start_line, 3, "line of `create table` in the host file");
    assert_eq!(s.start_col, 8, "column of `create table` in the host file");
    let region = regions_of(&ex)[0];
    assert_eq!(region.metadata.get(PARSED_KEY), Some(&true.into()));
    assert_eq!(region.metadata.get(LANGUAGE_KEY), Some(&"sql".into()));
    // The region is held by its owner; the table by the region; the table edge says where from.
    let owner = "apexlang . . . app/app#supportingObject#installScript:create-tables#";
    assert!(ex.local_edges.iter().any(|e| e.kind == EdgeKind::Contains
        && e.source.as_str() == owner
        && e.target == region.symbol));
    let held = ex
        .local_edges
        .iter()
        .find(|e| e.target == emp.symbol && e.source == region.symbol)
        .expect("region contains the table");
    assert_eq!(held.provenance, Provenance::Parsed);
    assert_eq!(
        held.metadata.get("embedded_host").and_then(|v| v.as_str()),
        Some("app/supporting-objects/install-scripts.apx")
    );
    assert_eq!(
        held.metadata
            .get("embedded_region")
            .and_then(|v| v.as_str()),
        Some(region.symbol.as_str())
    );
}

/// Every fenced block in the fixture slice is a region marker: parsed, or unparsed with a reason.
#[test]
fn no_fenced_block_is_dropped() {
    let mut files: Vec<PathBuf> = Vec::new();
    let mut stack = vec![PathBuf::from(FIXTURES)];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "apx") {
                files.push(p);
            }
        }
    }
    let (mut fences, mut parsed, mut unparsed) = (0, 0, 0);
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap();
        let rel = f
            .strip_prefix(Path::new(FIXTURES))
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let n = text
            .lines()
            .filter(|l| l.trim_start().starts_with("```"))
            .count()
            / 2;
        let ex = extract(&rel, &text);
        let regions = regions_of(&ex);
        assert_eq!(regions.len(), n, "{rel}: one region per fenced block");
        for r in regions {
            if r.metadata.get(PARSED_KEY) == Some(&true.into()) {
                parsed += 1;
            } else {
                assert!(
                    r.metadata.get(REASON_KEY).is_some(),
                    "{rel}: unparsed needs a reason"
                );
                unparsed += 1;
            }
        }
        fences += n;
    }
    assert_eq!(parsed + unparsed, fences);
    assert!(
        parsed > 0 && unparsed > 0,
        "parsed {parsed}, unparsed {unparsed}"
    );
}

/// The tag -> language mapping is data; an unmapped tag or a failing extractor is a marker, and
/// the host extraction never fails.
#[test]
fn tag_mapping_is_data_and_failures_are_markers() {
    let host = sf("h.apx", "x\nselect 1 from dual\ny\n");
    let span = Span {
        start_byte: 2,
        end_byte: 21,
        start_line: 1,
        start_col: 0,
        end_line: 2,
        end_col: 0,
    };
    let region = |tag: &str| EmbeddedRegion {
        owner: SymbolId("owner".into()),
        key: "k".into(),
        tag: Some(tag.into()),
        span,
    };
    let builtin = EmbedTable::builtin();
    assert_eq!(
        builtin.language_for(Some("javascript-browser")),
        Some("javascript")
    );
    assert_eq!(
        builtin.language_for(Some("plsql")),
        None,
        "no PL/SQL grammar is built in"
    );
    assert_eq!(builtin.language_for(Some("pgsql")), None);
    let table =
        EmbedTable::parse("[tags]\npgsql = \"sql\"\nbroken = \"no-such-language\"\n").unwrap();
    assert_eq!(table.language_for(Some("PGSQL")), Some("sql"));
    let out = extract_regions(&host, &[region("pgsql"), region("broken")], &table);
    let markers: Vec<_> = out
        .extraction
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Other(REGION_KIND.into()))
        .collect();
    assert_eq!(markers.len(), 2);
    assert_eq!(markers[0].metadata.get(PARSED_KEY), Some(&true.into()));
    assert_eq!(markers[1].metadata.get(PARSED_KEY), Some(&false.into()));
    assert!(
        markers[1]
            .metadata
            .get(REASON_KEY)
            .and_then(|v| v.as_str())
            .is_some_and(|r| r.contains("no-such-language"))
    );
}

fn bind_refs(ex: &Extraction) -> Vec<(String, String, String)> {
    ex.refs
        .iter()
        .filter(|r| r.hints.contains_key("apexlang_bind_variable"))
        .map(|r| {
            let p = apexlang_ref::parse(&r.raw_name).unwrap();
            let site = r.hints["apexlang_bind_variable"]
                .as_str()
                .unwrap()
                .to_string();
            (p.kind, p.slug, site)
        })
        .collect()
}

const PAGE7: &str = r#"page 7 (
    process load (
        source {
            plsqlCode:
                ```plsql
                :P7_NAME := 'x :P7_IN_STRING';
                -- :P7_IN_COMMENT
                :p0_global := q;
                ```
        }
    )

    pageItem P7_CONTACT (
        lov {
            sqlQuery:
                ```sql
                select name d, id r from contacts
                where customer_id = :P7_CUSTOMER_ID and note = ':P7_QUOTED'
                ```
        }
    )
)
"#;

/// Bind variables become refs to page items: typed in a parsed (sql) region, heuristic (`~`) in an
/// unparsed (plsql) one; strings and comments are skipped in both.
#[test]
fn bind_variables_link_to_page_items_with_honest_confidence() {
    let ex = extract("app/pages/p00007-x.apx", PAGE7);
    let got = bind_refs(&ex);
    assert_eq!(
        got,
        vec![
            ("~@bindable".into(), "P7_NAME".into(), ":P7_NAME".into()),
            ("~@bindable".into(), "P0_GLOBAL".into(), ":p0_global".into()),
            (
                "@bindable".into(),
                "P7_CUSTOMER_ID".into(),
                ":P7_CUSTOMER_ID".into()
            ),
        ]
    );
    let scopes: Vec<String> = ex
        .refs
        .iter()
        .filter(|r| r.hints.contains_key("apexlang_bind_variable"))
        .map(|r| apexlang_ref::parse(&r.raw_name).unwrap().scope)
        .collect();
    // Item names are unique in an application (a copied page keeps `P3_X` on page 503), so the
    // scope is the application, not the page the name suggests.
    assert!(
        scopes.iter().all(|s| s == "apexlang . . . app/app#"),
        "{scopes:?}"
    );
    // Exact site.
    let r = ex
        .refs
        .iter()
        .find(|r| {
            r.hints
                .get("apexlang_bind_variable")
                .and_then(|v| v.as_str())
                == Some(":P7_CUSTOMER_ID")
        })
        .unwrap();
    let s = r.location.span;
    assert_eq!(
        &PAGE7[s.start_byte as usize..s.end_byte as usize],
        ":P7_CUSTOMER_ID"
    );
    assert_eq!(s.start_line, 17);
    // The owner is the component holding the block.
    assert_eq!(
        r.from.as_str(),
        "apexlang . . . app/app#page:7#pageItem:P7_CONTACT#"
    );
}

/// The bind-variable pattern is data.
#[test]
fn bind_variable_pattern_is_data() {
    let toml = include_str!("../apexlang.toml").replace(
        "pattern = '(?i):(?P<item>P[0-9]+_[A-Za-z0-9_$#]+)'",
        "pattern = '&(?P<item>P[0-9]+_[A-Za-z0-9_$#]+)\\.'",
    );
    assert!(
        ApexlangData::parse(&toml.replace("(?P<item>", "(")).is_err(),
        "the `item` group is checked"
    );
    let ex = ApexlangExtractor::with_data(ApexlangData::parse(&toml).unwrap());
    let src = "region r (\n    html:\n        ```sql\n        select '&P3_X.' from dual where :P3_Y = 1\n        ```\n)\n";
    let got = bind_refs(&ex.extract(&sf("app/pages/p00003-x.apx", src)).unwrap());
    // `&P3_X.` sits inside a string literal of the sql region, so even the new pattern skips it;
    // `:P3_Y` no longer matches.
    assert!(got.is_empty(), "{got:?}");
    let src = src.replace("'&P3_X.'", "&P3_X.");
    let got = bind_refs(&ex.extract(&sf("app/pages/p00003-x.apx", &src)).unwrap());
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].1, "P3_X");
}

/// Every bind variable of a fixture page becomes a ref at its exact site, and nothing else does.
#[test]
fn fixture_slice_bind_variables_are_all_emitted_at_their_sites() {
    let path = format!("{FIXTURES}/example-1/pages/p09999-login.apx");
    let text = std::fs::read_to_string(&path).unwrap();
    let ex = extract("example-1/pages/p09999-login.apx", &text);
    let mut got: Vec<(u32, String)> = ex
        .refs
        .iter()
        .filter(|r| r.hints.contains_key("apexlang_bind_variable"))
        .map(|r| {
            let s = r.location.span;
            (
                s.start_line + 1,
                text[s.start_byte as usize..s.end_byte as usize].to_string(),
            )
        })
        .collect();
    got.sort();
    // Independently listed from the file (`grep -no ':P[0-9]*_[A-Z_]*'`): three PL/SQL blocks.
    let want: Vec<(u32, String)> = [
        (173, ":P9999_USERNAME"),
        (174, ":P9999_REMEMBER"),
        (174, ":P9999_USERNAME"),
        (193, ":P9999_USERNAME"),
        (194, ":P9999_PASSWORD"),
        (195, ":P9999_PERSISTENT_AUTH"),
        (213, ":P9999_USERNAME"),
        (214, ":P9999_REMEMBER"),
    ]
    .iter()
    .map(|(l, s)| (*l, s.to_string()))
    .collect();
    assert_eq!(got, want);
}

/// Oracle alternative quoting and quoted identifiers hide `:P..` from the text scan, and a real
/// bind right after each is still found.
#[test]
fn text_scan_skips_alternative_quotes_and_quoted_identifiers() {
    let src = "process p (\n    source {\n        plsqlCode:\n            ```plsql\n            x := q'[can't :P7_FAKE]';\n            :P7_REAL := 1;\n            y := Q'{:P7_FAKE2}';\n            select \":P7_FAKE3\" into :P7_REAL2 from dual;\n            z := nq'[can't :P7_FAKE4]';\n            :P7_REAL3 := 2;\n            ```\n    }\n)\n";
    let got: Vec<String> = bind_refs(&extract("app/pages/p00007-x.apx", src))
        .into_iter()
        .map(|(_, slug, _)| slug)
        .collect();
    assert_eq!(got, vec!["P7_REAL", "P7_REAL2", "P7_REAL3"]);
    // The same in a block the SQL grammar parses (it has no q-quote literal of its own).
    let sql = "region r (\n    source {\n        sqlQuery:\n            ```sql\n            select q'[can't :P7_FAKE]' from dual\n            where id = :P7_REAL\n            ```\n    }\n)\n";
    let got = bind_refs(&extract("app/pages/p00007-x.apx", sql));
    assert_eq!(
        got,
        vec![("@bindable".into(), "P7_REAL".into(), ":P7_REAL".into())]
    );
}

/// Two owners with the same key, and a repeated key, still mint distinct regions and distinct
/// embedded symbols.
#[test]
fn region_identity_is_unique_per_host() {
    let host = sf(
        "h.apx",
        "x\nfunction f() {}\ny\nfunction f() {}\nz\nfunction f() {}\n",
    );
    let span = |line: u32, start: u32| Span {
        start_byte: start,
        end_byte: start + 16,
        start_line: line,
        start_col: 0,
        end_line: line + 1,
        end_col: 0,
    };
    let region = |owner: &str, key: &str, s: Span| EmbeddedRegion {
        owner: SymbolId(owner.into()),
        key: key.into(),
        tag: Some("javascript".into()),
        span: s,
    };
    let out = extract_regions(
        &host,
        &[
            region("a", "script", span(1, 2)),
            region("b", "script", span(3, 20)),
            region("c", "a.b", span(5, 38)),
        ],
        EmbedTable::builtin(),
    );
    let markers: std::collections::BTreeSet<_> =
        out.regions.iter().map(|r| r.symbol.clone()).collect();
    assert_eq!(markers.len(), 3);
    let fns: std::collections::BTreeSet<_> = out
        .extraction
        .nodes
        .iter()
        .filter(|n| n.name == "f")
        .map(|n| n.symbol.clone())
        .collect();
    assert_eq!(fns.len(), 3, "{fns:?}");
    // Top-level code is owned by a marker that speaks the embedded language.
    let js_marker = out
        .extraction
        .nodes
        .iter()
        .find(|n| n.symbol == out.regions[0].symbol)
        .unwrap();
    assert_eq!(js_marker.language.as_str(), "javascript");
}

/// A block the minified/huge guard skips is an unparsed marker with the reason, not "parsed".
#[test]
fn a_skipped_block_is_not_reported_as_parsed() {
    let long = format!("var a = '{}';", "x".repeat(60_000));
    let src =
        format!("region r (\n    js:\n        ```javascript-browser\n{long}\n        ```\n)\n");
    let ex = extract("app/pages/p00001-x.apx", &src);
    let marker = regions_of(&ex)[0];
    assert_eq!(marker.metadata.get(PARSED_KEY), Some(&false.into()));
    assert!(
        marker.metadata[REASON_KEY]
            .as_str()
            .unwrap()
            .contains("minified")
    );
}

/// Secrets inside an embedded block are masked with that language's rules before extraction.
#[test]
fn embedded_secrets_are_redacted_like_their_language() {
    use wicked_estate_extract::redact::redact_source;
    let src = "region r (\n    js:\n        ```javascript-browser\n        const password = \"hunter2\";\n        ```\n    plsql:\n        ```plsql\n        l_password := 'hunter3';\n        apex_util.set_password(p_password => :P1_PWD);\n        ```\n    other:\n        ```\n        secret = 'hunter4'\n        ```\n)\n";
    let masked = redact_source("app/pages/p00001-x.apx", src);
    assert_eq!(masked.len(), src.len());
    for s in ["hunter2", "hunter3", "hunter4"] {
        assert!(!masked.contains(s), "{s} leaked: {masked}");
    }
    assert!(
        masked.contains(":P1_PWD"),
        "a bind variable is code, not a secret"
    );
    // One pass of masks, so redaction is idempotent.
    assert_eq!(redact_source("app/pages/p00001-x.apx", &masked), masked);
    let ex = extract("app/pages/p00001-x.apx", &masked);
    assert!(
        ex.nodes
            .iter()
            .all(|n| { n.signature.as_deref().is_none_or(|s| !s.contains("hunter")) })
    );
    // The host's own references are untouched.
    let page = "page 1 (\n    security {\n        authorizationScheme: @admin\n    }\n)\n";
    assert_eq!(redact_source("app/pages/p00001-x.apx", page), page);
}

#[test]
fn export_metadata_version_is_stored_on_the_application_node() {
    assert_eq!(
        export_metadata_path("x/app/application.apx").as_deref(),
        Some("x/app/.apex/apexlang.json")
    );
    assert_eq!(export_metadata_path("x/app/pages/p1.apx"), None);
    let app = "app APP (\n    name: App\n)\n";
    let version = |sidecar: Option<&[u8]>| {
        let mut ex = extract("x/app/application.apx", app);
        apply_export_metadata(&mut ex, "x/app/application.apx", sidecar);
        let n = ex
            .nodes
            .iter()
            .find(|n| n.metadata.get("apexlang_kind").and_then(|v| v.as_str()) == Some("app"))
            .unwrap()
            .clone();
        (
            n.metadata[MMD_VERSION_KEY].as_str().unwrap().to_string(),
            n.metadata[MMD_SUPPORTED_KEY].as_bool().unwrap(),
        )
    };
    assert_eq!(
        version(Some(br#"{ "mmdVersion" : "26.2.0+3479" }"#)),
        ("26.2.0+3479".into(), true)
    );
    // An unknown major is extracted, flagged and reported loudly (stderr marker).
    assert_eq!(
        version(Some(br#"{ "mmdVersion" : "27.1.0+1" }"#)),
        ("27.1.0+1".into(), false)
    );
    // Missing or unreadable: unknown, never a failure.
    assert_eq!(version(None), ("unknown".into(), false));
    assert_eq!(version(Some(b"not json")), ("unknown".into(), false));
}

/// Facets are bindable like page items; an item's own SQL naming itself is not a reference.
#[test]
fn facets_are_bindable_and_self_references_are_skipped() {
    let src = "page 158 (\n    region kb (\n        facet P158_KB_COLUMN_ID (\n        )\n    )\n    pageItem P158_X (\n        lov {\n            sqlQuery:\n                ```sql\n                select d, r from t where r = :P158_X or c = :P158_KB_COLUMN_ID\n                ```\n        }\n    )\n)\n";
    let ex = extract("app/pages/p00158-kanban.apx", src);
    let facet = ex
        .nodes
        .iter()
        .find(|n| n.name == "P158_KB_COLUMN_ID")
        .unwrap();
    assert_eq!(
        facet.metadata.get(apexlang_ref::BINDABLE_KEY),
        Some(&true.into())
    );
    let got: Vec<String> = bind_refs(&ex).into_iter().map(|(_, s, _)| s).collect();
    assert_eq!(
        got,
        vec!["P158_KB_COLUMN_ID"],
        "the item's own name is skipped"
    );
}
