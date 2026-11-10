//! BUILD-GATE G7.1 / G7.9 / G7.10 for wicked-estate#405 (DES-S7 §5): no planted secret value is
//! stored in any table, the WAL, the shared-memory file, decompressed content or the full-text
//! index, nor printed by any CLI read command or returned by any MCP tool, after a FRESH index
//! and after MIGRATING a store written before redaction. Keys of admitted files stay; keys of
//! excluded files (`.env`) never appear.
//!
//! Every planted value here is a FAKE: made-up strings and fake token shapes, never a credential.

use std::path::{Path, PathBuf};
use std::process::Command;

use wicked_estate_core::{GraphRead, GraphWrite, Language, Location, Node, NodeKind, Span, Symbol};
use wicked_estate_store::{GraphStoreMutExt, SqliteStore};

/// Planted VALUES (each must be absent everywhere).
const PLANTED: &[&str] = &[
    "PLANTEDcomposeSecret9f8e7d", // compose environment (map form)
    "PLANTEDlistSecret4b5c6d",    // compose environment (list form)
    "PLANTEDhelmSecret1a2b3c",    // Helm values.yaml
    "PLANTEDhelmSecretsFile77",   // Helm secrets.yaml (structural deny)
    "UExBTlRFRGs4c1NlY3JldA==",   // k8s Secret data
    "PLANTEDstringData55aa",      // k8s Secret stringData
    "ghp_PLANTEDconfigMapTok0123456789abcdefgh", // ConfigMap token-shaped value
    "PLANTEDuserinfoPw31",        // URL userinfo
    "PLANTEDqueryTok88",          // URL ?token=
    "PLANTEDfragment12",          // URL #fragment
    "rosebud7", // short low-entropy value under an innocuous key, URL path, host label, topic
    "PLANTEDcodeSecretXYZ", // code string literal
    "PLANTEDdotenvValue42", // committed .env (excluded file)
    "PLANTEDcredFile44", // committed credentials.json (excluded file)
    "PLANTEDtfvarsPw61", // terraform variables file
    "PLANTEDpropsLike71", // INI file
];
/// Token-shaped values planted through the ingress that never pass the extractor (G7.9).
const INGRESS: &[&str] = &[
    "ghp_PLANTEDcatalogName0123456789abcdefg", // captured by a drop-in extra-edge rule
    "AKIAPLANTEDINGRESS01",                    // annotation value
    "ghp_PLANTEDmissQuery0123456789abcdefghij", // telemetry search-miss query
    "AKIAPLANTEDTFSTATE01",                    // tfstate resource id
    "ghp_PLANTEDaccessItem0123456789abcdefghi", // telemetry access-log item id
];
/// Keys of admitted files: present after the index.
const ADMITTED_KEYS: &[&str] = &[
    "DB_PASSWORD",
    "DB_P2",
    "CALLBACK_URL",
    "stringData",
    "db_password",
];
/// Keys of excluded files: never present.
const EXCLUDED_KEYS: &[&str] = &["ENVONLY_KEY_Q9", "CREDFILE_KEY_Q9"];

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_wicked-estate")
}

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, fill(text)).unwrap();
}

/// Planted FAKE values are assembled at runtime so no secret-shaped assignment appears in the
/// source text (secret scanners rightly flag those even when the value is a fake).
fn fill(src: &str) -> String {
    src.replace("@DB_PW_KEY@", &["DB_", "PASS", "WORD"].concat())
        .replace("@PW_KEY@", &["pass", "word"].concat())
        .replace(
            "@COMPOSE_PW@",
            &["PLANTED", "compose", "Secret9f8e7d"].concat(),
        )
        .replace(
            "@PG_USERINFO@",
            &["app:", "PLANTEDuserinfo", "Pw31"].concat(),
        )
        .replace("@HELM_PW@", &["PLANTED", "helm", "Secret1a2b3c"].concat())
        .replace("@K8S_B64@", &["UExBTlRF", "RGs4c1Nl", "Y3JldA=="].concat())
        .replace("@STRING_DATA@", &["PLANTED", "string", "Data55aa"].concat())
}

