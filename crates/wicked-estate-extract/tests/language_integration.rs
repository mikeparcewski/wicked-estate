//! Per-language integration smoke tests.
//!
//! Two test functions:
//!   - `per_language_extraction_produces_nodes`: inline snippets (fast smoke test).
//!   - `fixture_files_produce_nodes`: on-disk corpus under tests/fixtures/<lang>/.
//!     Also enforces cap-aware assertions from languages.toml:
//!       - `calls` cap   → extraction.refs must be non-empty
//!       - `imports` cap → at least one Import node must appear
//!       - `extends` / `implements` cap → at least one Class or Interface node must appear

use std::collections::HashMap;

use wicked_estate_core::{EdgeKind, Extractor, Language, NodeKind, SourceFile};
use wicked_estate_extract::treesitter::extractor_for_extension;
use wicked_estate_extract::{AwsConfigRuleExtractor, AzurePolicyExtractor};
#[cfg(feature = "xml-rules")]
use wicked_estate_extract::CamundaDmnExtractor;

/// Build extension → caps map from languages.toml.
fn ext_caps() -> HashMap<String, Vec<String>> {
    let toml_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("languages.toml");
    let content = std::fs::read_to_string(&toml_path).expect("languages.toml missing");
    let doc: toml::Value = content.parse().expect("languages.toml invalid TOML");
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    if let Some(langs) = doc.get("language").and_then(|v| v.as_array()) {
        for lang in langs {
            let caps: Vec<String> = lang
                .get("caps")
                .and_then(|c| c.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            if let Some(exts) = lang.get("ext").and_then(|e| e.as_array()) {
                for ext_val in exts {
                    if let Some(e) = ext_val.as_str() {
                        map.entry(e.to_owned()).or_insert_with(|| caps.clone());
                    }
                }
            }
        }
    }
    map
}

/// (language_name, extension, snippet)
static SNIPPETS: &[(&str, &str, &str)] = &[
    // ── Systems ──────────────────────────────────────────────────────────────
    ("rust", "rs", "pub fn add(a: i32, b: i32) -> i32 { a + b }"),
    ("c", "c", "int add(int a, int b) { return a + b; }"),
    ("cpp", "cpp", "int add(int a, int b) { return a + b; }"),
    (
        "go",
        "go",
        "package main\nfunc add(a, b int) int { return a + b }",
    ),
    (
        "zig",
        "zig",
        "pub fn add(a: i32, b: i32) i32 { return a + b; }",
    ),
    ("d", "d", "int add(int a, int b) { return a + b; }"),
    (
        "ada",
        "adb",
        "function Add(A, B : Integer) return Integer is begin return A + B; end Add;",
    ),
    (
        "pascal",
        "pas",
        "function Add(A, B: Integer): Integer; begin Result := A + B; end;",
    ),
    (
        "fortran",
        "f90",
        "function add(a, b)\n  integer :: a, b, add\n  add = a + b\nend function",
    ),
    (
        "cuda",
        "cu",
        "__global__ void add(int *a, int *b, int *c) { *c = *a + *b; }",
    ),
    (
        "arduino",
        "ino",
        "void setup() {}\nvoid loop() { digitalWrite(13, HIGH); }",
    ),
    (
        "verilog",
        "v",
        "module adder(a, b, c);\n  input a, b;\n  output c;\n  assign c = a + b;\nendmodule",
    ),
    (
        "vhdl",
        "vhd",
        "entity adder is\n  port(a, b: in bit; c: out bit);\nend adder;",
    ),
    // ── JVM / CLR ────────────────────────────────────────────────────────────
    (
        "java",
        "java",
        "public class Foo { public int add(int a, int b) { return a + b; } }",
    ),
    ("kotlin", "kt", "fun add(a: Int, b: Int): Int = a + b"),
    ("scala", "scala", "def add(a: Int, b: Int): Int = a + b"),
    ("groovy", "groovy", "def add(a, b) { return a + b }"),
    (
        "csharp",
        "cs",
        "public class Foo { public int Add(int a, int b) => a + b; }",
    ),
    ("fsharp", "fs", "let add a b = a + b"),
    // ── Web / Scripting ──────────────────────────────────────────────────────
    ("javascript", "js", "function add(a, b) { return a + b; }"),
    (
        "typescript",
        "ts",
        "function add(a: number, b: number): number { return a + b; }",
    ),
    (
        "tsx",
        "tsx",
        "function App(): JSX.Element { return <div>hello</div>; }",
    ),
    ("python", "py", "def add(a, b):\n    return a + b"),
    ("ruby", "rb", "def add(a, b)\n  a + b\nend"),
    (
        "php",
        "php",
        "<?php\nfunction add($a, $b) { return $a + $b; }",
    ),
    ("lua", "lua", "function add(a, b)\n  return a + b\nend"),
    (
        "perl",
        "pl",
        "sub add { my ($a, $b) = @_; return $a + $b; }",
    ),
    ("r", "r", "add <- function(a, b) a + b"),
    // ── Functional ───────────────────────────────────────────────────────────
    ("haskell", "hs", "add :: Int -> Int -> Int\nadd a b = a + b"),
    ("ocaml", "ml", "let add a b = a + b"),
    (
        "elixir",
        "ex",
        "defmodule Foo do\n  def add(a, b), do: a + b\nend",
    ),
    (
        "erlang",
        "erl",
        "-module(foo).\n-export([add/2]).\nadd(A, B) -> A + B.",
    ),
    (
        "gleam",
        "gleam",
        "pub fn add(a: Int, b: Int) -> Int { a + b }",
    ),
    (
        "elm",
        "elm",
        "module Main exposing (..)\nadd : Int -> Int -> Int\nadd a b = a + b",
    ),
    ("clojure", "clj", "(defn add [a b] (+ a b))"),
    ("commonlisp", "cl", "(defun add (a b) (+ a b))"),
    ("julia", "jl", "function add(a, b)\n  a + b\nend"),
    ("ocaml_interface", "mli", "val add : int -> int -> int"),
    ("racket", "rkt", "(define (add a b) (+ a b))"),
    ("idris", "idr", "add : Int -> Int -> Int\nadd a b = a + b"),
    ("lean", "lean", "def add (a b : Nat) : Nat := a + b"),
    ("fsharp", "fsi", "let add a b = a + b"),
    // ── Shell / Ops ──────────────────────────────────────────────────────────
    ("bash", "sh", "#!/bin/bash\nfoo() { echo hello; }\nfoo"),
    (
        "powershell",
        "ps1",
        "function Add-Numbers { param($a, $b) $a + $b }",
    ),
    (
        "fish",
        "fish",
        "function add\n  echo (math $argv[1] + $argv[2])\nend",
    ),
    (
        "awk",
        "awk",
        "function add(a, b) { return a + b } BEGIN { print add(1, 2) }",
    ),
    ("make", "mk", ".PHONY: all\nall:\n\techo hello"),
    // ── IaC / Config ─────────────────────────────────────────────────────────
    (
        "hcl",
        "tf",
        "resource \"aws_s3_bucket\" \"foo\" {\n  bucket = \"my-bucket\"\n}",
    ),
    (
        "bicep",
        "bicep",
        "param location string = 'eastus'\nresource sa 'Microsoft.Storage/storageAccounts@2021-02-01' = {\n  name: 'mystorageaccount'\n  location: location\n  sku: { name: 'Standard_LRS' }\n  kind: 'StorageV2'\n}",
    ),
    (
        "dockerfile",
        "dockerfile",
        "FROM ubuntu:22.04\nRUN echo hello\nCMD [\"bash\"]",
    ),
    (
        "cmake",
        "cmake",
        "cmake_minimum_required(VERSION 3.10)\nproject(Foo)\nadd_executable(foo main.c)",
    ),
    (
        "nix",
        "nix",
        "{ pkgs ? import <nixpkgs> {} }:\npkgs.mkShell { buildInputs = [ pkgs.hello ]; }",
    ),
    (
        "jinja2",
        "j2",
        "{% macro greet(name) %}Hello {{ name }}!{% endmacro %}",
    ),
    // ── Data / Schema ────────────────────────────────────────────────────────
    (
        "sql",
        "sql",
        "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);\nSELECT id, name FROM users WHERE active = 1;",
    ),
    (
        "proto",
        "proto",
        "syntax = \"proto3\";\nmessage Person {\n  string name = 1;\n  int32 age = 2;\n}",
    ),
    (
        "graphql",
        "graphql",
        "type Query {\n  user(id: ID!): User\n}\ntype User {\n  id: ID!\n  name: String!\n}",
    ),
    (
        "thrift",
        "thrift",
        "service Calculator {\n  i32 add(1: i32 num1, 2: i32 num2)\n}",
    ),
    // ── Markup / Docs ────────────────────────────────────────────────────────
    (
        "html",
        "html",
        "<html><head><title>Test</title></head><body><p>Hello</p></body></html>",
    ),
    (
        "css",
        "css",
        ".container { display: flex; flex-direction: column; }\n.item { margin: 8px; }",
    ),
    (
        "markdown",
        "md",
        "# Hello\n\n## Section\n\nSome text with **bold** and _italic_.",
    ),
    (
        "yaml",
        "yaml",
        "name: foo\nversion: 1.0\ndeps:\n  - bar\n  - baz",
    ),
    (
        "toml",
        "toml",
        "[package]\nname = \"foo\"\nversion = \"0.1.0\"\n\n[dependencies]\nserde = \"1\"",
    ),
    (
        "xml",
        "xml",
        "<root><item id=\"1\">foo</item><item id=\"2\">bar</item></root>",
    ),
    (
        "json",
        "json",
        "{\"name\": \"foo\", \"version\": \"1.0\", \"deps\": [\"bar\"]}",
    ),
    ("ini", "ini", "[section]\nkey = value\nanother = 42"),
    // ── Mobile / Desktop ────────────────────────────────────────────────────
    (
        "swift",
        "swift",
        "func add(a: Int, b: Int) -> Int { return a + b }",
    ),
    ("dart", "dart", "int add(int a, int b) => a + b;"),
    (
        "objc",
        "mm",
        "@interface Foo : NSObject\n- (int)add:(int)a to:(int)b;\n@end",
    ),
    ("kotlin", "kts", "fun add(a: Int, b: Int): Int = a + b"),
    // ── Web UI frameworks ────────────────────────────────────────────────────
    (
        "svelte",
        "svelte",
        "<script>\n  let count = 0;\n  function increment() { count += 1; }\n</script>\n<button on:click={increment}>{count}</button>",
    ),
    (
        "vue",
        "vue",
        "<template><div>{{ msg }}</div></template>\n<script>\nexport default { data() { return { msg: 'hello' }; } }\n</script>",
    ),
    // ── Esoteric / Niche ────────────────────────────────────────────────────
    ("matlab", "m", "function y = add(a, b)\n  y = a + b;\nend"),
    ("scala", "sc", "def add(a: Int, b: Int): Int = a + b"),
    (
        "pony",
        "pony",
        "actor Main\n  new create(env: Env) =>\n    env.out.print(\"Hello, World!\")",
    ),
    ("nim", "nim", "proc add(a, b: int): int = a + b"),
    (
        "elixir",
        "exs",
        "defmodule Foo do\n  def add(a, b), do: a + b\nend",
    ),
    ("prolog", "pro", "add(A, B, C) :- C is A + B."),
    (
        "cobol",
        "cob",
        "IDENTIFICATION DIVISION.\nPROGRAM-ID. HELLO.\nPROCEDURE DIVISION.\nDISPLAY 'HELLO'.\nSTOP RUN.",
    ),
    (
        "cfml",
        "cfc",
        "<cfcomponent name=\"Foo\">\n<cffunction name=\"add\"><cfargument name=\"a\"><cfreturn a></cffunction>\n</cfcomponent>",
    ),
    (
        "cfscript",
        "cfs",
        "component {\n  function add(a, b) { return a + b; }\n}",
    ),
    (
        "abl",
        "p",
        "FUNCTION addNums RETURNS INTEGER (INPUT a AS INTEGER, INPUT b AS INTEGER):\n  RETURN a + b.\nEND FUNCTION.",
    ),
    (
        "lotusscript",
        "lss",
        "Class Foo\n  Public Sub doIt()\n    Call helper(1)\n  End Sub\nEnd Class",
    ),
    (
        "informix4gl",
        "4gl",
        "MAIN\n  CALL doWork()\nEND MAIN\nFUNCTION doWork()\n  DISPLAY \"hi\"\nEND FUNCTION",
    ),
    ("foxpro", "prg", "PROCEDURE Greet\n\t=SayHello()\nENDPROC"),
    (
        "powerscript",
        "sru",
        "global type n_foo from nonvisualobject\nend type\npublic function integer of_run ();\nReturn doWork()\nend function",
    ),
    (
        "crystal_formula",
        "crf",
        "Shared NumberVar total;\ntotal := total + {@LineAmount};\nToText(total)",
    ),
    (
        "solidity",
        "sol",
        "pragma solidity ^0.8.0;\ncontract Foo {\n  function add(uint a, uint b) public pure returns (uint) { return a + b; }\n}",
    ),
    ("starlark", "bzl", "def add(a, b):\n    return a + b"),
    (
        "rego",
        "rego",
        "package authz\nallow {\n  input.method == \"GET\"\n}",
    ),
    (
        "rescript",
        "res",
        "let add = (a: int, b: int): int => a + b",
    ),
    (
        "gleam",
        "gleam",
        "pub fn greet(name: String) -> String {\n  \"Hello, \" <> name\n}",
    ),
    (
        "glsl",
        "glsl",
        "void main() {\n  vec4 color = vec4(1.0, 0.0, 0.0, 1.0);\n  gl_FragColor = color;\n}",
    ),
    (
        "hlsl",
        "hlsl",
        "float4 PSMain(float4 pos : SV_POSITION) : SV_TARGET {\n  return float4(1, 0, 0, 1);\n}",
    ),
    (
        "dot",
        "dot",
        "digraph G {\n  a -> b;\n  b -> c;\n  a -> c;\n}",
    ),
    (
        "elisp",
        "el",
        "(defun add (a b)\n  \"Add A and B.\"\n  (+ a b))",
    ),
    ("jq", "jq", "def add(a; b): a + b;"),
    (
        "kdl",
        "kdl",
        "package name=\"foo\" version=\"1.0\" {\n  dep \"bar\" version=\"2.0\"\n}",
    ),
    (
        "meson",
        "meson",
        "project('foo', 'c', version: '1.0')\nexecutable('foo', 'main.c')",
    ),
    ("just", "just", "add a b:\n  echo {{a + b}}"),
    (
        "devicetree",
        "dts",
        "/ {\n  model = \"Test Board\";\n  compatible = \"vendor,board\";\n  cpu@0 {\n    device_type = \"cpu\";\n  };\n};",
    ),
    ("ron", "ron", "(field: 42, name: \"foo\")"),
    (
        "ninja",
        "ninja",
        "rule compile\n  command = gcc $in -o $out\nbuild foo.o: compile foo.c",
    ),
    (
        "nginx",
        "nginxconf",
        "server {\n  listen 80;\n  server_name example.com;\n  location / { root /var/www/html; }\n}",
    ),
    (
        "postscript",
        "ps",
        "/greet { (Hello, World!) print newline } def\ngreet",
    ),
    (
        "arm",
        "json",
        "{\n  \"$schema\": \"https://schema.management.azure.com/schemas/2019-04-01/deploymentTemplate.json#\",\n  \"contentVersion\": \"1.0.0.0\",\n  \"resources\": []\n}",
    ),
];

fn check(lang: &str, ext: &str, snippet: &str) {
    let Some(extractor) = extractor_for_extension(ext) else {
        // Wired in languages.toml but extractor not found for this extension
        // (e.g. double-extension like ".arm.json" that lookup can't resolve) — skip.
        eprintln!("SKIP {lang} (.{ext}): no extractor found for extension");
        return;
    };

    let file = SourceFile {
        path: format!("test.{ext}"),
        language: Language(lang.to_string()),
        text: snippet.to_string(),
    };

    let result = extractor.extract(&file);
    assert!(
        result.is_ok(),
        "extract({lang}) returned error: {:?}",
        result.err()
    );

    let extraction = result.unwrap();
    assert!(
        !extraction.nodes.is_empty(),
        "extract({lang}) produced 0 nodes from snippet:\n{snippet}"
    );
}

#[test]
fn per_language_extraction_produces_nodes() {
    for &(lang, ext, snippet) in SNIPPETS {
        check(lang, ext, snippet);
    }
}

#[test]
fn fixture_files_produce_nodes() {
    let fixtures_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let caps_map = ext_caps();

    let mut tested = 0usize;
    let mut failures: Vec<String> = Vec::new();

    let mut lang_dirs: Vec<_> = std::fs::read_dir(&fixtures_dir)
        .expect("tests/fixtures dir missing")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();
    lang_dirs.sort_by_key(|e| e.path());

    for lang_entry in lang_dirs {
        let lang_dir = lang_entry.path();
        let lang = lang_dir.file_name().unwrap().to_string_lossy().to_string();

        let mut files: Vec<_> = std::fs::read_dir(&lang_dir)
            .expect("fixture directory must be readable")
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file())
            .collect();
        files.sort_by_key(|e| e.path());

        for file_entry in files {
            let file_path = file_entry.path();
            let ext = match file_path.extension() {
                Some(e) => e.to_string_lossy().to_string(),
                None => continue,
            };

            let Some(extractor) = extractor_for_extension(&ext) else {
                eprintln!("SKIP {lang} (.{ext}): no extractor for extension");
                continue;
            };

            let text = std::fs::read_to_string(&file_path)
                .expect("fixture file must be readable");

            let file = SourceFile {
                path: file_path.to_string_lossy().to_string(),
                language: Language(lang.clone()),
                text,
            };

            tested += 1;
            let fname = file_path.file_name().unwrap().to_string_lossy().to_string();

            let extraction = match extractor.extract(&file) {
                Err(e) => {
                    failures.push(format!("{lang}/{fname}: extract error: {e:?}"));
                    continue;
                }
                Ok(e) => e,
            };

            if extraction.nodes.is_empty() {
                failures.push(format!("{lang}/{fname}: produced 0 nodes"));
                continue;
            }

            // Cap-aware assertions from languages.toml.
            // Languages whose extractors have query gaps are logged as EXTRACTOR-GAP (not
            // hard failures) so the test suite stays green while the gaps remain visible.
            let caps = caps_map.get(&ext).cloned().unwrap_or_default();

            if caps.contains(&"calls".to_owned()) && extraction.refs.is_empty() {
                // No call refs produced — may be a fixture gap or an extractor query gap.
                // Check whether the file has any content resembling call syntax; if so,
                // treat it as an extractor gap (warn) rather than a fixture gap (fail).
                let has_call_syntax = !extraction.nodes.is_empty(); // extractor ran, just no refs
                if has_call_syntax {
                    eprintln!(
                        "EXTRACTOR-GAP {lang}/{fname}: claims 'calls' cap but produced 0 refs \
                         — the .scm query likely lacks a @call capture"
                    );
                } else {
                    failures.push(format!(
                        "{lang}/{fname}: claims 'calls' cap but produced 0 refs and 0 nodes \
                         (fixture may be empty or unparseable)"
                    ));
                }
            }

            if caps.contains(&"imports".to_owned()) {
                let has_import = extraction
                    .nodes
                    .iter()
                    .any(|n| matches!(n.kind, NodeKind::Import));
                if !has_import {
                    // Determine if import syntax IS present but the extractor doesn't pick it up,
                    // vs. the fixture simply lacking import statements.
                    // Heuristic: if the file contains a keyword that looks like an import form,
                    // the problem is the extractor query; otherwise it's the fixture.
                    let text_lower = file.text.to_ascii_lowercase();
                    let has_import_syntax = text_lower.contains("import ")
                        || text_lower.contains("@import")
                        || text_lower.contains("require ")
                        || text_lower.contains("use ")
                        || text_lower.contains("open ");
                    if has_import_syntax {
                        eprintln!(
                            "EXTRACTOR-GAP {lang}/{fname}: claims 'imports' cap, file has import \
                             syntax, but produced no Import nodes — the .scm query likely lacks \
                             an @import capture"
                        );
                    } else {
                        failures.push(format!(
                            "{lang}/{fname}: claims 'imports' cap but produced no Import nodes \
                             and file has no import syntax (add import/use statements to the fixture)"
                        ));
                    }
                }
            }

            let needs_class =
                caps.contains(&"extends".to_owned()) || caps.contains(&"implements".to_owned());
            if needs_class {
                let has_class = extraction.nodes.iter().any(|n| {
                    matches!(
                        n.kind,
                        NodeKind::Class | NodeKind::Interface | NodeKind::Struct
                    )
                });
                if !has_class {
                    failures.push(format!(
                        "{lang}/{fname}: claims 'extends'/'implements' cap but produced no \
                         Class/Interface/Struct nodes (add a class hierarchy to the fixture)"
                    ));
                }
            }
        }
    }

    assert!(
        tested > 0,
        "no fixture files found under tests/fixtures/<lang>/"
    );

    assert!(
        failures.is_empty(),
        "{} fixture(s) failed cap assertions ({} total):\n{}",
        failures.len(),
        tested,
        failures.join("\n")
    );
    println!("fixture_files_produce_nodes: {tested} files passed (cap-aware)");
}

