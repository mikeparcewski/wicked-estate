fn main() {
    let src = std::path::Path::new("src");
    let mut build = cc::Build::new();
    build.include(src);
    build.flag_if_supported("-w");
    build.file(src.join("parser.c"));
    build.compile("tree-sitter-vba");
    println!("cargo:rerun-if-changed=src/parser.c");
}