/// The planted fixture repository (`notes/evidence/repro.sh` secfix, widened to every G7.1 row).
fn fixture(root: &Path) {
    write(
        root,
        "docker-compose.yml",
        "services:\n  api:\n    image: acme/api:1.2\n    environment:\n      @DB_PW_KEY@: @COMPOSE_PW@\n      DB_P2: rosebud7\n      API_BASE_URL: http://orders:8080/v1\n      PG_URL: postgres://@PG_USERINFO@@db.example.test:5432/app\n      HOOK_URL: https://hooks.example.test/x?token=PLANTEDqueryTok88#PLANTEDfragment12\n      CALLBACK_URL: https://example.test/rosebud7/hook\n      EDGE_URL: https://rosebud7.example.test/api\n      ORDERS_TOPIC: rosebud7\n  worker:\n    image: acme/worker:1\n    environment:\n      - LIST_SECRET=PLANTEDlistSecret4b5c6d\n",
    );
    write(
        root,
        "chart/values.yaml",
        "db:\n  @PW_KEY@: @HELM_PW@\norders:\n  baseUrl: http://orders.svc:8080\n",
    );
    write(
        root,
        "chart/secrets.yaml",
        "apiKey: PLANTEDhelmSecretsFile77\n",
    );
    write(
        root,
        "k8s/secret.yaml",
        "apiVersion: v1\nkind: Secret\nmetadata:\n  name: db-creds\ndata:\n  @PW_KEY@: @K8S_B64@\nstringData:\n  token: @STRING_DATA@\n",
    );
    write(
        root,
        "k8s/cm.yaml",
        "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: cfg\ndata:\n  SOME_SETTING: ghp_PLANTEDconfigMapTok0123456789abcdefgh\n",
    );
    write(
        root,
        "app.js",
        "const t = process.env.TOKEN || \"PLANTEDcodeSecretXYZ\";\nexport function run() { return t; }\n",
    );
    write(root, ".env", "ENVONLY_KEY_Q9=PLANTEDdotenvValue42\n");
    write(
        root,
        ".env.example",
        "ENVONLY_KEY_Q9=PLANTEDdotenvValue42\n",
    );
    write(
        root,
        "credentials.json",
        "{\"CREDFILE_KEY_Q9\": \"PLANTEDcredFile44\"}\n",
    );
    write(
        root,
        "infra/prod.tfvars",
        "db_password = \"PLANTEDtfvarsPw61\"\nregion = \"us-east-1\"\n",
    );
    write(root, "conf/app.ini", "[db]\nhost = PLANTEDpropsLike71\n");
    // A drop-in rule (a user-declared identity extractor) over a catalog holding a token shape.
    write(
        root,
        "catalog.json",
        "{\"name\":\"ghp_PLANTEDcatalogName0123456789abcdefg\"}\n",
    );
    write(
        root,
        ".wicked-estate-extractors/catalog.toml",
        "[[rule]]\nname = \"catalog\"\nfile_glob = \"catalog.json\"\npattern = '\"name\":\"(?P<n>[^\"]+)\"'\n\n[rule.emit_node]\nid_template = \"cat:{n}\"\nlabel_capture = \"n\"\nkind = \"synthetic\"\nnode_scheme = \"catalog\"\n",
    );
}

/// Every place a value could sit: the db, WAL and shm files raw; every text or blob cell of
/// every table (blobs also zstd-decoded); the full-text index's term vocabulary.
fn scan_db(db: &Path, needles: &[&str]) -> Vec<String> {
    let mut hits = Vec::new();
    for suffix in ["", "-wal", "-shm"] {
        let p = PathBuf::from(format!("{}{suffix}", db.display()));
        if let Ok(bytes) = std::fs::read(&p) {
            for n in needles {
                if contains(&bytes, n.as_bytes()) {
                    hits.push(format!("raw file {suffix:?}: {n}"));
                }
            }
        }
    }
    let conn = rusqlite::Connection::open(db).unwrap();
    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    for t in &tables {
        let mut stmt = conn.prepare(&format!("SELECT * FROM \"{t}\"")).unwrap();
        let cols = stmt.column_count();
        let mut rows = stmt.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            for i in 0..cols {
                let cell: Vec<u8> = match row.get_ref(i).unwrap() {
                    rusqlite::types::ValueRef::Text(b) => b.to_vec(),
                    rusqlite::types::ValueRef::Blob(b) => {
                        let mut v = b.to_vec();
                        if let Ok(d) = zstd::decode_all(b) {
                            v.extend(d);
                        }
                        v
                    }
                    _ => continue,
                };
                for n in needles {
                    if contains(&cell, n.as_bytes()) {
                        hits.push(format!("table {t}: {n}"));
                    }
                }
            }
        }
    }
    conn.execute_batch("CREATE VIRTUAL TABLE temp.vocab USING fts5vocab(main, nodes_fts, row);")
        .unwrap();
    let terms: Vec<String> = conn
        .prepare("SELECT term FROM temp.vocab")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    for n in needles {
        let low = n.to_lowercase();
        if terms.iter().any(|t| t.contains(&low)) {
            hits.push(format!("fts vocabulary: {n}"));
        }
    }
    hits
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

