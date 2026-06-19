//! Runtime language-plugin loader end-to-end test.
//!
//! Builds the example nginx plugin's shared library at test time (via `cc`), drops a complete plugin
//! directory (libnginx.<ext> + plugin.toml + nginx.scm) into a temp plugins folder, points
//! `WICKED_ESTATE_PLUGINS` at it, and verifies wicked-estate loads the grammar at runtime and
//! extracts from it — exactly the drop-in flow a user gets, with no grammar compiled into the core.
//!
//! Single test function on purpose: the plugin registry is a process-wide `OnceLock`, so the env var
//! must be set before the first plugin access.

use std::path::{Path, PathBuf};
use std::process::Command;

use wicked_estate_core::{Extractor, Language, NodeKind, SourceFile};
use wicked_estate_extract::TreeSitterExtractor;

#[test]
fn nginx_plugin_loads_and_extracts() {
    // Locate the example plugin source (repo_root/examples/plugins/nginx).
    let example = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/plugins/nginx")
        .canonicalize()
        .expect("example plugin dir exists");
    let parser_c = example.join("src/parser.c");
    assert!(parser_c.is_file(), "generated parser missing: {parser_c:?}");

    // cc must be available (it is in CI — the bundled grammars compile with it). Skip cleanly if not.
    if Command::new("cc").arg("--version").output().is_err() {
        eprintln!("SKIP: no `cc` available to build the plugin library");
        return;
    }

    // Assemble a temp plugins dir: <tmp>/plugins/nginx/{libnginx.<ext>, plugin.toml, nginx.scm}.
    let tmp = std::env::temp_dir().join(format!("we-plugin-test-{}", std::process::id()));
    let plugin_dir = tmp.join("plugins").join("nginx");
    std::fs::create_dir_all(&plugin_dir).expect("mkdir temp plugin dir");

    let lib_name = format!("libnginx{}", std::env::consts::DLL_SUFFIX);
    let lib_out = plugin_dir.join(&lib_name);
    let status = Command::new("cc")
        .args(["-shared", "-fPIC", "-O2", "-w", "-I"])
        .arg(example.join("src"))
        .arg("-o")
        .arg(&lib_out)
        .arg(&parser_c)
        .status()
        .expect("run cc");
    assert!(status.success(), "cc failed to build {lib_out:?}");

    std::fs::copy(example.join("plugin.toml"), plugin_dir.join("plugin.toml")).unwrap();
    std::fs::copy(example.join("nginx.scm"), plugin_dir.join("nginx.scm")).unwrap();

    // Point the loader at our temp plugins dir BEFORE any plugin access (set_var is unsafe in 2024).
    let plugins_root: PathBuf = tmp.join("plugins");
    unsafe {
        std::env::set_var("WICKED_ESTATE_PLUGINS", &plugins_root);
    }

    // nginx is NOT a built-in language — this only resolves via the runtime plugin.
    let extractor = TreeSitterExtractor::for_language("nginx")
        .expect("nginx plugin should load and its query compile");

    let conf = r#"
http {
    upstream backend {
        server 10.0.0.1:8080;
    }
    server {
        listen 80;
        location /api {
            proxy_pass http://backend;
        }
    }
}
"#;
    let ex = extractor
        .extract(&SourceFile {
            path: "nginx.conf".to_string(),
            language: Language::new("nginx"),
            text: conf.to_string(),
        })
        .expect("extract via plugin grammar");

    let modules: Vec<_> = ex
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Module)
        .map(|n| n.name.as_str())
        .collect();
    for block in ["http", "upstream", "server", "location"] {
        assert!(
            modules.contains(&block),
            "expected nginx block '{block}' as a module node; got {modules:?}"
        );
    }

    // Extension dispatch also resolves through the plugin (.nginxconf → nginx).
    assert!(
        wicked_estate_extract::treesitter::extractor_for_extension("nginxconf").is_some(),
        "extension dispatch should find the nginx plugin"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}
