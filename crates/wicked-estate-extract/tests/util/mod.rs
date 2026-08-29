//! Shared helper for the cc-gated plugin-override integration tests: build the example nginx
//! grammar into a dylib inside a plugin dir, with a caller-provided manifest.
//!
//! Each test FILE owns exactly ONE registry configuration (the registry is a process-wide
//! `OnceLock`) — this module only assembles plugin dirs; the caller sets `WICKED_ESTATE_PLUGINS`
//! once before the first registry access.

use std::path::Path;
use std::process::Command;

/// Compile the example nginx grammar into `plugin_dir` as `libnginx.<ext>`, copy `nginx.scm`
/// next to it, and write `manifest` as `plugin.toml`. Returns `false` — writing NOTHING — when
/// no `cc` is available (callers skip their dylib leg with an eprintln, the
/// `plugin_loader.rs` pattern).
pub fn build_nginx_plugin(plugin_dir: &Path, manifest: &str) -> bool {
    if Command::new("cc").arg("--version").output().is_err() {
        return false;
    }
    let example = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/plugins/nginx")
        .canonicalize()
        .expect("example plugin dir exists");
    let parser_c = example.join("src/parser.c");
    assert!(parser_c.is_file(), "generated parser missing: {parser_c:?}");

    std::fs::create_dir_all(plugin_dir).expect("mkdir plugin dir");
    let lib_out = plugin_dir.join(format!("libnginx{}", std::env::consts::DLL_SUFFIX));
    let status = Command::new("cc")
        .args(["-shared", "-fPIC", "-O2", "-w", "-I"])
        .arg(example.join("src"))
        .arg("-o")
        .arg(&lib_out)
        .arg(&parser_c)
        .status()
        .expect("run cc");
    assert!(status.success(), "cc failed to build {lib_out:?}");

    std::fs::copy(example.join("nginx.scm"), plugin_dir.join("nginx.scm")).unwrap();
    std::fs::write(plugin_dir.join("plugin.toml"), manifest).unwrap();
    true
}