fn all_content(db: &Path) -> String {
    let conn = rusqlite::Connection::open(db).unwrap();
    let mut stmt = conn.prepare("SELECT blob FROM content").unwrap();
    let blobs: Vec<Vec<u8>> = stmt
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    blobs
        .iter()
        .map(|b| String::from_utf8(zstd::decode_all(&b[..]).unwrap()).unwrap())
        .collect::<Vec<_>>()
        .join("\n")
}

fn run(args: &[&str]) -> (bool, String) {
    let out = Command::new(bin())
        .args(args)
        .output()
        .expect("run wicked-estate");
    let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
    s.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.success(), s)
}

/// The CLI read surface a user or agent reaches for (each command's default call).
fn cli_answers(db: &str) -> String {
    let mut all = String::new();
    for args in [
        vec!["stats"],
        vec!["stats", "--json"],
        vec!["query", "run"],
        vec!["query", "DB_PASSWORD"],
        vec!["blast-radius", "run", "--json"],
        vec!["source", "--file", "app.js"],
        vec!["source", "--file", "docker-compose.yml", "--json"],
        vec!["context", "run", "--json"],
        vec!["nodes", "--json"],
        vec!["entrypoints", "--json"],
        vec!["leaves", "--json"],
        vec!["dead-code", "--json"],
        vec!["graph-view"],
        vec!["annotations", "run", "--json"],
        vec!["misses", "--json"],
        vec!["rank", "--json"],
        vec!["traverse", "run", "--json"],
        vec!["stale-annotations", "--older-than", "1s", "--json"],
        vec!["supports", "owners", "--json"],
    ] {
        let mut a = args.clone();
        a.extend(["--db", db]);
        let (_, out) = run(&a);
        all.push_str(&out);
    }
    all
}

/// Every MCP code tool's default call (required arguments filled from its input schema).
fn mcp_answers(db: &Path) -> String {
    let store = SqliteStore::open_readonly(db).expect("open read-only");
    let mut all = String::new();
    for tool in wicked_estate_mcp::all_tools() {
        let name = tool.name().to_string();
        let mut args = serde_json::Map::new();
        if let Some(schema) = wicked_estate_mcp::input_schema(&name) {
            let props = schema.get("properties").cloned().unwrap_or_default();
            for req in schema
                .get("required")
                .and_then(|r| r.as_array())
                .cloned()
                .unwrap_or_default()
            {
                let key = req.as_str().unwrap_or_default().to_string();
                let ty = props
                    .get(&key)
                    .and_then(|p| p.get("type"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("string");
                let v = match ty {
                    "integer" | "number" => serde_json::json!(3),
                    "boolean" => serde_json::json!(false),
                    "array" => serde_json::json!(["run"]),
                    "object" => serde_json::json!({}),
                    _ => serde_json::json!("run"),
                };
                args.insert(key, v);
            }
        }
        let req = serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": name, "arguments": args }
        });
        let resp = wicked_estate_mcp::handle_request(&store, &req);
        all.push_str(&resp.to_string());
    }
    // A targeted call that must return the (masked) source, so the sweep is not vacuous.
    let id = store
        .find_symbols(&wicked_estate_core::SymbolQuery {
            exact_name: Some("run".into()),
            ..Default::default()
        })
        .unwrap()
        .into_iter()
        .next()
        .map(|n| n.symbol.0);
    if let Some(id) = id {
        let req = serde_json::json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": { "name": "FetchContent", "arguments": { "symbol": id } }
        });
        all.push_str(&wicked_estate_mcp::handle_request(&store, &req).to_string());
        all.push_str("\n@@FETCHED@@");
    }
    all
}

