//! Compiles the generated tree-sitter RPG parser (`src/parser.c`). No external scanner.
//! Paths are relative to the crate root (cargo runs build scripts there).

fn main() {
    let src = std::path::Path::new("src");
    let mut build = cc::Build::new();
    build.include(src);
    // Generated parsers trip benign C warnings; silence them without -Werror surprises.
    build.flag_if_supported("-w");
    build.file(src.join("parser.c"));
    build.compile("tree-sitter-rpg");
    println!("cargo:rerun-if-changed=src/parser.c");
}