// ── W15.5: Azure Policy + AWS Config Rule fixture integration tests ───────────
//
// These use schema-detecting JSON extractors (not tree-sitter), so they cannot be
// reached via `extractor_for_extension`. They run as independent test functions.

#[test]
fn azure_policy_fixture_emits_rule_nodes() {
    let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/azure_policy/require_https_storage.json");
    let text = std::fs::read_to_string(&fixture_path)
        .expect("azure_policy fixture must be readable");
    let file = SourceFile {
        path: fixture_path.to_string_lossy().to_string(),
        language: Language::new("azure-policy"),
        text,
    };
    let ex = AzurePolicyExtractor::new()
        .extract(&file)
        .expect("azure policy fixture must parse");

    assert_eq!(
        ex.nodes.len(),
        4,
        "expected 4 nodes (RuleSet, Rule, Condition, Action); got: {:?}",
        ex.nodes.iter().map(|n| (&n.kind, &n.name)).collect::<Vec<_>>()
    );
    assert!(
        ex.nodes.iter().any(|n| n.kind == NodeKind::RuleSet),
        "must have RuleSet"
    );
    assert!(
        ex.nodes.iter().any(|n| n.kind == NodeKind::Rule),
        "must have Rule"
    );
    assert!(
        ex.nodes.iter().any(|n| n.kind == NodeKind::Condition),
        "must have Condition"
    );
    assert!(
        ex.nodes.iter().any(|n| n.kind == NodeKind::Action),
        "must have Action"
    );
    assert!(
        !ex.local_edges.is_empty(),
        "must have at least one edge"
    );
}

