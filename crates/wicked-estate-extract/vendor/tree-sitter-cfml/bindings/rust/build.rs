//! Compiles the vendored CFML grammars (cfml tag + cfscript). Each grammar has a generated
//! `parser.c` plus an external `scanner.c`. The cfml scanner includes `../../common/scanner.h`,
//! resolved by the C preprocessor relative to the scanner source — so the `common/` dir must
//! sit two levels above `cfml/src/` (preserved in the vendor layout). The cfquery SQL dialect
//! grammar from upstream is intentionally NOT vendored (SQL is covered by the SQL grammar).

fn main() {
    let root = std::path::Path::new(".");
    let flags = [
        "-std=c11", "-w", // silence benign generated-parser warnings
    ];

    for (name, dir) in &[
        ("tree-sitter-cfml", "cfml"),
        ("tree-sitter-cfscript", "cfscript"),
    ] {
        let src_dir = root.join(dir).join("src");
        let mut config = cc::Build::new();
        config.include(&src_dir);
        for flag in &flags {
            config.flag_if_supported(flag);
        }
        for file in &["parser.c", "scanner.c"] {
            let path = src_dir.join(file);
            config.file(&path);
            println!("cargo:rerun-if-changed={}", path.to_str().unwrap());
        }
        config.compile(name);
    }
    println!("cargo:rerun-if-changed=common/scanner.h");
}