/// The answers must carry real data (a failing or empty command cannot pass the no-leak checks
/// vacuously): `query run` names the function; `source --file app.js` returns the masked line.
fn assert_answers_have_data(db: &str) {
    let (ok, out) = run(&["query", "run", "--db", db]);
    assert!(ok && out.contains("run"), "query run: {out}");
    let (ok, out) = run(&["source", "--file", "app.js", "--db", db]);
    assert!(
        ok && out.contains("process.env.TOKEN") && out.contains("PRIVACY:"),
        "source: {out}"
    );
}

fn assert_clean(what: &str, hits: &[String]) {
    assert!(
        hits.is_empty(),
        "{what}: planted values found:\n{}",
        hits.join("\n")
    );
}

fn assert_no_value(what: &str, text: &str, needles: &[&str]) {
    let found: Vec<&&str> = needles.iter().filter(|n| text.contains(**n)).collect();
    assert!(
        found.is_empty(),
        "{what}: planted values in output: {found:?}"
    );
}

fn needles() -> Vec<&'static str> {
    PLANTED
        .iter()
        .chain(INGRESS.iter())
        .chain(EXCLUDED_KEYS.iter())
        .copied()
        .collect()
}

/// Plant the G7.9 ingress values through the CLI paths that bypass the extractor.
fn plant_ingress(dir: &Path, db: &str) {
    let tf = dir.join("state.tfstate");
    std::fs::write(
        &tf,
        r#"{"version":4,"resources":[{"mode":"managed","type":"aws_instance","name":"web","provider":"provider[\"registry.terraform.io/hashicorp/aws\"]","instances":[{"attributes":{"id":"AKIAPLANTEDTFSTATE01"}}]}]}"#,
    )
    .unwrap();
    let (ok, out) = run(&["tfstate", tf.to_str().unwrap(), "--db", db]);
    assert!(ok, "tfstate: {out}");
    let tel = dir.join("telemetry.json");
    std::fs::write(
        &tel,
        r#"{"search_misses":[{"query":"where is ghp_PLANTEDmissQuery0123456789abcdefghij","searched_at":1}],"access_log":[{"item_id":"ghp_PLANTEDaccessItem0123456789abcdefghi","session_id":"s","accessed_at":1}]}"#,
    )
    .unwrap();
    let (ok, out) = run(&["import-telemetry", tel.to_str().unwrap(), "--db", db]);
    assert!(ok, "import-telemetry: {out}");
    let (ok, out) = run(&[
        "annotate",
        "run",
        "--key",
        "note",
        "--value",
        "pasted AKIAPLANTEDINGRESS01 here",
        "--db",
        db,
    ]);
    assert!(ok, "annotate: {out}");
}

#[test]
fn fresh_index_stores_and_serves_no_planted_value() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("repo");
    fixture(&root);
    let db = dir.path().join("g.db");
    let dbs = db.to_str().unwrap();
    // G7.6: content outside the root, reachable only through symlinks, is never read.
    let outside = dir.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("canary.yaml"), "k: PLANTEDoutsideCanary90\n").unwrap();
    std::fs::write(outside.join("rule.toml"), "# PLANTEDoutsideRule91\n").unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(outside.join("canary.yaml"), root.join("linked.yaml")).unwrap();
        std::fs::create_dir_all(root.join(".wicked-estate-extractors")).unwrap();
        std::os::unix::fs::symlink(
            outside.join("rule.toml"),
            root.join(".wicked-estate-extractors/out.toml"),
        )
        .unwrap();
    }

    let (ok, out) = run(&["index", root.to_str().unwrap(), "--db", dbs]);
    assert!(ok, "index failed: {out}");
    assert_no_value("index output", &out, &needles());
    assert_no_value(
        "index output (outside reads)",
        &out,
        &["PLANTEDoutsideCanary90", "PLANTEDoutsideRule91"],
    );
    assert_clean(
        "outside the root",
        &scan_db(&db, &["PLANTEDoutsideCanary90", "PLANTEDoutsideRule91"]),
    );
    plant_ingress(dir.path(), dbs);

    assert_clean("fresh index", &scan_db(&db, &needles()));
    let content = all_content(&db);
    for k in ADMITTED_KEYS {
        assert!(
            content.contains(k),
            "admitted key {k} missing from stored content"
        );
    }
    // G7.10: the admitted-suffix shapes keep their public parts only.
    assert!(
        content.contains("https://example.test/REDACTED/REDA"),
        "CALLBACK_URL: {content}"
    );
    assert!(
        content.contains(".example.test/api"),
        "EDGE_URL host kept: {content}"
    );
    assert!(
        content.contains("image: acme/api:1.2"),
        "trusted compose field kept"
    );
    assert_no_value("CLI answers", &cli_answers(dbs), &needles());
    let mcp = mcp_answers(&db);
    assert!(
        mcp.contains("@@FETCHED@@") && mcp.contains("PRIVACY:"),
        "FetchContent answered with source"
    );
    assert_no_value("MCP answers", &mcp, &needles());
    assert_answers_have_data(dbs);
}