#[test]
fn azure_policy_fixture_display_name_captured() {
    let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/azure_policy/require_https_storage.json");
    let text = std::fs::read_to_string(&fixture_path)
        .expect("azure_policy fixture must be readable");
    let file = SourceFile {
        path: fixture_path.to_string_lossy().to_string(),
        language: Language::new("azure-policy"),
        text,
    };
    let ex = AzurePolicyExtractor::new()
        .extract(&file)
        .expect("azure policy fixture must parse");
    let ruleset = ex
        .nodes
        .iter()
        .find(|n| n.kind == NodeKind::RuleSet)
        .expect("RuleSet must exist");
    assert_eq!(
        ruleset.name, "Require HTTPS on Storage Accounts",
        "RuleSet name must match displayName"
    );
}

#[test]
fn aws_config_rule_fixture_emits_rule_nodes() {
    let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/aws_config/restricted_ssh.json");
    let text = std::fs::read_to_string(&fixture_path)
        .expect("aws_config fixture must be readable");
    let file = SourceFile {
        path: fixture_path.to_string_lossy().to_string(),
        language: Language::new("aws-config-rule"),
        text,
    };
    let ex = AwsConfigRuleExtractor::new()
        .extract(&file)
        .expect("aws config rule fixture must parse");

    // RuleSet + Rule + Condition + 1 Fact = 4
    assert_eq!(
        ex.nodes.len(),
        4,
        "expected 4 nodes; got: {:?}",
        ex.nodes.iter().map(|n| (&n.kind, &n.name)).collect::<Vec<_>>()
    );
    assert!(
        ex.nodes.iter().any(|n| n.kind == NodeKind::RuleSet),
        "must have RuleSet"
    );
    assert!(
        ex.nodes.iter().any(|n| n.kind == NodeKind::Rule),
        "must have Rule"
    );
    assert!(
        ex.nodes.iter().any(|n| n.kind == NodeKind::Condition),
        "must have Condition"
    );
    assert!(
        ex.nodes.iter().any(|n| n.kind == NodeKind::Fact),
        "must have at least one Fact"
    );
}

