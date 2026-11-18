//! Live LSP round-trip test — W3.3.
//!
//! Launches whatever TypeScript server the registry resolves for the fixture
//! (`ServerRegistry::launch_for`: TypeScript 7's own `tsc --lsp`, or typescript-language-server
//! with `tsserver.path` for 5-6, #484) against a tiny in-test TypeScript fixture, fires
//! `definition`, `hover`, and `references`, and asserts the responses are structurally correct.
//!
//! **No usable TypeScript**: the test prints `[lsp_live] SKIP: <why>` and returns, naming every
//! place the resolver looked. With `WICKED_LSP_LIVE_REQUIRED=1` (estate CI's gate job) that is a
//! failure instead, so CI can never pass by skipping the live tier. `WICKED_LSP_LIVE_EXPECT`
//! (`native` | `tsserver`) asserts which of the two paths was taken.

use std::{fs, thread, time::Duration};

use wicked_estate_resolve::lsp::{
    DEFAULT_TIMEOUT, Launch, Location, LspClient, LspTier, ServerRegistry, path_to_file_uri,
};

// ── helper: the resolved TypeScript launch, or a visible skip (a failure when required) ──

fn typescript_launch_or_skip(root: &str) -> Option<Launch> {
    match ServerRegistry::standard().launch_for("typescript", root) {
        Ok(launch) => {
            println!("[lsp_live] server: {} {:?}", launch.binary, launch.args);
            // CI says which path each step must take, so a step cannot pass on the other one.
            match std::env::var("WICKED_LSP_LIVE_EXPECT").as_deref() {
                Ok("native") => assert!(
                    launch.init_options.is_null() && launch.args == ["--lsp", "--stdio"],
                    "[lsp_live] expected TypeScript 7's own `tsc --lsp`, got {launch:?}"
                ),
                Ok("tsserver") => assert!(
                    launch.init_options["tsserver"]["path"].is_string(),
                    "[lsp_live] expected typescript-language-server with tsserver.path, got {launch:?}"
                ),
                Ok(other) => panic!(
                    "[lsp_live] WICKED_LSP_LIVE_EXPECT must be native or tsserver, got {other:?}"
                ),
                Err(_) => {}
            }
            Some(launch)
        }
        Err(e) => {
            if std::env::var("WICKED_LSP_LIVE_REQUIRED").as_deref() == Ok("1") {
                panic!(
                    "[lsp_live] WICKED_LSP_LIVE_REQUIRED=1 but no TypeScript LSP server is usable: {e}"
                );
            }
            println!("[lsp_live] SKIP: {e}");
            None
        }
    }
}

// ── fixture ───────────────────────────────────────────────────────────────────

/// A tiny TypeScript module with a named function that can be jumped-to.
///
/// Layout written to a temp dir:
/// ```text
/// <tmpdir>/
///   tsconfig.json
///   src/greeter.ts    ← defines `greet(name)` on line 1 (0-based)
///   src/main.ts       ← calls `greet` on line 2 (0-based), col 20
/// ```
fn write_ts_fixture(dir: &std::path::Path) {
    fs::create_dir_all(dir.join("src")).unwrap();

    // Minimal tsconfig so typescript-language-server can load the project.
    fs::write(
        dir.join("tsconfig.json"),
        r#"{ "compilerOptions": { "strict": true, "target": "ES2020" }, "include": ["src/**/*"] }"#,
    )
    .unwrap();

    // greeter.ts:
    //   line 0: // Greeter module
    //   line 1: export function greet(name: string): string {
    //   line 2:   return `Hello, ${name}!`;
    //   line 3: }
    fs::write(
        dir.join("src/greeter.ts"),
        "// Greeter module\nexport function greet(name: string): string {\n  return `Hello, ${name}!`;\n}\n",
    )
    .unwrap();

    // main.ts:
    //   line 0: import { greet } from './greeter';
    //   line 1: (blank)
    //   line 2: const msg: string = greet('world');
    //   line 3: console.log(msg);
    //
    // `greet(` starts at col 20 on line 2.
    fs::write(
        dir.join("src/main.ts"),
        "import { greet } from './greeter';\n\nconst msg: string = greet('world');\nconsole.log(msg);\n",
    )
    .unwrap();
}

// ── the live test ─────────────────────────────────────────────────────────────

