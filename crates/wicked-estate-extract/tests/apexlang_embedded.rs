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
            ("~pageItem".into(), "P7_NAME".into(), ":P7_NAME".into()),
            ("~pageItem".into(), "P0_GLOBAL".into(), ":p0_global".into()),
            (
                "pageItem".into(),
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
    assert_eq!(scopes[0], "apexlang . . . app/app#page:7#");
    assert_eq!(
        scopes[1], "apexlang . . . app/app#page:0#",
        "the page comes from the name"
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
        "pattern = '(?i):(?P<item>P(?P<page>[0-9]+)_[A-Za-z0-9_$#]+)'",
        "pattern = '&(?P<item>P(?P<page>[0-9]+)_[A-Za-z0-9_$#]+)\\.'",
    );
    assert!(
        ApexlangData::parse(&toml.replace("(?P<page>", "(")).is_err(),
        "named groups checked"
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

/// Every bind variable of the fixture slice becomes a ref at its exact site.
#[test]
fn fixture_slice_bind_variables_are_all_emitted_at_their_sites() {
    let path = format!("{FIXTURES}/example-1/pages/p09999-login.apx");
    let text = std::fs::read_to_string(&path).unwrap();
    let ex = extract("example-1/pages/p09999-login.apx", &text);
    let binds: Vec<_> = ex
        .refs
        .iter()
        .filter(|r| r.hints.contains_key("apexlang_bind_variable"))
        .collect();
    assert!(!binds.is_empty());
    for r in binds {
        let s = r.location.span;
        let site = &text[s.start_byte as usize..s.end_byte as usize];
        assert!(site.to_ascii_uppercase().starts_with(":P9999_"), "{site}");
    }
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