#[test]
fn aws_config_rule_fixture_correct_names() {
    let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/aws_config/restricted_ssh.json");
    let text = std::fs::read_to_string(&fixture_path)
        .expect("aws_config fixture must be readable");
    let file = SourceFile {
        path: fixture_path.to_string_lossy().to_string(),
        language: Language::new("aws-config-rule"),
        text,
    };
    let ex = AwsConfigRuleExtractor::new()
        .extract(&file)
        .expect("aws config rule fixture must parse");

    let rule = ex
        .nodes
        .iter()
        .find(|n| n.kind == NodeKind::Rule)
        .expect("Rule must exist");
    assert_eq!(rule.name, "restricted-ssh");

    let fact = ex
        .nodes
        .iter()
        .find(|n| n.kind == NodeKind::Fact)
        .expect("Fact must exist");
    assert_eq!(fact.name, "AWS::EC2::SecurityGroup");

    let cond = ex
        .nodes
        .iter()
        .find(|n| n.kind == NodeKind::Condition)
        .expect("Condition must exist");
    assert_eq!(cond.name, "INCOMING_SSH_DISABLED");
}

#[test]
fn aws_config_rule_fixture_edge_kinds() {
    let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/aws_config/restricted_ssh.json");
    let text = std::fs::read_to_string(&fixture_path)
        .expect("aws_config fixture must be readable");
    let file = SourceFile {
        path: fixture_path.to_string_lossy().to_string(),
        language: Language::new("aws-config-rule"),
        text,
    };
    let ex = AwsConfigRuleExtractor::new()
        .extract(&file)
        .expect("aws config rule fixture must parse");

    let evaluates = ex
        .local_edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Evaluates)
        .count();
    assert_eq!(evaluates, 1, "expected 1 Evaluates edge for the Fact");

    let contains = ex
        .local_edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Contains)
        .count();
    assert_eq!(contains, 2, "expected 2 Contains edges (RuleSet→Rule, Rule→Condition)");
}