/// A store as estate 0.32.1 wrote it: raw file texts in `content`, raw first lines as node
/// signatures (and therefore in the full-text index), `indexed_root` set, no redaction epoch.
fn old_store(root: &Path, db: &Path) {
    let mut s = SqliteStore::open(db).unwrap();
    s.begin_batch().unwrap();
    let mut nodes = Vec::new();
    for rel in [
        "docker-compose.yml",
        "chart/values.yaml",
        "chart/secrets.yaml",
        "k8s/secret.yaml",
        "k8s/cm.yaml",
        "app.js",
        "infra/prod.tfvars",
        "conf/app.ini",
    ] {
        let raw = std::fs::read_to_string(root.join(rel)).unwrap();
        s.set_file_content(rel, &raw).unwrap();
        s.set_file_digest(rel, "stale-digest").unwrap();
        let mut offset = 0u32;
        for (i, line) in raw.lines().enumerate() {
            let mut n = Node::new(
                Symbol::file(format!("{rel}#{i}")).id(),
                NodeKind::Variable,
                format!("line{i}"),
                Language::new("text"),
                Location::new(
                    rel,
                    Span {
                        start_byte: offset,
                        end_byte: offset + line.len() as u32,
                        start_line: i as u32,
                        start_col: 0,
                        end_line: i as u32,
                        end_col: line.len() as u32,
                    },
                ),
            );
            n.signature = Some(line.to_string());
            nodes.push(n);
            offset += line.len() as u32 + 1;
        }
    }
    s.upsert_nodes(&nodes).unwrap();
    s.commit_batch().unwrap();
    s.meta_set_key("indexed_root", root.to_str().unwrap());
    drop(s);
    // Rows an older build wrote that the new write backstop would refuse: inserted raw. An older
    // build never recorded a redaction epoch (this build stamps one on an empty store).
    let conn = rusqlite::Connection::open(db).unwrap();
    conn.execute("DELETE FROM meta WHERE k='redaction_epoch'", [])
        .unwrap();
    let ident = "route synthetic /reset/ghp_PLANTEDidentToken0123456789abcd:";
    conn.execute("INSERT INTO symbols(sym) VALUES(?1)", [ident])
        .unwrap();
    let sid: i64 = conn
        .query_row("SELECT sid FROM symbols WHERE sym=?1", [ident], |r| {
            r.get(0)
        })
        .unwrap();
    let mut route = Node::new(
        wicked_estate_core::SymbolId(ident.into()),
        NodeKind::Other("route".into()),
        "/reset/ghp_PLANTEDidentToken0123456789abcd",
        Language::new("java"),
        Location::new("app.js", Span::ZERO),
    );
    route.signature = Some("ghp_PLANTEDidentToken0123456789abcd".into());
    conn.execute(
        "INSERT INTO nodes(symbol,name,kind,language,file,data,scope) VALUES(?1,?2,'route','java','app.js',?3,'')",
        rusqlite::params![sid, route.name, serde_json::to_string(&route).unwrap()],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO nodes_fts(symbol,name,signature,doc) VALUES(?1,?2,?3,'')",
        rusqlite::params![ident, route.name, route.signature],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO annotations(node_sym,key,value) VALUES(?1,'note','pasted AKIAPLANTEDOLDANNOT1')",
        [sid],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO search_misses(query, searched_at) VALUES('where is AKIAPLANTEDOLDMISS01', 1)",
        [],
    )
    .unwrap();
    let mut fact = wicked_estate_core::Edge::new(
        wicked_estate_core::SymbolId(ident.into()),
        wicked_estate_core::SymbolId(ident.into()),
        wicked_estate_core::EdgeKind::Calls,
        wicked_estate_core::ResolutionTier::Scip,
        "scip-t",
    );
    fact.metadata.insert(
        "scip_symbol".into(),
        serde_json::json!("pkg `ghp_PLANTEDsupportFact0123456789abc`."),
    );
    conn.execute(
        "INSERT INTO support_owners(producer,snapshot,generation) VALUES('scip','s',1)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO edge_supports(producer,snapshot,source,target,kind,fact_id,data) VALUES('scip','s',?1,?1,'\"calls\"','f1',?2)",
        rusqlite::params![sid, serde_json::to_string(&fact).unwrap()],
    )
    .unwrap();
    // A file whose text an older build never stored: its signature cannot be re-sliced.
    conn.execute("INSERT INTO files(path,digest) VALUES('gone.js','x')", [])
        .unwrap();
    let gone_sym = "ts-javascript . . . gone/x.";
    conn.execute("INSERT INTO symbols(sym) VALUES(?1)", [gone_sym])
        .unwrap();
    let gsid: i64 = conn
        .query_row("SELECT sid FROM symbols WHERE sym=?1", [gone_sym], |r| {
            r.get(0)
        })
        .unwrap();
    let mut gone = Node::new(
        wicked_estate_core::SymbolId(gone_sym.into()),
        NodeKind::Variable,
        "x",
        Language::new("javascript"),
        Location::new("gone.js", Span::ZERO),
    );
    gone.signature = Some("const password = \"PLANTEDnoContent73\";".into());
    conn.execute(
        "INSERT INTO nodes(symbol,name,kind,language,file,data,scope) VALUES(?1,'x','variable','javascript','gone.js',?2,'')",
        rusqlite::params![gsid, serde_json::to_string(&gone).unwrap()],
    )
    .unwrap();
    // A stored text holding a token shape (the backstop would have masked it on write).
    let tok = "token = ghp_PLANTEDcontentToken0123456789abcdef\n";
    let sha = "f00dfeedf00dfeedf00dfeedf00dfeedf00dfeed";
    conn.execute(
        "INSERT INTO content(git_sha, blob) VALUES(?1, ?2)",
        rusqlite::params![sha, zstd::encode_all(tok.as_bytes(), 3).unwrap()],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO files(path,digest,git_sha) VALUES('tok.txt','x',?1)",
        [sha],
    )
    .unwrap();
    drop(conn);
    let hits = scan_db(db, &old_needles());
    for must in [
        "PLANTEDcodeSecretXYZ",
        "ghp_PLANTEDidentToken",
        "AKIAPLANTEDOLDANNOT1",
        "PLANTEDnoContent73",
        "ghp_PLANTEDcontentToken",
        "AKIAPLANTEDOLDMISS01",
        "ghp_PLANTEDsupportFact",
    ] {
        assert!(
            hits.iter().any(|h| h.contains(must)),
            "fixture must hold old {must}: {hits:?}"
        );
    }
}

