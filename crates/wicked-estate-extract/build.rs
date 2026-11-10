//! Compiles the vendored in-house APEXlang grammar (`vendor/tree-sitter-apexlang`).
//!
//! The other vendored grammars are their own `wicked-estate-tree-sitter-*` crates. This one is
//! compiled here instead: a new crate name cannot be published through crates.io Trusted
//! Publishing (see `.github/workflows/publish.yml`), so a separate crate would hold every release
//! on a manual token bootstrap. The source layout is the usual tree-sitter one, so it can be lifted
//! into a crate later without regenerating anything.

fn main() {
    let src = std::path::Path::new("vendor/tree-sitter-apexlang/src");
    let mut build = cc::Build::new();
    build.include(src);
    build.flag_if_supported("-w");
    build.file(src.join("parser.c"));
    build.file(src.join("scanner.c"));
    build.compile("tree-sitter-apexlang");
    println!("cargo:rerun-if-changed=vendor/tree-sitter-apexlang/src/parser.c");
    println!("cargo:rerun-if-changed=vendor/tree-sitter-apexlang/src/scanner.c");
}