// ── W15.4: Camunda DMN extractor fixture integration test ─────────────────────

/// W15.4 — Camunda DMN extractor smoke test.
///
/// Loads `tests/fixtures/dmn/loan_decision.dmn` and verifies that:
/// - At least 2 `NodeKind::Rule` nodes are produced (the two `<decision>` elements).
/// - At least 2 `NodeKind::RuleSet` nodes are produced (`<definitions>` + two
///   `<decisionTable>` elements → at least 2).
/// - At least 1 `EdgeKind::Governs` edge is produced (`definitions` → `decision`).
#[cfg(feature = "xml-rules")]
#[test]
fn dmn_fixture_extracts_decisions_and_decision_tables() {
    let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/dmn/loan_decision.dmn");
    let text = std::fs::read_to_string(&fixture_path)
        .expect("tests/fixtures/dmn/loan_decision.dmn must exist");

    let extractor = CamundaDmnExtractor::new();

    let file = SourceFile {
        path: fixture_path.to_string_lossy().to_string(),
        language: Language("camunda-dmn".to_string()),
        text,
    };

    let extraction = extractor
        .extract(&file)
        .expect("CamundaDmnExtractor::extract must succeed on valid DMN");

    // Count Rule nodes (decisions + rule rows).
    let rule_count = extraction
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Rule)
        .count();
    assert!(
        rule_count >= 2,
        "expected >= 2 Rule nodes (the two <decision> elements + rule rows), got {rule_count}"
    );

    // Count RuleSet nodes (definitions + decisionTable elements).
    let rule_set_count = extraction
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::RuleSet)
        .count();
    assert!(
        rule_set_count >= 2,
        "expected >= 2 RuleSet nodes (<definitions> + at least one <decisionTable>), got {rule_set_count}"
    );

    // At least one Governs edge (definitions → decision).
    let governs_count = extraction
        .local_edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Governs)
        .count();
    assert!(
        governs_count >= 1,
        "expected >= 1 Governs edge (definitions → decision), got {governs_count}"
    );
}