/// Every planted value of the old-store fixture, the raw-row ones included.
fn old_needles() -> Vec<&'static str> {
    PLANTED
        .iter()
        .copied()
        .chain([
            "ghp_PLANTEDidentToken0123456789abcd",
            "AKIAPLANTEDOLDANNOT1",
            "PLANTEDnoContent73",
            "ghp_PLANTEDcontentToken0123456789abcdef",
            "AKIAPLANTEDOLDMISS01",
            "ghp_PLANTEDsupportFact0123456789abc",
        ])
        .collect()
}

#[test]
fn an_unmigrated_store_serves_no_planted_value_and_migration_cleans_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("repo");
    fixture(&root);
    let db = dir.path().join("old.db");
    let dbs = db.to_str().unwrap();
    old_store(&root, &db);

    // Serve-time guard: before any migration, no answer returns an old value.
    {
        let ro = SqliteStore::open_readonly(&db).unwrap();
        assert!(ro.redaction_pending(), "old store is pending");
        assert!(
            ro.file_content("app.js").unwrap().is_none(),
            "source text withheld"
        );
    }
    let mut cli = cli_answers(dbs);
    let ident = "route synthetic /reset/ghp_PLANTEDidentToken0123456789abcd:";
    let (_, sup) = run(&[
        "supports", "edge", "--source", ident, "--target", ident, "--kind", "calls", "--json",
        "--db", dbs,
    ]);
    // The command echoes the ids it was given (input, not stored data): check the support fact's
    // stored values without them.
    let sup_stored: Vec<&str> = old_needles()
        .into_iter()
        .filter(|n| !ident.contains(n))
        .collect();
    assert_no_value(
        "supports edge (unmigrated)",
        &sup.replace(ident, ""),
        &sup_stored,
    );
    assert!(sup.contains("scip"), "the support fact is answered: {sup}");
    let (_, misses) = run(&["misses", "--json", "--db", dbs]);
    cli.push_str(&misses);
    assert!(
        cli.contains("REDACTION-PENDING"),
        "loud marker on an unmigrated store"
    );
    assert_no_value("CLI answers (unmigrated)", &cli, &old_needles());
    assert_no_value(
        "MCP answers (unmigrated)",
        &mcp_answers(&db),
        &old_needles(),
    );

    // Explicit migration.
    let (ok, out) = run(&["redact-store", "--db", dbs]);
    assert!(ok, "redact-store: {out}");
    assert_no_value("redact-store output", &out, &old_needles());
    assert_clean("migrated store", &scan_db(&db, &old_needles()));
    assert!(!SqliteStore::open_readonly(&db).unwrap().redaction_pending());

    // Idempotent, and the next index re-extracts from masked text.
    let (ok, out) = run(&["redact-store", "--db", dbs]);
    assert!(
        ok && out.contains("already at the current redaction policy"),
        "{out}"
    );
    let (ok, out) = run(&["index", root.to_str().unwrap(), "--db", dbs]);
    assert!(ok, "index after migration: {out}");
    assert_clean("migrated + re-indexed", &scan_db(&db, &needles()));
    assert_no_value("CLI answers (migrated)", &cli_answers(dbs), &needles());
    assert_no_value("MCP answers (migrated)", &mcp_answers(&db), &needles());
}

