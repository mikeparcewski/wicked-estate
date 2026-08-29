//! End-to-end resolver-precision guard (engine defect #2): indexing a mixed-language repo through
//! `index_path` must produce NO Calls edges targeting `Import` nodes and NO cross-family Calls
//! edges (family per `wicked-estate-extract/languages.toml`), while same-family Calls edges
//! still resolve. This is the corpus finding (studio: TS `res.json()` → Python `import json`
//! node; 262 python→ts Calls edges) reduced to a fixture.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use wicked_estate_core::{EdgeKind, GraphRead, NodeKind};
use wicked_estate_store::SqliteStore;

fn fresh_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ci_resprec_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn index_path_emits_no_import_targets_and_no_cross_family_calls() {
    let dir = fresh_dir("mixed");
    // Python side: an `import json` node (an Import node named "json") + a unique function.
    fs::write(
        dir.join("api.py"),
        "import json\n\n\ndef compute():\n    return json.loads(\"{}\")\n",
    )
    .unwrap();
    // TS side: `res.json()` (must NOT bind to the Python import node), `compute()` (must NOT
    // bind cross-family to the Python function), and a same-family call that MUST resolve.
    fs::write(
        dir.join("util.ts"),
        "export function helper(): number {\n  return 1;\n}\n",
    )
    .unwrap();
    fs::write(
        dir.join("client.ts"),
        "import { helper } from './util';\n\
         export function call(res: { json: () => unknown }): unknown {\n\
           compute();\n\
           helper();\n\
           return res.json();\n\
         }\n\
         declare function compute(): void;\n",
    )
    .unwrap();

    let mut store = SqliteStore::in_memory().expect("open sqlite");
    wicked_estate::index_path(&mut store, &dir).expect("index_path");

    let nodes = GraphRead::all_nodes(&store).unwrap();
    let by_sym: HashMap<_, _> = nodes.iter().map(|n| (n.symbol.clone(), n)).collect();

    // The fixture is only meaningful if the Python import node exists.
    assert!(
        nodes
            .iter()
            .any(|n| n.kind == NodeKind::Import && n.name == "json"),
        "fixture must produce a Python Import node named json"
    );

    // Family table straight from the manifest (the same data the resolver uses).
    let families: HashMap<String, String> = wicked_estate_extract::registry()
        .into_iter()
        .map(|l| {
            let fam = l.family().to_string();
            (l.name, fam)
        })
        .collect();

    let calls: Vec<_> = GraphRead::all_edges(&store)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == EdgeKind::Calls)
        .collect();

    let mut same_family_resolved = 0usize;
    for e in &calls {
        let (Some(src), Some(tgt)) = (by_sym.get(&e.source), by_sym.get(&e.target)) else {
            continue;
        };
        assert!(
            tgt.kind != NodeKind::Import,
            "Calls edge must never target an Import node: {} -> {} ({})",
            src.name,
            tgt.name,
            tgt.location.file
        );
        let sf = families.get(src.language.as_str());
        let tf = families.get(tgt.language.as_str());
        if let (Some(sf), Some(tf)) = (sf, tf) {
            assert_eq!(
                sf, tf,
                "cross-family Calls edge must not exist: {} ({}) -> {} ({})",
                src.name, src.language.0, tgt.name, tgt.language.0
            );
            same_family_resolved += 1;
        }
    }

    // Not vacuous: the same-family `helper()` call must still resolve.
    assert!(
        same_family_resolved >= 1,
        "at least the ts->ts helper() call must resolve; got {} same-family Calls edges of {} total",
        same_family_resolved,
        calls.len()
    );

    let _ = fs::remove_dir_all(&dir);
}