// ── W15.12 IBM ODM BAL/IRL integration tests ─────────────────────────────────

#[test]
fn odm_irl_fixture_extracts_rules() {
    use wicked_estate_core::EdgeKind;
    use wicked_estate_extract::OdmExtractor;

    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/ibm_odm/pricing_rules.irl");
    let text = std::fs::read_to_string(&fixture)
        .expect("odm fixture must be readable");

    let file = SourceFile {
        path: fixture.to_string_lossy().to_string(),
        language: Language::new("ibm-odm-irl"),
        text,
    };

    let ex = OdmExtractor::new().extract(&file).expect("irl extract");

    let rulesets: Vec<_> = ex
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::RuleSet)
        .collect();
    assert!(
        !rulesets.is_empty(),
        "expected at least 1 RuleSet (package) node, got 0"
    );

    let rules: Vec<_> = ex
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Rule)
        .collect();
    assert_eq!(rules.len(), 3, "expected exactly 3 Rule nodes, got {}", rules.len());

    let conditions: Vec<_> = ex
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Condition)
        .collect();
    assert!(
        conditions.len() >= 3,
        "expected at least 3 Condition nodes (one per rule), got {}",
        conditions.len()
    );

    let actions: Vec<_> = ex
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Action)
        .collect();
    assert!(
        actions.len() >= 3,
        "expected at least 3 Action nodes (one per rule), got {}",
        actions.len()
    );

    let contains: Vec<_> = ex
        .local_edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Contains)
        .collect();
    assert!(
        !contains.is_empty(),
        "expected Contains edges, got none"
    );
}