#[test]
fn index_migrates_an_old_store_on_its_first_write_open() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("repo");
    fixture(&root);
    let db = dir.path().join("old.db");
    let dbs = db.to_str().unwrap();
    old_store(&root, &db);
    let (ok, out) = run(&["index", root.to_str().unwrap(), "--db", dbs]);
    assert!(ok, "index: {out}");
    assert!(
        out.contains("REDACTION: cleaned a store written before secret redaction"),
        "{out}"
    );
    assert_no_value("index output", &out, &old_needles());
    assert_clean("index-migrated store", &scan_db(&db, &old_needles()));
}

/// Review x0-r1 L1: a code graph written before `indexed_root` existed (file rows, no root) is
/// still recognised as pending and is migrated, never stamped clean.
#[test]
fn a_rootless_old_store_is_pending_and_migrated() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("repo");
    fixture(&root);
    let db = dir.path().join("old.db");
    old_store(&root, &db);
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute("DELETE FROM meta WHERE k='indexed_root'", [])
        .unwrap();
    assert!(SqliteStore::open_readonly(&db).unwrap().redaction_pending());
    let (ok, out) = run(&["redact-store", "--db", db.to_str().unwrap()]);
    assert!(ok && out.contains("file texts re-masked"), "{out}");
    assert_clean("rootless migrated", &scan_db(&db, &old_needles()));
}

/// Review x0-r1 W1: a reader attached during the migration keeps the WAL from truncating; the
/// cleanup stays pending and an idle re-run finishes it.
#[test]
fn a_pinned_reader_defers_cleanup_and_an_idle_rerun_finishes_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("repo");
    fixture(&root);
    let db = dir.path().join("old.db");
    let dbs = db.to_str().unwrap();
    old_store(&root, &db);
    let reader = rusqlite::Connection::open(&db).unwrap();
    reader.execute_batch("BEGIN;").unwrap();
    let _: i64 = reader
        .query_row("SELECT count(*) FROM nodes", [], |r| r.get(0))
        .unwrap();
    let (_, out) = run(&["redact-store", "--db", dbs]);
    assert_no_value("redact-store output (pinned)", &out, &old_needles());
    let pending: Option<String> = rusqlite::Connection::open(&db)
        .unwrap()
        .query_row(
            "SELECT v FROM meta WHERE k='redaction_cleanup_pending'",
            [],
            |r| r.get(0),
        )
        .ok();
    assert!(
        pending.is_some(),
        "cleanup stays pending while a reader is attached: {out}"
    );
    reader.execute_batch("COMMIT;").unwrap();
    drop(reader);
    let (ok, out) = run(&["redact-store", "--db", dbs]);
    assert!(
        ok && out.contains("finished the cleanup"),
        "idle re-run: {out}"
    );
    assert_clean("after the idle re-run", &scan_db(&db, &old_needles()));
}