#[test]
fn typescript_definition_and_hover_roundtrip() {
    // ── set up tmp workspace ──────────────────────────────────────────────────
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write_ts_fixture(root);

    let root_str = root.to_str().expect("non-UTF8 tempdir path");

    // ── spawn the resolved server (or skip visibly) ───────────────────────────
    let Some(launch) = typescript_launch_or_skip(root_str) else {
        return;
    };
    let mut client = LspClient::spawn_with_options(
        &launch.binary,
        &launch.args,
        root_str,
        DEFAULT_TIMEOUT,
        launch.init_options,
    )
    .expect("LspClient must start against the resolved TypeScript server");

    // ── open documents so the server can answer queries ───────────────────────
    // LSP requires `textDocument/didOpen` before any position-based query.
    let greeter_path = root.join("src/greeter.ts");
    let main_path = root.join("src/main.ts");
    let greeter_uri = path_to_file_uri(greeter_path.to_str().expect("non-UTF8"));
    let main_uri = path_to_file_uri(main_path.to_str().expect("non-UTF8"));
    let greeter_text = fs::read_to_string(&greeter_path).unwrap();
    let main_text = fs::read_to_string(&main_path).unwrap();

    client
        .did_open(&greeter_uri, "typescript", &greeter_text)
        .expect("did_open greeter.ts");
    client
        .did_open(&main_uri, "typescript", &main_text)
        .expect("did_open main.ts");

    // Give tsserver a moment to process the open notifications and build its
    // internal project model before we send queries.
    thread::sleep(Duration::from_millis(500));

    // ── textDocument/definition ───────────────────────────────────────────────
    // main.ts line 2 (0-based): `const msg: string = greet('world');`
    //                                                  ^ col 20
    let defs = client
        .definition(&main_uri, 2, 20)
        .expect("definition request must succeed");

    assert!(
        !defs.is_empty(),
        "expected at least one definition location for `greet` in main.ts:2:20; got none"
    );

    // The definition should point into greeter.ts.
    let def = &defs[0];
    assert!(
        def.uri.contains("greeter"),
        "definition should point to greeter.ts, got URI: {}",
        def.uri
    );

    // `greet` is defined on line 1 of greeter.ts (0-based: `export function greet`).
    assert!(
        def.start_line <= 2,
        "definition should be near the top of greeter.ts, got line {}",
        def.start_line
    );

    println!(
        "[lsp_live] definition: {}:{}:{}",
        def.uri, def.start_line, def.start_col
    );

    // ── textDocument/hover ────────────────────────────────────────────────────
    let hover = client
        .hover(&main_uri, 2, 20)
        .expect("hover request must succeed");

    if let Some(h) = &hover {
        assert!(!h.text.is_empty(), "hover result text should not be empty");
        println!("[lsp_live] hover text: {:?}", h.text);
    } else {
        println!("[lsp_live] hover returned null (acceptable)");
    }

    // ── textDocument/references ───────────────────────────────────────────────
    // Ask for references to `greet` from its definition site in greeter.ts line 1 col 17.
    let refs = client
        .references(&greeter_uri, 1, 17, /*include_declaration=*/ true)
        .expect("references request must succeed");

    assert!(
        !refs.is_empty(),
        "references for `greet` definition should return at least the declaration itself"
    );

    println!(
        "[lsp_live] references count: {} — first: {}:{}",
        refs.len(),
        refs[0].uri,
        refs[0].start_line
    );

    println!("[lsp_live] PASS — TypeScript LSP round-trip complete");
}

// ── LspTier round-trip (phase-0 of W3.6, ADR-009) ─────────────────────────────
//
// Drives `LspTier::definition` (NOT `LspClient` directly): the tier must handle
// `textDocument/didOpen` itself — tsserver/pyright return empty results for
// unopened documents, so a tier that skips didOpen cannot answer at all.
// This test is committed BEFORE the didOpen fix per the lane's BEFORE/AFTER
// measurement protocol: its red run (empty results) is the BEFORE evidence.