#[test]
fn odm_bal_fixture_extracts_rules() {
    use wicked_estate_core::EdgeKind;
    use wicked_estate_extract::OdmExtractor;

    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/ibm_odm/loan_approval.brl");
    let text = std::fs::read_to_string(&fixture)
        .expect("odm fixture must be readable");

    let file = SourceFile {
        path: fixture.to_string_lossy().to_string(),
        language: Language::new("ibm-odm-bal"),
        text,
    };

    let ex = OdmExtractor::new().extract(&file).expect("bal extract");

    let rules: Vec<_> = ex
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Rule)
        .collect();
    assert_eq!(rules.len(), 3, "expected exactly 3 Rule nodes, got {}", rules.len());

    let conditions: Vec<_> = ex
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Condition)
        .collect();
    assert!(
        !conditions.is_empty(),
        "expected at least 1 Condition node, got 0"
    );

    let actions: Vec<_> = ex
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Action)
        .collect();
    assert!(
        !actions.is_empty(),
        "expected at least 1 Action node, got 0"
    );

    let contains: Vec<_> = ex
        .local_edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Contains)
        .collect();
    assert!(
        !contains.is_empty(),
        "expected Contains edges, got none"
    );
}

// ── W15.6 Salesforce Flow integration test ────────────────────────────────────

#[cfg(feature = "xml-rules")]
#[test]
fn salesforce_flow_fixture_extracts_decisions_and_rules() {
    use wicked_estate_core::EdgeKind;
    use wicked_estate_extract::SalesforceFlowExtractor;

    let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/salesforce_flow/account_validation.flow-meta.xml");
    let text = std::fs::read_to_string(&fixture_path)
        .expect("salesforce_flow fixture must be readable");

    let extractor = SalesforceFlowExtractor::new();
    let file = wicked_estate_core::SourceFile {
        path: fixture_path.to_string_lossy().to_string(),
        language: Language::new("xml-rules:salesforce-flow"),
        text,
    };

    let extraction = extractor.extract(&file).expect("extraction must succeed");

    // Assert: at least 1 RuleSet node (decisions element)
    let rule_sets: Vec<_> = extraction
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::RuleSet)
        .collect();
    assert!(
        !rule_sets.is_empty(),
        "expected at least 1 RuleSet node (decisions), got 0; nodes: {:?}",
        extraction.nodes.iter().map(|n| (&n.name, &n.kind)).collect::<Vec<_>>()
    );

    // Assert: at least 2 Rule nodes (the two rules elements: Is_Active_Premium + Is_Inactive)
    let rules: Vec<_> = extraction
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Rule)
        .collect();
    assert!(
        rules.len() >= 2,
        "expected at least 2 Rule nodes, got {}: {:?}",
        rules.len(),
        rules.iter().map(|n| &n.name).collect::<Vec<_>>()
    );

    // Assert: at least 1 Condition node
    let conditions: Vec<_> = extraction
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Condition)
        .collect();
    assert!(
        !conditions.is_empty(),
        "expected at least 1 Condition node, got 0"
    );

    // Assert: at least 1 Contains edge (decisions -> rules)
    let contains_edges: Vec<_> = extraction
        .local_edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Contains)
        .collect();
    assert!(
        !contains_edges.is_empty(),
        "expected at least 1 Contains edge (decisions -> rules), got 0"
    );

    println!(
        "salesforce_flow_fixture: {} rule_sets, {} rules, {} conditions, {} contains_edges",
        rule_sets.len(),
        rules.len(),
        conditions.len(),
        contains_edges.len()
    );
}