/// A fixture with a plain `.ts` caller and a `.tsx` caller (JSX syntax), both importing
/// `greet` from `./greeter`. The `.tsx` file proves the languageId mapping
/// (`tsx` → `typescriptreact`) against the real server.
fn write_tsx_fixture(dir: &std::path::Path) {
    fs::create_dir_all(dir.join("src")).unwrap();

    fs::write(
        dir.join("tsconfig.json"),
        r#"{ "compilerOptions": { "strict": true, "target": "ES2020", "jsx": "preserve" }, "include": ["src/**/*"] }"#,
    )
    .unwrap();

    // greeter.ts — line 1 (0-based): `export function greet(...)`.
    fs::write(
        dir.join("src/greeter.ts"),
        "// Greeter module\nexport function greet(name: string): string {\n  return `Hello, ${name}!`;\n}\n",
    )
    .unwrap();

    // main.ts — line 2 (0-based): `const msg: string = greet('world');`, `greet` at col 20.
    fs::write(
        dir.join("src/main.ts"),
        "import { greet } from './greeter';\n\nconst msg: string = greet('world');\nconsole.log(msg);\n",
    )
    .unwrap();

    // app.tsx — line 3 (0-based): `  return <span>{greet('tsx world')}</span>;`, `greet` at col 16.
    fs::write(
        dir.join("src/app.tsx"),
        "import { greet } from './greeter';\n\nexport function App() {\n  return <span>{greet('tsx world')}</span>;\n}\n",
    )
    .unwrap();
}

/// Bounded retry (tsserver builds its project model asynchronously after didOpen):
/// up to 20 attempts, 250ms apart — never a single fixed sleep. Succeeds once a location
/// lands in the file matching `expect_uri_substr` (mid-load, tsserver answers with the
/// local import-specifier binding before it can follow the import cross-file); otherwise
/// returns the last outcome for the failure message.
fn retry_definition(
    tier: &mut LspTier,
    language: &str,
    uri: &str,
    line: u32,
    col: u32,
    expect_uri_substr: &str,
) -> Result<Vec<Location>, String> {
    let mut last: Result<Vec<Location>, String> = Err("never attempted".to_string());
    for _ in 0..20 {
        match tier.definition(language, uri, line, col) {
            Ok(locs) if locs.iter().any(|l| l.uri.contains(expect_uri_substr)) => {
                return Ok(locs);
            }
            Ok(locs) => last = Ok(locs),
            Err(e) => last = Err(e.to_string()),
        }
        thread::sleep(Duration::from_millis(250));
    }
    last
}

#[test]
fn lsp_tier_definition_roundtrip_including_tsx() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write_tsx_fixture(root);
    let root_str = root.to_str().expect("non-UTF8 tempdir path");
    if typescript_launch_or_skip(root_str).is_none() {
        return;
    }

    let mut tier = LspTier::new(root_str);
    let main_uri = path_to_file_uri(root.join("src/main.ts").to_str().unwrap());
    let app_uri = path_to_file_uri(root.join("src/app.tsx").to_str().unwrap());

    // ── .ts path ──────────────────────────────────────────────────────────────
    let defs = retry_definition(&mut tier, "typescript", &main_uri, 2, 20, "greeter");
    match &defs {
        Ok(locs) => assert!(
            locs.iter().any(|l| l.uri.contains("greeter")),
            "definition for `greet` in main.ts should point at greeter.ts, got: {locs:?}"
        ),
        Err(e) => panic!(
            "LspTier::definition on main.ts returned no non-empty result within the retry \
             budget (last error: {e}) — the tier is not sending textDocument/didOpen"
        ),
    }
    println!("[lsp_live] tier definition (.ts): {:?}", defs.unwrap()[0]);

    // ── .tsx path (languageId mapping tsx→typescriptreact) ───────────────────
    let defs_tsx = retry_definition(&mut tier, "tsx", &app_uri, 3, 16, "greeter");
    match &defs_tsx {
        Ok(locs) => assert!(
            locs.iter().any(|l| l.uri.contains("greeter")),
            "definition for `greet` in app.tsx should point at greeter.ts, got: {locs:?}"
        ),
        Err(e) => panic!(
            "LspTier::definition on app.tsx returned no non-empty result within the retry \
             budget (last error: {e}) — didOpen or the tsx→typescriptreact languageId \
             mapping is broken"
        ),
    }
    println!(
        "[lsp_live] tier definition (.tsx): {:?}",
        defs_tsx.unwrap()[0]
    );

    // ── cache path: a second query on the same (already-open) file succeeds ──
    let again = tier
        .definition("typescript", &main_uri, 2, 20)
        .expect("second definition query on an already-open file must succeed");
    assert!(
        !again.is_empty(),
        "second query on the same file returned empty — the opened-docs cache broke the session"
    );

    println!("[lsp_live] PASS — LspTier round-trip (.ts + .tsx + cache) complete");
}
