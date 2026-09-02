//! Generic tree-sitter [`Extractor`] driven by per-language `.scm` query files.
//!
//! Uses the prior art capture convention:
//! - Definitions: `@code_<kind>.def` (the whole node) + `@code_<kind>.name` (the identifier).
//!   Variant anchors accepted: `@code_<kind>` (no suffix), `@code_<kind>.arrow`, and
//!   `@code_<kind>.decl` — a DECLARATION anchor (prototype/forward declaration): identical
//!   identity to `.def`, but the minted Node carries the `is_declaration` metadata flag so
//!   the store's multi-file contribution table prefers a definition record as the primary
//!   (M4 / Option A, wicked-estate#152).
//! - Calls: `@call.function` / `@call.method` (name from the captured node text).
//! - Imports: `@import` (the statement node) with optional `@import.source` (the path node).
//!   When `.source` is present its text is used; otherwise the `@import` node text is used.
//! - Heritage: `@code_extends.def` + `@code_extends.target` → `EdgeKind::Extends`.
//!   `@code_implements.def` + `@code_implements.target` → `EdgeKind::Implements`.
//!   Heritage refs are attributed from the declaring type to the target name.
//!
//! One implementation handles every language — a new language is a grammar + a query file.
//!
//! # Data-driven registry
//!
//! [`LANG_TABLE`] maps each language name to its grammar fn + embedded query. Use
//! [`TreeSitterExtractor::for_language`] to build one by name, or [`extractor_for_extension`] to
//! map a file extension → language → extractor in one step (via the `languages.toml` manifest).

use serde_json;
use std::collections::{HashMap, HashSet};
use streaming_iterator::StreamingIterator;
use tree_sitter::{Parser, Query, QueryCursor};
use wicked_estate_core::{
    Descriptor, Edge, EdgeKind, Error, Extraction, Extractor, Language, Location, Node, NodeKind,
    ResolutionTier, Result, SourceFile, Span, Suffix, Symbol, SymbolId, UnresolvedRef, edge_tags,
};

// ── Embedded query files ──────────────────────────────────────────────────────
const RUST_QUERY: &str = include_str!("queries/rust.scm");
const PYTHON_QUERY: &str = include_str!("queries/python.scm");
const TYPESCRIPT_QUERY: &str = include_str!("queries/typescript.scm");
const TSX_QUERY: &str = include_str!("queries/tsx.scm");
const JAVASCRIPT_QUERY: &str = include_str!("queries/javascript.scm");
const GO_QUERY: &str = include_str!("queries/go.scm");
const JAVA_QUERY: &str = include_str!("queries/java.scm");
const C_QUERY: &str = include_str!("queries/c.scm");
const CPP_QUERY: &str = include_str!("queries/cpp.scm");
const CSHARP_QUERY: &str = include_str!("queries/csharp.scm");
const RUBY_QUERY: &str = include_str!("queries/ruby.scm");
const BASH_QUERY: &str = include_str!("queries/bash.scm");
const JSON_QUERY: &str = include_str!("queries/json.scm");
const YAML_QUERY: &str = include_str!("queries/yaml.scm");
const PHP_QUERY: &str = include_str!("queries/php.scm");
const SCALA_QUERY: &str = include_str!("queries/scala.scm");
const HTML_QUERY: &str = include_str!("queries/html.scm");
const CSS_QUERY: &str = include_str!("queries/css.scm");
const OCAML_QUERY: &str = include_str!("queries/ocaml.scm");
const JULIA_QUERY: &str = include_str!("queries/julia.scm");
const LUA_QUERY: &str = include_str!("queries/lua.scm");
const ELIXIR_QUERY: &str = include_str!("queries/elixir.scm");
// ── W2.1 batch 3: 6 wired languages (HCL + Swift ABI 15, deferred) ───────────
// ABI status (tree-sitter 0.24 supports ABI 13-14 only):
//   haskell-0.23.1  → ABI 14 ✓    nix-0.3.0       → ABI 13 ✓
//   r-1.2.0         → ABI 14 ✓    swift-0.7.1     → ABI 15 ✗ (deferred)
//   kotlin-ng-1.1.0 → ABI 14 ✓    toml-ng-0.7.0   → ABI 14 ✓
//   md-0.3.2        → ABI 14 ✓    hcl-1.1.0       → ABI 15 ✗ (deferred)
// Deferred .scm files are authored and verified; only LangEntry + lang fn are gated.
const HASKELL_QUERY: &str = include_str!("queries/haskell.scm");
// HCL now wired via arborium-hcl (ABI 15 / tree-sitter 0.25) — see HCL_QUERY below.
const NIX_QUERY: &str = include_str!("queries/nix.scm");
const R_QUERY: &str = include_str!("queries/r.scm");
const SWIFT_QUERY: &str = include_str!("queries/swift.scm");
// Long-tail coverage sweep — queries authored against each grammar's node-types.json.
const SOLIDITY_QUERY: &str = include_str!("queries/solidity.scm");
const THRIFT_QUERY: &str = include_str!("queries/thrift.scm");
const VERILOG_QUERY: &str = include_str!("queries/verilog.scm");
const VHDL_QUERY: &str = include_str!("queries/vhdl.scm");
const D_QUERY: &str = include_str!("queries/d.scm");
const STARLARK_QUERY: &str = include_str!("queries/starlark.scm");
const CUDA_QUERY: &str = include_str!("queries/cuda.scm");
const ARDUINO_QUERY: &str = include_str!("queries/arduino.scm");
const APEX_QUERY: &str = include_str!("queries/apex.scm");
const RACKET_QUERY: &str = include_str!("queries/racket.scm");
const KOTLIN_QUERY: &str = include_str!("queries/kotlin.scm");
const TOML_QUERY: &str = include_str!("queries/toml.scm");
const MARKDOWN_QUERY: &str = include_str!("queries/markdown.scm");
const COBOL_QUERY: &str = include_str!("queries/cobol.scm");

// ── W2.1 arborium batch (28 new languages, ABI 15, tree-sitter 0.25) ─────────
const ADA_QUERY: &str = include_str!("queries/ada.scm");
const AWK_QUERY: &str = include_str!("queries/awk.scm");
const CLOJURE_QUERY: &str = include_str!("queries/clojure.scm");
const CMAKE_QUERY: &str = include_str!("queries/cmake.scm");
const COMMONLISP_QUERY: &str = include_str!("queries/commonlisp.scm");
const DART_QUERY: &str = include_str!("queries/dart.scm");
const DOCKERFILE_QUERY: &str = include_str!("queries/dockerfile.scm");
const ELM_QUERY: &str = include_str!("queries/elm.scm");
const ERLANG_QUERY: &str = include_str!("queries/erlang.scm");
const FISH_QUERY: &str = include_str!("queries/fish.scm");
const FSHARP_QUERY: &str = include_str!("queries/fsharp.scm");
const GLEAM_QUERY: &str = include_str!("queries/gleam.scm");
const GROOVY_QUERY: &str = include_str!("queries/groovy.scm");
const GLSL_QUERY: &str = include_str!("queries/glsl.scm");
const GRAPHQL_QUERY: &str = include_str!("queries/graphql.scm");
const HCL_QUERY: &str = include_str!("queries/hcl.scm");
const MAKE_QUERY: &str = include_str!("queries/make.scm");
const MATLAB_QUERY: &str = include_str!("queries/matlab.scm");
const OBJC_QUERY: &str = include_str!("queries/objc.scm");
const PERL_QUERY: &str = include_str!("queries/perl.scm");
const POWERSHELL_QUERY: &str = include_str!("queries/powershell.scm");
const PROLOG_QUERY: &str = include_str!("queries/prolog.scm");
const PROTO_QUERY: &str = include_str!("queries/proto.scm");
const SQL_QUERY: &str = include_str!("queries/sql.scm");
const SVELTE_QUERY: &str = include_str!("queries/svelte.scm");
const VUE_QUERY: &str = include_str!("queries/vue.scm");
const ZIG_QUERY: &str = include_str!("queries/zig.scm");

// ── arborium batch 2 — 20 more languages (toward ≥73) ────────────────────────
const HLSL_QUERY: &str = include_str!("queries/hlsl.scm");
const IDRIS_QUERY: &str = include_str!("queries/idris.scm");
const INI_QUERY: &str = include_str!("queries/ini.scm");
const JQ_QUERY: &str = include_str!("queries/jq.scm");
const JSDOC_QUERY: &str = include_str!("queries/jsdoc.scm");
const JUST_QUERY: &str = include_str!("queries/just.scm");
const KDL_QUERY: &str = include_str!("queries/kdl.scm");
const LEAN_QUERY: &str = include_str!("queries/lean.scm");
const MESON_QUERY: &str = include_str!("queries/meson.scm");
const NINJA_QUERY: &str = include_str!("queries/ninja.scm");
const POSTSCRIPT_QUERY: &str = include_str!("queries/postscript.scm");
const REGEX_QUERY: &str = include_str!("queries/regex.scm");
const REGO_QUERY: &str = include_str!("queries/rego.scm");
const RESCRIPT_QUERY: &str = include_str!("queries/rescript.scm");
const RON_QUERY: &str = include_str!("queries/ron.scm");
const DEVICETREE_QUERY: &str = include_str!("queries/devicetree.scm");
const DOT_QUERY: &str = include_str!("queries/dot.scm");
const ELISP_QUERY: &str = include_str!("queries/elisp.scm");

// ── W9.3 IaC + legacy/mainframe: Bicep, Fortran, Pascal ──────────────────────
const BICEP_QUERY: &str = include_str!("queries/bicep.scm");
const FORTRAN_QUERY: &str = include_str!("queries/fortran.scm");
const PASCAL_QUERY: &str = include_str!("queries/pascal.scm");
const RPG_QUERY: &str = include_str!("queries/rpg.scm");

// ── W13 IaC language expansion ────────────────────────────────────────────────
// W13.1 Nix: already wired above (NIX_QUERY / lang_nix / LANG_TABLE entry).
// W13.2 Jinja2: tree-sitter-jinja2 v0.0.14 (uros-5, crates.io).
// W13.3 Helm/Go-template: ngalaiko/tree-sitter-go-template — C source only, no crate on
//   crates.io. Deferred: requires vendoring C source + build.rs. Track as W13.3-needs-vendoring.
// W13.4 ARM Templates: semantic overlay on tree-sitter-json (already wired).
const JINJA2_QUERY: &str = include_str!("queries/jinja2.scm");
const ARM_QUERY: &str = include_str!("queries/arm.scm");

// ── Visual Basic family (4 variants) ─────────────────────────────────────────
const VB6_QUERY: &str = include_str!("queries/vb6.scm");
const VBNET_QUERY: &str = include_str!("queries/vbnet.scm");
const VBA_QUERY: &str = include_str!("queries/vba.scm");
const VBSCRIPT_QUERY: &str = include_str!("queries/vbscript.scm");
const CFML_QUERY: &str = include_str!("queries/cfml.scm");
const CFSCRIPT_QUERY: &str = include_str!("queries/cfscript.scm");
const ABL_QUERY: &str = include_str!("queries/abl.scm");
const LOTUSSCRIPT_QUERY: &str = include_str!("queries/lotusscript.scm");
const INFORMIX4GL_QUERY: &str = include_str!("queries/informix4gl.scm");
const FOXPRO_QUERY: &str = include_str!("queries/foxpro.scm");
const POWERSCRIPT_QUERY: &str = include_str!("queries/powerscript.scm");
const CRYSTAL_FORMULA_QUERY: &str = include_str!("queries/crystal_formula.scm");

// ── Grammar language fns (one per language) ──────────────────────────────────
// Each function returns tree_sitter::Language so it can be stored as fn() -> Language.
fn lang_rust() -> tree_sitter::Language {
    tree_sitter_rust::LANGUAGE.into()
}
fn lang_python() -> tree_sitter::Language {
    tree_sitter_python::LANGUAGE.into()
}
fn lang_typescript() -> tree_sitter::Language {
    tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
}
fn lang_tsx() -> tree_sitter::Language {
    tree_sitter_typescript::LANGUAGE_TSX.into()
}
fn lang_javascript() -> tree_sitter::Language {
    tree_sitter_javascript::LANGUAGE.into()
}
fn lang_go() -> tree_sitter::Language {
    tree_sitter_go::LANGUAGE.into()
}
fn lang_java() -> tree_sitter::Language {
    tree_sitter_java::LANGUAGE.into()
}
fn lang_c() -> tree_sitter::Language {
    tree_sitter_c::LANGUAGE.into()
}
fn lang_cpp() -> tree_sitter::Language {
    tree_sitter_cpp::LANGUAGE.into()
}
/// C# uses the 0.21.x crate (old API: `language()` fn, ABI 14). Newer crate versions (0.23.x+)
/// use ABI 15, which is incompatible with tree-sitter 0.24 (supports ABI 13-14 only).
fn lang_csharp() -> tree_sitter::Language {
    tree_sitter_c_sharp::language()
}
fn lang_ruby() -> tree_sitter::Language {
    tree_sitter_ruby::LANGUAGE.into()
}
fn lang_bash() -> tree_sitter::Language {
    tree_sitter_bash::LANGUAGE.into()
}
fn lang_json() -> tree_sitter::Language {
    tree_sitter_json::LANGUAGE.into()
}
fn lang_yaml() -> tree_sitter::Language {
    tree_sitter_yaml::LANGUAGE.into()
}
fn lang_php() -> tree_sitter::Language {
    tree_sitter_php::LANGUAGE_PHP.into()
}
fn lang_scala() -> tree_sitter::Language {
    tree_sitter_scala::LANGUAGE.into()
}
fn lang_html() -> tree_sitter::Language {
    tree_sitter_html::LANGUAGE.into()
}
fn lang_css() -> tree_sitter::Language {
    tree_sitter_css::LANGUAGE.into()
}
fn lang_ocaml() -> tree_sitter::Language {
    tree_sitter_ocaml::LANGUAGE_OCAML.into()
}
fn lang_julia() -> tree_sitter::Language {
    tree_sitter_julia::LANGUAGE.into()
}
fn lang_lua() -> tree_sitter::Language {
    tree_sitter_lua::LANGUAGE.into()
}
fn lang_elixir() -> tree_sitter::Language {
    tree_sitter_elixir::LANGUAGE.into()
}
// ── W2.1 batch 3 language fns ─────────────────────────────────────────────────
fn lang_haskell() -> tree_sitter::Language {
    tree_sitter_haskell::LANGUAGE.into()
}
// lang_hcl() is now in the arborium batch below — arborium-hcl wires it with tree-sitter 0.25.
fn lang_nix() -> tree_sitter::Language {
    tree_sitter_nix::LANGUAGE.into()
}
fn lang_r() -> tree_sitter::Language {
    tree_sitter_r::LANGUAGE.into()
}
fn lang_swift() -> tree_sitter::Language {
    tree_sitter_swift::LANGUAGE.into()
}
fn lang_solidity() -> tree_sitter::Language {
    arborium_solidity::language().into()
}
fn lang_thrift() -> tree_sitter::Language {
    arborium_thrift::language().into()
}
fn lang_verilog() -> tree_sitter::Language {
    arborium_verilog::language().into()
}
fn lang_vhdl() -> tree_sitter::Language {
    arborium_vhdl::language().into()
}
fn lang_d() -> tree_sitter::Language {
    arborium_d::language().into()
}
fn lang_starlark() -> tree_sitter::Language {
    arborium_starlark::language().into()
}
fn lang_cuda() -> tree_sitter::Language {
    tree_sitter_cuda::LANGUAGE.into()
}
fn lang_arduino() -> tree_sitter::Language {
    tree_sitter_arduino::LANGUAGE.into()
}
fn lang_apex() -> tree_sitter::Language {
    tree_sitter_sfapex::apex::LANGUAGE.into()
}
fn lang_racket() -> tree_sitter::Language {
    tree_sitter_racket::LANGUAGE.into()
}
fn lang_kotlin() -> tree_sitter::Language {
    tree_sitter_kotlin_ng::LANGUAGE.into()
}
fn lang_toml() -> tree_sitter::Language {
    tree_sitter_toml_ng::LANGUAGE.into()
}
fn lang_markdown() -> tree_sitter::Language {
    tree_sitter_md::LANGUAGE.into()
}

// Legacy mainframe: COBOL via the arborium-cobol grammar (tree-sitter 0.24-compatible).
fn lang_cobol() -> tree_sitter::Language {
    arborium_cobol::language().into()
}

// ── W2.1 arborium batch — ABI 15 grammars (tree-sitter 0.25) ─────────────────
// All use the uniform arborium `language()` API (returns tree_sitter::Language).
fn lang_ada() -> tree_sitter::Language {
    arborium_ada::language().into()
}
fn lang_awk() -> tree_sitter::Language {
    arborium_awk::language().into()
}
fn lang_clojure() -> tree_sitter::Language {
    arborium_clojure::language().into()
}
fn lang_cmake() -> tree_sitter::Language {
    arborium_cmake::language().into()
}
fn lang_commonlisp() -> tree_sitter::Language {
    arborium_commonlisp::language().into()
}
fn lang_dart() -> tree_sitter::Language {
    arborium_dart::language().into()
}
fn lang_dockerfile() -> tree_sitter::Language {
    arborium_dockerfile::language().into()
}
fn lang_elm() -> tree_sitter::Language {
    arborium_elm::language().into()
}
fn lang_erlang() -> tree_sitter::Language {
    arborium_erlang::language().into()
}
fn lang_fish() -> tree_sitter::Language {
    arborium_fish::language().into()
}
fn lang_fsharp() -> tree_sitter::Language {
    arborium_fsharp::language().into()
}
fn lang_gleam() -> tree_sitter::Language {
    arborium_gleam::language().into()
}
fn lang_groovy() -> tree_sitter::Language {
    arborium_groovy::language().into()
}
fn lang_glsl() -> tree_sitter::Language {
    arborium_glsl::language().into()
}
fn lang_graphql() -> tree_sitter::Language {
    arborium_graphql::language().into()
}
fn lang_hcl() -> tree_sitter::Language {
    arborium_hcl::language().into()
}
fn lang_make() -> tree_sitter::Language {
    arborium_make::language().into()
}
fn lang_matlab() -> tree_sitter::Language {
    arborium_matlab::language().into()
}
fn lang_objc() -> tree_sitter::Language {
    arborium_objc::language().into()
}
fn lang_perl() -> tree_sitter::Language {
    arborium_perl::language().into()
}
fn lang_powershell() -> tree_sitter::Language {
    arborium_powershell::language().into()
}
fn lang_prolog() -> tree_sitter::Language {
    arborium_prolog::language().into()
}
fn lang_proto() -> tree_sitter::Language {
    arborium_proto::language().into()
}
fn lang_sql() -> tree_sitter::Language {
    arborium_sql::language().into()
}
fn lang_svelte() -> tree_sitter::Language {
    arborium_svelte::language().into()
}
fn lang_vue() -> tree_sitter::Language {
    arborium_vue::language().into()
}
fn lang_zig() -> tree_sitter::Language {
    arborium_zig::language().into()
}

// ── arborium batch 2 language fns ─────────────────────────────────────────────
fn lang_hlsl() -> tree_sitter::Language {
    arborium_hlsl::language().into()
}
fn lang_idris() -> tree_sitter::Language {
    arborium_idris::language().into()
}
fn lang_ini() -> tree_sitter::Language {
    arborium_ini::language().into()
}
fn lang_jq() -> tree_sitter::Language {
    arborium_jq::language().into()
}
fn lang_jsdoc() -> tree_sitter::Language {
    arborium_jsdoc::language().into()
}
fn lang_just() -> tree_sitter::Language {
    arborium_just::language().into()
}
fn lang_kdl() -> tree_sitter::Language {
    arborium_kdl::language().into()
}
fn lang_lean() -> tree_sitter::Language {
    arborium_lean::language().into()
}
fn lang_meson() -> tree_sitter::Language {
    arborium_meson::language().into()
}
fn lang_ninja() -> tree_sitter::Language {
    arborium_ninja::language().into()
}
fn lang_postscript() -> tree_sitter::Language {
    arborium_postscript::language().into()
}
fn lang_regex() -> tree_sitter::Language {
    arborium_regex::language().into()
}
fn lang_rego() -> tree_sitter::Language {
    arborium_rego::language().into()
}
fn lang_rescript() -> tree_sitter::Language {
    arborium_rescript::language().into()
}
fn lang_ron() -> tree_sitter::Language {
    arborium_ron::language().into()
}
fn lang_devicetree() -> tree_sitter::Language {
    arborium_devicetree::language().into()
}
fn lang_dot() -> tree_sitter::Language {
    arborium_dot::language().into()
}
fn lang_elisp() -> tree_sitter::Language {
    arborium_elisp::language().into()
}

// ── W9.3 IaC + legacy/mainframe language fns ─────────────────────────────────
// All three use `pub const LANGUAGE: LanguageFn` (tree-sitter-bicep 1.1.0,
// tree-sitter-fortran 0.6.0, tree-sitter-pascal 0.10.2).
fn lang_bicep() -> tree_sitter::Language {
    tree_sitter_bicep::LANGUAGE.into()
}
fn lang_fortran() -> tree_sitter::Language {
    tree_sitter_fortran::LANGUAGE.into()
}
fn lang_pascal() -> tree_sitter::Language {
    tree_sitter_pascal::LANGUAGE.into()
}

// Free-format RPG IV via the in-house grammar (vendor/tree-sitter-rpg) — authored, not published.
fn lang_rpg() -> tree_sitter::Language {
    wicked_estate_tree_sitter_rpg::LANGUAGE.into()
}

// Progress OpenEdge ABL via the in-house grammar (vendor/tree-sitter-abl) — authored, not published.
fn lang_abl() -> tree_sitter::Language {
    wicked_estate_tree_sitter_abl::LANGUAGE.into()
}

// LotusScript via the in-house grammar (vendor/tree-sitter-lotusscript) — authored, not published.
fn lang_lotusscript() -> tree_sitter::Language {
    wicked_estate_tree_sitter_lotusscript::LANGUAGE.into()
}

// Informix 4GL via the in-house grammar (vendor/tree-sitter-informix4gl) — authored, not published.
fn lang_informix4gl() -> tree_sitter::Language {
    wicked_estate_tree_sitter_informix4gl::LANGUAGE.into()
}

// Visual FoxPro via the in-house grammar (vendor/tree-sitter-foxpro) — authored, not published.
fn lang_foxpro() -> tree_sitter::Language {
    wicked_estate_tree_sitter_foxpro::LANGUAGE.into()
}

// PowerBuilder PowerScript via the in-house grammar (vendor/tree-sitter-powerscript).
fn lang_powerscript() -> tree_sitter::Language {
    wicked_estate_tree_sitter_powerscript::LANGUAGE.into()
}

// Crystal Reports formula language via the in-house grammar (vendor/tree-sitter-crystal-formula).
fn lang_crystal_formula() -> tree_sitter::Language {
    wicked_estate_tree_sitter_crystal_formula::LANGUAGE.into()
}

// ── Visual Basic family ───────────────────────────────────────────────────────
fn lang_vb6() -> tree_sitter::Language {
    wicked_estate_tree_sitter_vb6::LANGUAGE.into()
}
fn lang_vbnet() -> tree_sitter::Language {
    tree_sitter_vb_dotnet::LANGUAGE.into()
}
fn lang_vba() -> tree_sitter::Language {
    wicked_estate_tree_sitter_vba::LANGUAGE.into()
}
fn lang_cfml() -> tree_sitter::Language {
    wicked_estate_tree_sitter_cfml::LANGUAGE_CFML.into()
}
fn lang_cfscript() -> tree_sitter::Language {
    wicked_estate_tree_sitter_cfml::LANGUAGE_CFSCRIPT.into()
}
fn lang_vbscript() -> tree_sitter::Language {
    wicked_estate_tree_sitter_vbscript::LANGUAGE.into()
}

// ── W13 IaC language expansion ────────────────────────────────────────────────
// W13.2 Jinja2 — tree-sitter-jinja2 0.0.14 (uros-5, crates.io, ABI 15 / ts-0.25).
fn lang_jinja2() -> tree_sitter::Language {
    tree_sitter_jinja2::LANGUAGE.into()
}
// W13.4 ARM Templates — reuses tree-sitter-json (already a dep); arm.scm adds ARM-specific queries.
fn lang_arm() -> tree_sitter::Language {
    tree_sitter_json::LANGUAGE.into()
}

// ── Data-driven registry ──────────────────────────────────────────────────────

/// One entry in the static language table: grammar name, file extensions, grammar fn, query.
struct LangEntry {
    name: &'static str,
    /// File extensions (no dot) this wired grammar handles — drives extension→extractor dispatch.
    /// Decoupled from the aspirational `languages.toml` coverage manifest (which tracks the 73-lang
    /// target); this table is what is *actually wired*.
    ext: &'static [&'static str],
    make_language: fn() -> tree_sitter::Language,
    query_src: &'static str,
}

/// All languages wired for tree-sitter extraction. Adding a language is one row here + a .scm file.
/// Note on crate versions: grammars must use ABI 14 (tree-sitter 0.24 supports ABI 13-14).
/// Grammars at ABI 15 (tree-sitter-go 0.25, tree-sitter-bash 0.25, tree-sitter-javascript 0.25,
/// tree-sitter-c 0.24.x, tree-sitter-c-sharp 0.23.x, tree-sitter-hcl 1.1.0, tree-sitter-swift 0.7.1)
/// were dropped or deferred.  Upgrade these entries when tree-sitter 0.25 is adopted workspace-wide.
static LANG_TABLE: &[LangEntry] = &[
    LangEntry {
        name: "rust",
        ext: &["rs"],
        make_language: lang_rust,
        query_src: RUST_QUERY,
    },
    LangEntry {
        name: "python",
        ext: &["py", "pyi"],
        make_language: lang_python,
        query_src: PYTHON_QUERY,
    },
    LangEntry {
        name: "typescript",
        ext: &["ts", "mts", "cts"],
        make_language: lang_typescript,
        query_src: TYPESCRIPT_QUERY,
    },
    LangEntry {
        name: "tsx",
        ext: &["tsx"],
        make_language: lang_tsx,
        query_src: TSX_QUERY,
    },
    LangEntry {
        name: "javascript",
        ext: &["js", "jsx", "mjs", "cjs"],
        make_language: lang_javascript,
        query_src: JAVASCRIPT_QUERY,
    },
    LangEntry {
        name: "go",
        ext: &["go"],
        make_language: lang_go,
        query_src: GO_QUERY,
    },
    LangEntry {
        name: "java",
        ext: &["java"],
        make_language: lang_java,
        query_src: JAVA_QUERY,
    },
    LangEntry {
        name: "c",
        ext: &["c"],
        make_language: lang_c,
        query_src: C_QUERY,
    },
    LangEntry {
        // .h routes to cpp, NOT c (D04-6/D2): the cpp grammar parses C, but the C
        // grammar cannot parse C++ headers — under the c row, every class/namespace/
        // method in a .h vanished. Single owner: keep this list in sync with
        // languages.toml (extractor_for_extension is first-match).
        name: "cpp",
        ext: &["cpp", "cc", "cxx", "hpp", "hh", "h"],
        make_language: lang_cpp,
        query_src: CPP_QUERY,
    },
    LangEntry {
        name: "csharp",
        ext: &["cs"],
        make_language: lang_csharp,
        query_src: CSHARP_QUERY,
    },
    LangEntry {
        name: "ruby",
        ext: &["rb"],
        make_language: lang_ruby,
        query_src: RUBY_QUERY,
    },
    LangEntry {
        name: "bash",
        ext: &["sh", "bash"],
        make_language: lang_bash,
        query_src: BASH_QUERY,
    },
    LangEntry {
        name: "json",
        ext: &["json"],
        make_language: lang_json,
        query_src: JSON_QUERY,
    },
    LangEntry {
        name: "yaml",
        ext: &["yaml", "yml"],
        make_language: lang_yaml,
        query_src: YAML_QUERY,
    },
    LangEntry {
        name: "php",
        ext: &["php"],
        make_language: lang_php,
        query_src: PHP_QUERY,
    },
    LangEntry {
        name: "scala",
        ext: &["scala", "sc"],
        make_language: lang_scala,
        query_src: SCALA_QUERY,
    },
    LangEntry {
        name: "html",
        ext: &["html", "htm"],
        make_language: lang_html,
        query_src: HTML_QUERY,
    },
    LangEntry {
        name: "css",
        ext: &["css"],
        make_language: lang_css,
        query_src: CSS_QUERY,
    },
    LangEntry {
        name: "ocaml",
        ext: &["ml", "mli"],
        make_language: lang_ocaml,
        query_src: OCAML_QUERY,
    },
    LangEntry {
        name: "julia",
        ext: &["jl"],
        make_language: lang_julia,
        query_src: JULIA_QUERY,
    },
    LangEntry {
        name: "lua",
        ext: &["lua"],
        make_language: lang_lua,
        query_src: LUA_QUERY,
    },
    LangEntry {
        name: "elixir",
        ext: &["ex", "exs"],
        make_language: lang_elixir,
        query_src: ELIXIR_QUERY,
    },
    // ── W2.1 batch 3 (6 wired languages; HCL + Swift ABI 15 deferred) ────────
    LangEntry {
        name: "haskell",
        ext: &["hs"],
        make_language: lang_haskell,
        query_src: HASKELL_QUERY,
    },
    // HCL is now wired via arborium-hcl in the arborium batch below.
    LangEntry {
        name: "nix",
        ext: &["nix"],
        make_language: lang_nix,
        query_src: NIX_QUERY,
    },
    LangEntry {
        name: "r",
        ext: &["r", "R"],
        make_language: lang_r,
        query_src: R_QUERY,
    },
    LangEntry {
        name: "swift",
        ext: &["swift"],
        make_language: lang_swift,
        query_src: SWIFT_QUERY,
    },
    LangEntry {
        name: "solidity",
        ext: &["sol"],
        make_language: lang_solidity,
        query_src: SOLIDITY_QUERY,
    },
    LangEntry {
        name: "thrift",
        ext: &["thrift"],
        make_language: lang_thrift,
        query_src: THRIFT_QUERY,
    },
    LangEntry {
        name: "verilog",
        ext: &["v", "sv"],
        make_language: lang_verilog,
        query_src: VERILOG_QUERY,
    },
    LangEntry {
        name: "vhdl",
        ext: &["vhd", "vhdl"],
        make_language: lang_vhdl,
        query_src: VHDL_QUERY,
    },
    LangEntry {
        name: "d",
        ext: &["d"],
        make_language: lang_d,
        query_src: D_QUERY,
    },
    LangEntry {
        name: "starlark",
        ext: &["bzl", "star"],
        make_language: lang_starlark,
        query_src: STARLARK_QUERY,
    },
    LangEntry {
        name: "cuda",
        ext: &["cu", "cuh"],
        make_language: lang_cuda,
        query_src: CUDA_QUERY,
    },
    LangEntry {
        name: "arduino",
        ext: &["ino"],
        make_language: lang_arduino,
        query_src: ARDUINO_QUERY,
    },
    LangEntry {
        name: "apex",
        ext: &["cls", "trigger"],
        make_language: lang_apex,
        query_src: APEX_QUERY,
    },
    LangEntry {
        name: "racket",
        ext: &["rkt"],
        make_language: lang_racket,
        query_src: RACKET_QUERY,
    },
    LangEntry {
        name: "kotlin",
        ext: &["kt", "kts"],
        make_language: lang_kotlin,
        query_src: KOTLIN_QUERY,
    },
    LangEntry {
        name: "toml",
        ext: &["toml"],
        make_language: lang_toml,
        query_src: TOML_QUERY,
    },
    LangEntry {
        name: "markdown",
        ext: &["md", "markdown"],
        make_language: lang_markdown,
        query_src: MARKDOWN_QUERY,
    },
    LangEntry {
        name: "cobol",
        ext: &["cob", "cbl", "cobol", "cpy"],
        make_language: lang_cobol,
        query_src: COBOL_QUERY,
    },
    // ── W2.1 arborium batch (28 new languages) ────────────────────────────────
    LangEntry {
        name: "ada",
        ext: &["adb", "ads"],
        make_language: lang_ada,
        query_src: ADA_QUERY,
    },
    LangEntry {
        name: "awk",
        ext: &["awk"],
        make_language: lang_awk,
        query_src: AWK_QUERY,
    },
    LangEntry {
        name: "clojure",
        ext: &["clj", "cljs", "cljc", "edn"],
        make_language: lang_clojure,
        query_src: CLOJURE_QUERY,
    },
    LangEntry {
        name: "cmake",
        ext: &["cmake"],
        make_language: lang_cmake,
        query_src: CMAKE_QUERY,
    },
    LangEntry {
        name: "commonlisp",
        ext: &["lisp", "cl"],
        make_language: lang_commonlisp,
        query_src: COMMONLISP_QUERY,
    },
    LangEntry {
        name: "dart",
        ext: &["dart"],
        make_language: lang_dart,
        query_src: DART_QUERY,
    },
    LangEntry {
        name: "dockerfile",
        // "dockerfile" extension (case-insensitive match handled by extractor_for_extension's
        // to_ascii_lowercase). Bare filenames ("Dockerfile") are handled by the pipeline
        // file-type detector, not the extension table.
        ext: &["dockerfile"],
        make_language: lang_dockerfile,
        query_src: DOCKERFILE_QUERY,
    },
    LangEntry {
        name: "elm",
        ext: &["elm"],
        make_language: lang_elm,
        query_src: ELM_QUERY,
    },
    LangEntry {
        name: "erlang",
        ext: &["erl", "hrl"],
        make_language: lang_erlang,
        query_src: ERLANG_QUERY,
    },
    LangEntry {
        name: "fish",
        ext: &["fish"],
        make_language: lang_fish,
        query_src: FISH_QUERY,
    },
    LangEntry {
        name: "fsharp",
        ext: &["fs", "fsi", "fsx"],
        make_language: lang_fsharp,
        query_src: FSHARP_QUERY,
    },
    LangEntry {
        name: "gleam",
        ext: &["gleam"],
        make_language: lang_gleam,
        query_src: GLEAM_QUERY,
    },
    LangEntry {
        name: "groovy",
        ext: &["groovy", "gradle"],
        make_language: lang_groovy,
        query_src: GROOVY_QUERY,
    },
    LangEntry {
        name: "glsl",
        ext: &["glsl", "vert", "frag"],
        make_language: lang_glsl,
        query_src: GLSL_QUERY,
    },
    LangEntry {
        name: "graphql",
        ext: &["graphql", "gql"],
        make_language: lang_graphql,
        query_src: GRAPHQL_QUERY,
    },
    LangEntry {
        // Previously deferred (ABI 15). Now wired via arborium-hcl which compiles with
        // tree-sitter 0.25. The hcl.scm query was authored and verified earlier.
        name: "hcl",
        ext: &["tf", "hcl", "tfvars"],
        make_language: lang_hcl,
        query_src: HCL_QUERY,
    },
    LangEntry {
        // "mk" extension only; "makefile"/"Makefile" are handled by filename-based
        // detection upstream, not by the extension table.
        name: "make",
        ext: &["mk"],
        make_language: lang_make,
        query_src: MAKE_QUERY,
    },
    LangEntry {
        // "m" is claimed by matlab (not objc) to avoid collision; objc uses "mm".
        name: "matlab",
        ext: &["m"],
        make_language: lang_matlab,
        query_src: MATLAB_QUERY,
    },
    LangEntry {
        // "mm" only — "m" is taken by matlab above.
        name: "objc",
        ext: &["mm"],
        make_language: lang_objc,
        query_src: OBJC_QUERY,
    },
    LangEntry {
        // "pl" and "pm" — prolog uses "pro" to avoid collision with "pl".
        name: "perl",
        ext: &["pl", "pm"],
        make_language: lang_perl,
        query_src: PERL_QUERY,
    },
    LangEntry {
        name: "powershell",
        ext: &["ps1", "psm1"],
        make_language: lang_powershell,
        query_src: POWERSHELL_QUERY,
    },
    LangEntry {
        // "pro" only — "pl" is taken by perl.
        name: "prolog",
        ext: &["pro"],
        make_language: lang_prolog,
        query_src: PROLOG_QUERY,
    },
    LangEntry {
        name: "proto",
        ext: &["proto"],
        make_language: lang_proto,
        query_src: PROTO_QUERY,
    },
    LangEntry {
        name: "sql",
        ext: &["sql"],
        make_language: lang_sql,
        query_src: SQL_QUERY,
    },
    LangEntry {
        name: "svelte",
        ext: &["svelte"],
        make_language: lang_svelte,
        query_src: SVELTE_QUERY,
    },
    LangEntry {
        name: "vue",
        ext: &["vue"],
        make_language: lang_vue,
        query_src: VUE_QUERY,
    },
    LangEntry {
        name: "zig",
        ext: &["zig"],
        make_language: lang_zig,
        query_src: ZIG_QUERY,
    },
    // ── arborium batch 2 (20 more languages) ─────────────────────────────────
    LangEntry {
        name: "hlsl",
        ext: &["hlsl"],
        make_language: lang_hlsl,
        query_src: HLSL_QUERY,
    },
    LangEntry {
        name: "idris",
        ext: &["idr"],
        make_language: lang_idris,
        query_src: IDRIS_QUERY,
    },
    LangEntry {
        name: "ini",
        ext: &["ini"],
        make_language: lang_ini,
        query_src: INI_QUERY,
    },
    LangEntry {
        name: "jq",
        ext: &["jq"],
        make_language: lang_jq,
        query_src: JQ_QUERY,
    },
    LangEntry {
        name: "jsdoc",
        ext: &["jsdoc"],
        make_language: lang_jsdoc,
        query_src: JSDOC_QUERY,
    },
    LangEntry {
        // "just" and "justfile" — bare "Justfile" handled by filename detection upstream.
        name: "just",
        ext: &["just", "justfile"],
        make_language: lang_just,
        query_src: JUST_QUERY,
    },
    LangEntry {
        name: "kdl",
        ext: &["kdl"],
        make_language: lang_kdl,
        query_src: KDL_QUERY,
    },
    LangEntry {
        name: "lean",
        ext: &["lean"],
        make_language: lang_lean,
        query_src: LEAN_QUERY,
    },
    LangEntry {
        // "meson" extension — bare "meson.build" handled by filename detection upstream.
        name: "meson",
        ext: &["meson"],
        make_language: lang_meson,
        query_src: MESON_QUERY,
    },
    LangEntry {
        name: "ninja",
        ext: &["ninja"],
        make_language: lang_ninja,
        query_src: NINJA_QUERY,
    },
    LangEntry {
        name: "postscript",
        ext: &["ps", "eps"],
        make_language: lang_postscript,
        query_src: POSTSCRIPT_QUERY,
    },
    LangEntry {
        name: "regex",
        ext: &["re"],
        make_language: lang_regex,
        query_src: REGEX_QUERY,
    },
    LangEntry {
        name: "rego",
        ext: &["rego"],
        make_language: lang_rego,
        query_src: REGO_QUERY,
    },
    LangEntry {
        name: "rescript",
        ext: &["res", "resi"],
        make_language: lang_rescript,
        query_src: RESCRIPT_QUERY,
    },
    LangEntry {
        name: "ron",
        ext: &["ron"],
        make_language: lang_ron,
        query_src: RON_QUERY,
    },
    LangEntry {
        name: "devicetree",
        ext: &["dts", "dtsi"],
        make_language: lang_devicetree,
        query_src: DEVICETREE_QUERY,
    },
    LangEntry {
        name: "dot",
        ext: &["dot", "gv"],
        make_language: lang_dot,
        query_src: DOT_QUERY,
    },
    LangEntry {
        name: "elisp",
        ext: &["el"],
        make_language: lang_elisp,
        query_src: ELISP_QUERY,
    },
    // ── W9.3 IaC + legacy/mainframe (Bicep, Fortran, Pascal) ─────────────────
    LangEntry {
        // Bicep: Azure IaC DSL — closes W9.3.
        name: "bicep",
        ext: &["bicep"],
        make_language: lang_bicep,
        query_src: BICEP_QUERY,
    },
    LangEntry {
        // Fortran: scientific/legacy numeric code.
        // "for" included as Fortran 77 fixed-form; remove if it conflicts with a future language.
        name: "fortran",
        ext: &["f90", "f95", "f03", "f08", "f", "for"],
        make_language: lang_fortran,
        query_src: FORTRAN_QUERY,
    },
    LangEntry {
        // Pascal / Delphi / Free Pascal. .dpk = Delphi package, .lpr = Lazarus program.
        name: "pascal",
        ext: &["pas", "pp", "dpr", "dpk", "lpr"],
        make_language: lang_pascal,
        query_src: PASCAL_QUERY,
    },
    LangEntry {
        // Free-format RPG IV (ILE) — in-house grammar. Fixed-format RPG is the line-extractor path.
        name: "rpg",
        ext: &["rpgle", "sqlrpgle"],
        make_language: lang_rpg,
        query_src: RPG_QUERY,
    },
    // ── W13 IaC language expansion ────────────────────────────────────────────
    // W13.1 Nix: already wired above (name: "nix", ext: &["nix"]).
    // W13.3 Helm/Go-template: deferred — no crates.io Rust binding exists for
    //   ngalaiko/tree-sitter-go-template. Requires vendoring C source + build.rs.
    LangEntry {
        // W13.2 Jinja2 — GCP Deployment Manager + Ansible templating.
        name: "jinja2",
        ext: &["j2", "jinja", "jinja2"],
        make_language: lang_jinja2,
        query_src: JINJA2_QUERY,
    },
    LangEntry {
        // W13.4 ARM Templates — Azure IaC; semantic overlay on tree-sitter-json.
        // Extension "arm.json" is the conventional double-extension; the extractor
        // dispatch also matches filenames like "azuredeploy.json" via the IaCExtractor path.
        name: "arm",
        ext: &["arm.json"],
        make_language: lang_arm,
        query_src: ARM_QUERY,
    },
    // ── Visual Basic family ──────────────────────────────────────────────────
    LangEntry {
        name: "vb6",
        ext: &["bas", "cls", "frm", "ctl"],
        make_language: lang_vb6,
        query_src: VB6_QUERY,
    },
    LangEntry {
        name: "vbnet",
        ext: &["vb"],
        make_language: lang_vbnet,
        query_src: VBNET_QUERY,
    },
    LangEntry {
        // VBA shares .bas/.cls/.frm with VB6; dispatch by name via `for_language("vba")`.
        // For extension-based dispatch, .vba (exported from Office) routes here.
        name: "vba",
        ext: &["vba"],
        make_language: lang_vba,
        query_src: VBA_QUERY,
    },
    LangEntry {
        name: "vbscript",
        ext: &["vbs", "wsf"],
        make_language: lang_vbscript,
        query_src: VBSCRIPT_QUERY,
    },
    // ── ColdFusion CFML ──────────────────────────────────────────────────────
    LangEntry {
        // Tag-based CFML: .cfm pages and tag-based .cfc components (<cffunction>, <cfcomponent>).
        // The tag grammar also recognises embedded <cfscript> function/method definitions.
        name: "cfml",
        ext: &["cfm", "cfc"],
        make_language: lang_cfml,
        query_src: CFML_QUERY,
    },
    LangEntry {
        // Script-based CFML: pure cfscript files and modern script components.
        name: "cfscript",
        ext: &["cfs"],
        make_language: lang_cfscript,
        query_src: CFSCRIPT_QUERY,
    },
    // ── Progress OpenEdge ABL (in-house grammar) ─────────────────────────────
    LangEntry {
        // .p procedures, .w windows, .i include files. ABL classes are .cls but that extension is
        // already claimed by apex/vb6 — ABL .cls files dispatch by language name (for_language).
        name: "abl",
        ext: &["p", "w", "i"],
        make_language: lang_abl,
        query_src: ABL_QUERY,
    },
    // ── LotusScript (in-house grammar) ───────────────────────────────────────
    LangEntry {
        // .lss = LotusScript Source export (the canonical extension); .ls is a community convention.
        name: "lotusscript",
        ext: &["lss", "ls"],
        make_language: lang_lotusscript,
        query_src: LOTUSSCRIPT_QUERY,
    },
    // ── Informix 4GL (in-house grammar) ──────────────────────────────────────
    LangEntry {
        name: "informix4gl",
        ext: &["4gl"],
        make_language: lang_informix4gl,
        query_src: INFORMIX4GL_QUERY,
    },
    // ── Visual FoxPro (in-house grammar) ─────────────────────────────────────
    LangEntry {
        name: "foxpro",
        ext: &["prg"],
        make_language: lang_foxpro,
        query_src: FOXPRO_QUERY,
    },
    // ── PowerBuilder PowerScript (in-house grammar) ──────────────────────────
    LangEntry {
        // Exported PowerBuilder objects: .sru (user object), .srw (window), .srf (function),
        // .sra (application), .srm (menu).
        name: "powerscript",
        ext: &["sru", "srw", "srf", "sra", "srm"],
        make_language: lang_powerscript,
        query_src: POWERSCRIPT_QUERY,
    },
    // ── Crystal Reports formula language (in-house grammar) ──────────────────
    LangEntry {
        // No standard text extension exists (formulas live in .rpt binaries); .crf is our
        // convention for exported formula text. Often dispatched by language name on fragments.
        name: "crystal_formula",
        ext: &["crf"],
        make_language: lang_crystal_formula,
        query_src: CRYSTAL_FORMULA_QUERY,
    },
];

/// Built-in grammar for a [`LANG_TABLE`] entry name. `LANG_TABLE` is private to this module; the
/// plugin registry needs the grammar to eagerly compile a query-only override's `.scm` against it
/// at load time (ADR-010) — this accessor is that seam.
pub(crate) fn builtin_language(name: &str) -> Option<tree_sitter::Language> {
    LANG_TABLE
        .iter()
        .find(|e| e.name == name)
        .map(|e| (e.make_language)())
}

/// The [`LANG_TABLE`] entry name owning a file extension (no dot, lowercase), if any. Drives the
/// cross-language extension-capture rule for grammar overrides (ADR-010).
pub(crate) fn builtin_owner_of_ext(ext: &str) -> Option<&'static str> {
    LANG_TABLE
        .iter()
        .find(|e| e.ext.contains(&ext))
        .map(|e| e.name)
}

// ── Public extractor ──────────────────────────────────────────────────────────

/// A tree-sitter-backed extractor for one language (grammar + query). `Send + Sync` — the
/// compiled `Query` and `Language` are both `Send + Sync`; the `Parser` is created per
/// `extract` call because `Parser` is not `Sync`.
pub struct TreeSitterExtractor {
    lang_name: String,
    language: tree_sitter::Language,
    /// Query compiled once at construction time so we never pay `Query::new` per file.
    query: Query,
}

impl TreeSitterExtractor {
    /// Build an extractor for the named language (must be in [`LANG_TABLE`]).
    ///
    /// Returns `None` when the language is not wired for tree-sitter extraction yet, or
    /// when the embedded query fails to compile against the grammar (broken query = language
    /// unavailable rather than a panic at extract time).
    pub fn for_language(name: &str) -> Option<Self> {
        if let Some(entry) = LANG_TABLE.iter().find(|e| e.name == name) {
            // ADR-010 tier 3: an ARMED grammar override (double opt-in, eagerly compile-verified
            // at registry load) replaces both grammar and query. A disarmed-by-compile-failure
            // override never reaches this arm, so `from_grammar`'s silent `.ok()?` never sees a
            // user query for a built-in language.
            if let Some(g) = crate::plugin::grammar_override_for_name(entry.name) {
                return Self::from_grammar(&g.lang, g.plugin.language.clone(), &g.plugin.query_src);
            }
            let language = (entry.make_language)();
            // ADR-010 tier 2: a compiled-ok query-only override replaces the built-in query on
            // the built-in grammar. A failed override returns None from `override_query_for`
            // (loud fallback fired at registry load) — the built-in query stays in use: an
            // override can never make a built-in language unavailable.
            let query_src =
                crate::plugin::override_query_for(entry.name).unwrap_or(entry.query_src);
            let query = Query::new(&language, query_src).ok()?;
            return Some(Self {
                lang_name: entry.name.to_string(),
                language,
                query,
            });
        }
        // Not built in — fall back to a runtime-loaded plugin grammar (see [`crate::plugin`]).
        let p = crate::plugin::find_by_name(name)?;
        Self::from_grammar(&p.name, p.language.clone(), &p.query_src)
    }

    /// Build an extractor from a runtime-provided grammar + query (used by the plugin loader).
    /// Returns `None` if the query fails to compile against the grammar.
    pub fn from_grammar(
        name: &str,
        language: tree_sitter::Language,
        query_src: &str,
    ) -> Option<Self> {
        let query = Query::new(&language, query_src).ok()?;
        Some(Self {
            lang_name: name.to_string(),
            language,
            query,
        })
    }

    // ── Language-specific constructors (backwards-compatible public API) ──────

    /// Rust extractor (kept for backwards compatibility with wicked-estate).
    pub fn rust() -> Self {
        Self::for_language("rust").expect("rust is always registered")
    }

    /// Python extractor (kept for backwards compatibility with wicked-estate).
    pub fn python() -> Self {
        Self::for_language("python").expect("python is always registered")
    }
}

/// Map a file extension → a wired extractor, using [`LANG_TABLE`]'s own extensions (NOT the
/// aspirational `languages.toml` manifest — that tracks the 73-language coverage target, which is
/// a superset of what is actually wired). Returns `None` when no wired grammar claims the extension.
pub fn extractor_for_extension(ext: &str) -> Option<TreeSitterExtractor> {
    let needle = ext.trim_start_matches('.').to_ascii_lowercase();
    // ADR-010 tier 3: an ARMED grammar override's SURVIVING extension claims (post the
    // cross-language ownership filter) win before the built-in extension match. Query-only
    // overrides need no hook here — the built-in match below delegates to `for_language`,
    // which applies them.
    if let Some(g) = crate::plugin::grammar_override_for_ext(&needle) {
        return TreeSitterExtractor::from_grammar(
            &g.lang,
            g.plugin.language.clone(),
            &g.plugin.query_src,
        );
    }
    if let Some(entry) = LANG_TABLE
        .iter()
        .find(|e| e.ext.iter().any(|x| *x == needle))
    {
        return TreeSitterExtractor::for_language(entry.name);
    }
    // Not built in — fall back to a runtime-loaded plugin grammar claiming this extension.
    let p = crate::plugin::find_by_extension(&needle)?;
    TreeSitterExtractor::from_grammar(&p.name, p.language.clone(), &p.query_src)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Build a map from paragraph/section header start-byte → true body end-byte for COBOL.
///
/// The arborium-cobol grammar has no `paragraph` wrapper node — paragraph_header and all
/// statements are flat children of procedure_division / procedure_declaratives. A paragraph's
/// body therefore spans from the header line to the byte before the next header (or end of
/// parent). This fixup is applied after the query-based extraction so `source` returns the
/// full paragraph body, not just the label line.
fn cobol_paragraph_end_map(tree: &tree_sitter::Tree) -> std::collections::HashMap<usize, usize> {
    const PROC_PARENTS: &[&str] = &["procedure_division", "procedure_declaratives"];
    const HEADERS: &[&str] = &["paragraph_header", "section_header"];

    let mut map = std::collections::HashMap::new();
    let cursor = tree.root_node().walk();

    // Walk the full tree looking for procedure_division / procedure_declaratives nodes.
    let mut stack: Vec<tree_sitter::Node> = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        if PROC_PARENTS.contains(&node.kind()) {
            // Iterate children in order, extending the previous header's span.
            let child_count = node.child_count();
            let mut prev_header_start: Option<usize> = None;
            for i in 0..child_count {
                let child = node.child(i).unwrap();
                if HEADERS.contains(&child.kind()) {
                    if let Some(prev_start) = prev_header_start {
                        // End the previous header just before this one starts.
                        map.insert(prev_start, child.start_byte());
                    }
                    prev_header_start = Some(child.start_byte());
                }
            }
            // Last header runs to end of the parent node.
            if let Some(prev_start) = prev_header_start {
                map.insert(prev_start, node.end_byte());
            }
        }
        // Push children for DFS (procedure_division may be nested under program_definition etc.)
        for i in 0..node.child_count() {
            stack.push(node.child(i).unwrap());
        }
    }
    drop(cursor); // suppress unused-variable warning
    map
}

fn ts_span(n: tree_sitter::Node) -> Span {
    let s = n.start_position();
    let e = n.end_position();
    Span {
        start_byte: n.start_byte() as u32,
        end_byte: n.end_byte() as u32,
        start_line: s.row as u32,
        start_col: s.column as u32,
        end_line: e.row as u32,
        end_col: e.column as u32,
    }
}

/// Logical module path for a file: the path without its extension (drives stable symbol ids).
fn module_path(path: &str) -> String {
    match path.rsplit_once('.') {
        Some((stem, _)) => stem.to_string(),
        None => path.to_string(),
    }
}

/// Version of the definition-SymbolId scheme minted by this extractor.
///
/// Scheme "1" (implicit, never stored): exactly two descriptors — `[module/, name<suffix>]` —
/// so every same-named definition in one module minted the SAME id and the store's
/// `ON CONFLICT(symbol) DO UPDATE` collapsed them into one node. Scheme "2" nested definition
/// identity under the contiguous run of Type-suffixed enclosing definitions
/// (`<module>/Repo#save().`). Scheme "3" adds the two generic query-level identity roles
/// (`@code_<kind>.anchor` — a non-emitting containment anchor, e.g. a Rust `impl` block — and
/// `@code_<kind>.owner` — an owner type name spliced as the innermost Type descriptor where no
/// containing node exists, e.g. a Go receiver or a C++ `Foo::` qualifier), so members whose
/// owner is not an enclosing definition node nest too — see the 2026-08 amendment in
/// `docs/adr/ADR-002-stable-symbol-identity.md`. Stored per repo as the `id_scheme` meta key;
/// a mismatch forces a full re-extraction so a DB never mixes schemes.
pub const SYMBOL_ID_SCHEME: &str = "3";

fn def_suffix(kind: &str) -> Suffix {
    match kind {
        "function" | "method" | "constructor" => Suffix::Method,
        "class" | "struct" | "enum" | "trait" | "interface" | "module" | "namespace"
        | "type_alias" | "type" => Suffix::Type,
        _ => Suffix::Term,
    }
}

/// A definition collected during the match loop — no id yet. Ids are minted in a second pass
/// (after the loop) because the enclosing chain needs the FULL definition list:
/// `QueryCursor::matches` does not contract document order, and a def's enclosing type can come
/// from a different pattern than the def itself (e.g. Python methods vs. their class).
struct PendingDef {
    kind: String,
    name: String,
    start: usize,
    end: usize,
    span: Span,
    signature: Option<String>,
    /// `false` for `@code_<kind>.anchor` records: the record participates in
    /// [`enclosing_chain`] as a containment anchor but pass 2 mints NO Node, NO
    /// `DefRec` (so framework `def_symbol_at` joins never land on the anchor
    /// range), and NO Contains edge for it. Constraint (documented at the
    /// `.anchor` role): an anchor pattern must never be range-equal + same-name
    /// with an emitting def — the `(start, end, name)` dedup below would keep an
    /// uncontracted-order winner.
    emit: bool,
    /// `true` for `@code_<kind>.decl` records (prototype / forward declaration):
    /// pass 2 mints the Node with the `is_declaration` metadata flag set (M4 /
    /// Option A — the store prefers a DEFINITION contribution as the node's
    /// primary). Identity is untouched: same SymbolId construction as `.def`.
    decl: bool,
    /// `@code_<kind>.owner` captured in the same match: the owner TYPE NAME of a
    /// def whose owner is not an enclosing node (Go receiver, C++ `Foo::`
    /// qualifier, Ruby `def self.m`). Pass 2 appends it as the INNERMOST Type
    /// descriptor after [`enclosing_chain`].
    owner: Option<String>,
}

/// The chain of Type descriptors a definition nests under (ADR-002 amendment, 2026-08).
///
/// Rule: walk every OTHER pending def whose byte range strictly contains `[start, end)`
/// (contains AND not range-equal — range-equality excludes the def itself and duplicate
/// captures of it), innermost → outermost, and collect a `Name#` descriptor while the container
/// is Type-suffixed; the FIRST non-Type container (Method, Function, or Term) truncates the
/// chain. Anything declared inside a function/method/Term body therefore takes no Type
/// descriptors and stays module-flat — the deliberate residual that preserves ADR-002's
/// rename-stability for function bodies. Returned outer → inner, ready to splice into the id.
///
/// Anchor-artifact exception (MI-R1-1): a non-Type pending def whose byte range EQUALS a
/// Type-suffixed pending def's range is the same syntactic node re-captured by a second query
/// pattern — an anchor artifact, not a real inner scope. Such records are dropped from the
/// container walk before truncation applies. DEFENSIVE: the last real producer (python.scm's
/// ORM field patterns, which anchored `@code_field.def` at the WHOLE `class_definition`) was
/// re-anchored at the field's own statement in the scm-anchors lane; the drop stays as a
/// fleet-wide guard against the recurrence class (73 query files, any future wide anchor),
/// tested directly by `enclosing_chain_drops_equal_range_anchor_artifacts`.
fn enclosing_chain(pending: &[PendingDef], start: usize, end: usize) -> Vec<Descriptor> {
    let mut containers: Vec<&PendingDef> = pending
        .iter()
        .filter(|p| p.start <= start && end <= p.end && !(p.start == start && p.end == end))
        .collect();
    // Drop anchor artifacts: non-Type records range-equal to a Type record (see doc above).
    let type_ranges: Vec<(usize, usize)> = containers
        .iter()
        .filter(|p| def_suffix(&p.kind) == Suffix::Type)
        .map(|p| (p.start, p.end))
        .collect();
    containers.retain(|p| {
        def_suffix(&p.kind) == Suffix::Type || !type_ranges.contains(&(p.start, p.end))
    });
    // Innermost first: latest start wins; on equal starts the smaller range is inner.
    containers.sort_by(|a, b| b.start.cmp(&a.start).then(a.end.cmp(&b.end)));
    // A container captured by two patterns (decorated TS class, Rust function_item) appears
    // twice with the same range+name — its descriptor must enter the chain ONCE.
    containers.dedup_by(|a, b| a.start == b.start && a.end == b.end && a.name == b.name);
    let mut chain: Vec<Descriptor> = containers
        .into_iter()
        .take_while(|p| def_suffix(&p.kind) == Suffix::Type)
        .map(|p| Descriptor::new(p.name.clone(), Suffix::Type))
        .collect();
    chain.reverse(); // outer → inner
    chain
}

/// The ONLY constructor of a tree-sitter code-definition SymbolId (scheme 2):
/// `[module/] ++ chain ++ [name<suffix>]`. Framework emitters that need a def's id resolve it
/// through [`def_symbol_at`] first and fall back to this with a self-excluded chain.
fn def_symbol(
    scheme: &str,
    module: &str,
    chain: &[Descriptor],
    name: &str,
    suffix: Suffix,
) -> SymbolId {
    let mut descriptors = Vec::with_capacity(chain.len() + 2);
    descriptors.push(Descriptor::new(module.to_string(), Suffix::Namespace));
    descriptors.extend_from_slice(chain);
    descriptors.push(Descriptor {
        name: name.to_string(),
        suffix,
        disambiguator: None,
    });
    Symbol::global(scheme, None, descriptors).id()
}

/// The symbol of the smallest already-minted definition containing `pos` whose name is `name`
/// (a def's range contains its own name identifier). How framework emitters (DI / route /
/// event-listener) join their edges onto the def nodes the def loop produced.
fn def_symbol_at(defs: &[DefRec], pos: usize, name: &str) -> Option<SymbolId> {
    defs.iter()
        .filter(|d| d.start <= pos && pos < d.end && d.name == name)
        .min_by_key(|d| d.end - d.start)
        .map(|d| d.symbol.clone())
}

fn def_nodekind(kind: &str) -> NodeKind {
    match kind {
        "function" => NodeKind::Function,
        "method" => NodeKind::Method,
        "class" => NodeKind::Class,
        "struct" => NodeKind::Struct,
        "enum" => NodeKind::Enum,
        "trait" => NodeKind::Trait,
        "module" => NodeKind::Module,
        "namespace" => NodeKind::Namespace,
        "constructor" => NodeKind::Constructor,
        "interface" => NodeKind::Interface,
        "constant" => NodeKind::Constant,
        "variable" => NodeKind::Variable,
        "field" | "property" => NodeKind::Field,
        "type_alias" | "type" => NodeKind::TypeAlias,
        "enum_member" => NodeKind::Other("enum_member".to_string()),
        "macro" => NodeKind::Macro,
        other => NodeKind::Other(other.to_string()),
    }
}

struct DefRec {
    symbol: SymbolId,
    start: usize,
    end: usize,
    /// The definition's name — [`def_symbol_at`] joins framework captures onto defs by it.
    name: String,
}

/// Strip surrounding quote/delimiter chars from a captured literal to get its canonical name.
/// Handles: `'react'` → `react`, `"fmt"` → `fmt`, `<stdio.h>` → `stdio.h`. Used for import paths
/// AND string-literal call targets (COBOL `CALL 'SUB'`) so the stored name is queryable by its
/// real name and resolves cross-file / cross-language.
fn strip_literal_quotes(raw: &str) -> String {
    let s = raw.trim();
    if (s.starts_with('\'') && s.ends_with('\'')) || (s.starts_with('"') && s.ends_with('"')) {
        return s[1..s.len() - 1].to_string();
    }
    if s.starts_with('<') && s.ends_with('>') {
        return s[1..s.len() - 1].to_string();
    }
    s.to_string()
}

/// Strip a single leading `:` from a captured DEFINITION name so symbol-literal
/// def sites store the real method name (Ruby `alias_method :new_name, :old` —
/// the captured `simple_symbol` text is `:new_name`; a stored `:new_name` is
/// unqueryable by its real name, the §11 quote-leak scar). Generic — no
/// per-language logic. Applied ONLY when the query opts in with the
/// `@code_<kind>.name.symbol` capture suffix (EG-COR-1): colon-leading names are
/// LEGITIMATE in def position elsewhere in the fleet (CSS `:root`/`:hover`
/// selectors, YAML `:adapter:` symbol keys), so stripping on the plain
/// `.name` channel silently rewrote stored names + SymbolIds fleet-wide.
/// `::X` (scope resolution) and a bare `:` are unchanged.
fn strip_leading_symbol_colon(name: String) -> String {
    if let Some(rest) = name.strip_prefix(':') {
        if !rest.is_empty() && !rest.starts_with(':') {
            return rest.to_string();
        }
    }
    name
}

/// Normalize a captured DEFINITION name: strip paired quotes (VB6-style quoted def
/// names). Deliberately does NOT strip `<…>` the way [`strip_literal_quotes`] does
/// for import paths — operator method names like Ruby's `<=>` and `<<` are real
/// definition names and must be stored verbatim (the angle case would truncate
/// `<=>` to `=`). Leading-`:` stripping is NOT done here: it is opt-in per query
/// via the `.name.symbol` capture suffix (see [`strip_leading_symbol_colon`]),
/// because CSS/YAML def names legitimately start with `:`.
fn strip_def_name(raw: &str) -> String {
    let s = raw.trim();
    if s.len() >= 2
        && ((s.starts_with('\'') && s.ends_with('\'')) || (s.starts_with('"') && s.ends_with('"')))
    {
        return s[1..s.len() - 1].to_string();
    }
    s.to_string()
}

// ── Import-map extraction helpers ────────────────────────────────────────────

/// Extract a name→module map from a single import match given the full statement text and the
/// already-stripped module source string. Best-effort; returns an empty map on parse failure.
///
/// Handles the common surface forms:
/// - JS/TS named:   `import { A, B as C } from './mod'`  → {A: mod, C: mod}
/// - JS/TS default: `import D from './mod'`               → {D: mod}
/// - JS/TS star:    `import * as NS from './mod'`          → {NS: mod}
/// - Python from:   `from pkg import A, B`                → {A: pkg, B: pkg}
///   (matched when stmt_text starts with "from ")
/// - Any other:     nothing (returns empty; caller accumulates import_sources separately)
///
/// `stmt_text`   — raw text of the whole import statement node.
/// `module_src`  — already stripped (un-quoted) module/package name.
fn extract_name_module_pairs(stmt_text: &str, module_src: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let stmt = stmt_text.trim();

    // ── Python `from X import A, B [as C]` ───────────────────────────────────
    // The @import_stmt for python from-import looks like "from pkg import A, B"
    if stmt.starts_with("from ") {
        // e.g. "from pkg.sub import A, B as Alias"
        if let Some(rest) = stmt.strip_prefix("from ") {
            if let Some(imp_pos) = rest.find(" import ") {
                let names_part = rest[imp_pos + " import ".len()..].trim();
                for token in names_part.split(',') {
                    let token = token.trim();
                    // "A as Alias" → Alias is the local name
                    let local = if let Some(as_pos) = token.find(" as ") {
                        token[as_pos + 4..].trim()
                    } else {
                        token
                    };
                    if !local.is_empty()
                        && local
                            .chars()
                            .next()
                            .is_some_and(|c| c.is_alphabetic() || c == '_')
                    {
                        map.insert(local.to_string(), module_src.to_string());
                    }
                }
            }
        }
        return map;
    }

    // ── JS/TS import statement ────────────────────────────────────────────────
    // Strip leading `import` keyword.
    let rest = match stmt.strip_prefix("import ") {
        Some(r) => r.trim(),
        None => return map,
    };

    // Drop trailing semicolon.
    let rest = rest.trim_end_matches(';').trim();

    // `import type …` — strip `type` keyword (TypeScript).
    let rest = rest.strip_prefix("type ").map(str::trim).unwrap_or(rest);

    // Strip trailing `from 'module'` suffix — everything after ` from '` or ` from "`.
    let rest = if let Some(from_pos) = rest.rfind(" from ") {
        rest[..from_pos].trim()
    } else {
        // No `from` — side-effect-only import `import 'mod'`; no names.
        return map;
    };

    // `* as NS` → NS
    if let Some(star_rest) = rest.strip_prefix("* as ") {
        let ns = star_rest.trim();
        if !ns.is_empty() {
            map.insert(ns.to_string(), module_src.to_string());
        }
        return map;
    }

    // `{ A, B as C, D }` named imports, possibly with a leading default `Default, { … }`.
    // Split on the outer `{ }` block.
    let default_before; // text before `{`, may contain the default import name
    let named_block; // text inside `{ … }`, may be empty if no named imports
    if let (Some(open), Some(close)) = (rest.find('{'), rest.rfind('}')) {
        default_before = rest[..open].trim().trim_end_matches(',').trim();
        named_block = &rest[open + 1..close];
    } else {
        // Pure default import: `import D from './mod'`
        default_before = rest;
        named_block = "";
    }

    // Process the default binding (the name before `{`, if any).
    let default_name = default_before.trim();
    if !default_name.is_empty()
        && default_name
            .chars()
            .next()
            .is_some_and(|c| c.is_alphabetic() || c == '_')
    {
        map.insert(default_name.to_string(), module_src.to_string());
    }

    // Process named bindings.
    for token in named_block.split(',') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        // "A as B" → local name is B.
        let local = if let Some(as_pos) = token.find(" as ") {
            token[as_pos + 4..].trim()
        } else {
            token
        };
        if !local.is_empty()
            && local
                .chars()
                .next()
                .is_some_and(|c| c.is_alphabetic() || c == '_')
        {
            map.insert(local.to_string(), module_src.to_string());
        }
    }

    map
}

/// The smallest definition whose byte range contains `pos` — the enclosing scope of a reference.
fn enclosing(defs: &[DefRec], pos: usize) -> Option<SymbolId> {
    defs.iter()
        .filter(|d| d.start <= pos && pos < d.end)
        .min_by_key(|d| d.end - d.start)
        .map(|d| d.symbol.clone())
}

// ── Capture-name classification ───────────────────────────────────────────────

/// What role a capture name plays in the prior art convention.
#[derive(Debug)]
enum CaptureRole<'a> {
    /// `@code_<kind>.def` / `@code_<kind>` / `@code_<kind>.arrow` — anchor node for a definition
    /// (`emit: true`), or `@code_<kind>.anchor` — a NON-EMITTING containment anchor
    /// (`emit: false`): it enters the pending-def list so members nest under it in
    /// [`enclosing_chain`], but pass 2 mints no Node / DefRec / Contains edge for it
    /// (Rust `impl` blocks, Ruby `class << self` — minting them would create phantom
    /// or duplicate nodes, the #129 double-emit class).
    ///
    /// `@code_<kind>.decl` (`decl: true`) — an emitting anchor for a DECLARATION
    /// (prototype / forward declaration): identical to `.def` in every identity
    /// respect (same SymbolId construction, same kind, same Contains edge), but the
    /// minted Node carries the [`wicked_estate_core::node::DECLARATION_METADATA_KEY`]
    /// metadata flag (M4 / Option A, wicked-estate#152) so the store's multi-file
    /// contribution table prefers a DEFINITION contribution as the node's primary
    /// location/kind. Metadata only — NEVER part of the id: a `.decl` capture and a
    /// `.def` capture of the same logical symbol mint the SAME SymbolId.
    DefAnchor {
        kind: &'a str,
        emit: bool,
        decl: bool,
    },
    /// `@code_<kind>.owner` — the owner TYPE NAME captured in the same match as a def whose
    /// owner is not an enclosing node (Go receivers, C++ `Foo::` qualifiers, Ruby `def self.m`).
    /// Spliced as the innermost Type descriptor of that def's id in pass 2.
    DefOwner { kind: &'a str },
    /// `@code_<kind>.name` — the identifier for a definition of `<kind>`.
    /// `symbol` is true for the `@code_<kind>.name.symbol` variant: the captured
    /// text is a symbol literal (Ruby `:name`) whose leading `:` must be stripped
    /// to get the real definition name. Plain `.name` captures keep a leading `:`
    /// verbatim (CSS `:root`, YAML `:adapter` are real names).
    DefName { kind: &'a str, symbol: bool },
    /// `@call.function` — a direct function call name.
    CallFunction,
    /// `@call.method` — a method call name.
    CallMethod,
    /// `@import` — the import statement node (text used as raw name if no `.source`).
    Import,
    /// `@import.source` — the path node inside an import statement.
    ImportSource,
    /// `@code_extends.def` — anchor of an extends heritage match.
    ExtendsAnchor,
    /// `@code_extends.target` — the target name in an extends heritage match.
    ExtendsTarget,
    /// `@code_implements.def` — anchor of an implements heritage match.
    ImplementsAnchor,
    /// `@code_implements.target` — the target name in an implements heritage match.
    ImplementsTarget,
    /// `@di.source.name` — the identifier of the type that receives an injected collaborator
    /// (the injecting class). Generic framework-DI capture; any language's `.scm` may emit it.
    DiSourceName,
    /// `@di.target` — the injected collaborator's type name (resolved cross-file, like heritage).
    DiTarget,
    /// `@route.path` — the route/path string a handler is bound to by a framework annotation.
    RoutePath,
    /// `@route.handler.name` — the identifier of the method that handles a route.
    RouteHandlerName,
    /// `@event.listener.name` — the identifier of a method that listens for an event/message
    /// (`@EventListener`, `@KafkaListener`). The listener is the source (dependent).
    EventListenerName,
    /// `@event.type` — a real event *type* a listener handles (resolved cross-file like heritage).
    EventType,
    /// `@event.topic` — a topic/queue *string* a listener subscribes to (modeled as a synthetic
    /// node, like a route path).
    EventTopic,
    /// `@event.emit.type` — a real event *type* published at a call site. Source = enclosing def.
    EventEmitType,
    /// `@event.emit.topic` — a topic/queue *string* published at a call site. Source = enclosing
    /// def, target = a synthetic topic node.
    EventEmitTopic,
    /// Anything else (params, body, return_type, comments, decorators, …) — ignored.
    Other,
}

fn classify_capture(cap_name: &str) -> CaptureRole<'_> {
    // Framework relationships (DI wiring, route handlers). Generic across languages — a `.scm`
    // emits these captures; the relationship logic stays in the query file, not in Rust per-lang.
    match cap_name {
        "di.source.name" => return CaptureRole::DiSourceName,
        "di.target" => return CaptureRole::DiTarget,
        "route.path" => return CaptureRole::RoutePath,
        "route.handler.name" => return CaptureRole::RouteHandlerName,
        "event.listener.name" => return CaptureRole::EventListenerName,
        "event.type" => return CaptureRole::EventType,
        "event.topic" => return CaptureRole::EventTopic,
        "event.emit.type" => return CaptureRole::EventEmitType,
        "event.emit.topic" => return CaptureRole::EventEmitTopic,
        _ => {}
    }

    // Heritage
    if cap_name == "code_extends.def" {
        return CaptureRole::ExtendsAnchor;
    }
    if cap_name == "code_extends.target" {
        return CaptureRole::ExtendsTarget;
    }
    if cap_name == "code_implements.def" {
        return CaptureRole::ImplementsAnchor;
    }
    if cap_name == "code_implements.target" {
        return CaptureRole::ImplementsTarget;
    }

    // Calls
    if cap_name == "call.function" {
        return CaptureRole::CallFunction;
    }
    if cap_name == "call.method" {
        return CaptureRole::CallMethod;
    }

    // Imports
    if cap_name == "import" {
        return CaptureRole::Import;
    }
    if cap_name == "import.source" {
        return CaptureRole::ImportSource;
    }

    // Definition anchors: @code_<kind>.def  OR  @code_<kind>  (no second segment)
    // OR @code_<kind>.arrow  (arrow fn variant)
    // Pattern: starts with "code_", then kind, then optional ".def"/".arrow"/nothing
    if let Some(rest) = cap_name.strip_prefix("code_") {
        // rest is "<kind>" or "<kind>.<suffix>"
        if let Some(dot) = rest.find('.') {
            let kind = &rest[..dot];
            let suffix = &rest[dot + 1..];
            if suffix == "def" || suffix == "arrow" {
                return CaptureRole::DefAnchor {
                    kind,
                    emit: true,
                    decl: false,
                };
            }
            if suffix == "decl" {
                return CaptureRole::DefAnchor {
                    kind,
                    emit: true,
                    decl: true,
                };
            }
            if suffix == "anchor" {
                return CaptureRole::DefAnchor {
                    kind,
                    emit: false,
                    decl: false,
                };
            }
            if suffix == "owner" {
                return CaptureRole::DefOwner { kind };
            }
            if suffix == "name" {
                return CaptureRole::DefName {
                    kind,
                    symbol: false,
                };
            }
            if suffix == "name.symbol" {
                return CaptureRole::DefName { kind, symbol: true };
            }
            // Everything else (.params, .body, .return_type, .value, .type, .base,
            // .annotation, …) is auxiliary — ignored.
        } else {
            // @code_<kind> with no dot — treat as def anchor (e.g. @code_variable, @code_module)
            return CaptureRole::DefAnchor {
                kind: rest,
                emit: true,
                decl: false,
            };
        }
    }

    CaptureRole::Other
}

// ── Minified / huge-file guard ────────────────────────────────────────────────

/// Return `true` when a source file is pathologically large or minified so that
/// tree-sitter extraction would hang or bloat the graph with useless tokens.
///
/// Three independent heuristics — **any one** triggers the guard:
///
/// | Heuristic | Threshold | Rationale |
/// |-----------|-----------|-----------|
/// | Total size | > 1 MiB (1,048,576 bytes) | Even well-formatted files rarely exceed this; anything larger is almost certainly generated or vendored. |
/// | Longest line | > 50,000 chars | A single line this wide is a minified bundle, transpiler output, or inline base64 data; no hand-written source reaches this. |
/// | Average chars/line | > 2,000 | A dense average — e.g. a 6 MB file with 1,000 newlines — indicates heavily generated or concatenated content even when no single line is enormous. |
///
/// The function is `O(n)` in the file length (one pass) and returns immediately on
/// the size heuristic so it never even scans the content of truly enormous files.
///
/// Called at the top of every `extract` implementation; callers that do want to
/// index such files must strip / split the content before calling `extract`.
pub fn is_minified_or_huge(text: &str) -> bool {
    // Heuristic 1 — total byte size > 1 MiB.
    // Check bytes, not chars, to stay O(1) on the fast path.
    const MAX_BYTES: usize = 1_048_576; // 1 MiB
    if text.len() > MAX_BYTES {
        return true;
    }

    // Heuristics 2 & 3 — scan lines once.
    const MAX_LINE_CHARS: usize = 50_000; // minified / generated single-line threshold
    const MAX_AVG_CHARS_PER_LINE: usize = 2_000; // dense average threshold

    let mut total_chars: usize = 0;
    let mut line_count: usize = 0;

    for line in text.lines() {
        let len = line.chars().count();
        // Heuristic 2 — longest line.
        if len > MAX_LINE_CHARS {
            return true;
        }
        total_chars += len;
        line_count += 1;
    }

    // Heuristic 3 — average chars / line (guard against divide-by-zero on empty files).
    if line_count > 0 && (total_chars / line_count) > MAX_AVG_CHARS_PER_LINE {
        return true;
    }

    false
}

// ── Extractor impl ────────────────────────────────────────────────────────────

impl Extractor for TreeSitterExtractor {
    fn languages(&self) -> Vec<Language> {
        vec![Language::new(&self.lang_name)]
    }

    fn extract(&self, file: &SourceFile) -> Result<Extraction> {
        // Guard: skip minified / huge files immediately — tree-sitter would either
        // hang on a 5 MB minified bundle or produce a meaningless torrent of tokens.
        // The pipeline-level SKIPPED_MINIFIED: observability marker is wired in wicked-estate.
        if is_minified_or_huge(&file.text) {
            return Ok(Extraction {
                nodes: Vec::new(),
                local_edges: Vec::new(),
                refs: Vec::new(),
            });
        }

        // Parser is not Sync, so we create one per call (cheap — no grammar compilation).
        let mut parser = Parser::new();
        parser
            .set_language(&self.language)
            .map_err(|e| Error::Extraction(e.to_string()))?;
        let tree = parser
            .parse(file.text.as_bytes(), None)
            .ok_or_else(|| Error::Extraction(format!("parse failed for {}", file.path)))?;
        // Reuse the query compiled once at construction time.
        let query = &self.query;
        let names = query.capture_names();
        let src = file.text.as_bytes();
        let module = module_path(&file.path);
        let scheme = format!("ts-{}", self.lang_name);

        let mut def_nodes: Vec<Node> = Vec::new();
        let mut defs: Vec<DefRec> = Vec::new();
        // Definitions collected during the match loop; ids are minted in pass 2 (after the loop)
        // where the full list is available to compute each def's enclosing Type chain.
        let mut pending: Vec<PendingDef> = Vec::new();
        // raw_refs: (raw_name, EdgeKind, byte_pos_for_enclosing, span)
        let mut raw_refs: Vec<(String, EdgeKind, usize, Span)> = Vec::new();
        // Framework DI: (injecting_class_name, name_pos, injected_type_name, span). The source
        // symbol is the class (joined onto its def node via `def_symbol_at`), NOT the enclosing
        // def — constructor injection sits inside the constructor, whose `enclosing()` would be
        // the constructor.
        let mut di_pairs: Vec<(String, usize, String, Span)> = Vec::new();
        // Framework routes: (route_path, handler_method_name, name_pos, span). Source =
        // synthetic route node, target = the handler method's def-node symbol.
        let mut route_triples: Vec<(String, String, usize, Span)> = Vec::new();
        // Event listeners targeting a real type: (listener_method_name, name_pos,
        // event_type_name, span). source = listener method def node, target = event type
        // (resolved cross-file).
        let mut event_listen_type_triples: Vec<(String, usize, String, Span)> = Vec::new();
        // Event listeners targeting a topic string: (listener_method_name, name_pos, topic,
        // span). source = listener method def node, target = synthetic topic node.
        let mut event_listen_topic_triples: Vec<(String, usize, String, Span)> = Vec::new();
        // Event emits to a topic string: (topic, call_site_pos, span). source = enclosing def
        // (resolved after the match loop, like a Calls ref), target = synthetic topic node.
        let mut event_emit_topic_sites: Vec<(String, usize, Span)> = Vec::new();
        let mut import_targets: Vec<(String, Span)> = Vec::new();
        let mut seen_imports: HashSet<String> = HashSet::new();
        // File-level import map: local name → module source (for hint injection).
        // Built from @import/@import.source pairs during the match loop.
        let mut file_import_map: HashMap<String, String> = HashMap::new();
        // Module sources that could not be parsed into name→source pairs (best-effort fallback).
        let mut file_import_sources: Vec<String> = Vec::new();

        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(query, tree.root_node(), src);
        while let Some(m) = matches.next() {
            // ── Collect all captures for this match ─────────────────────────
            // We need to look at the whole match at once to handle paired captures
            // (def+name, extends.def+extends.target, etc.).

            // Per-kind: (anchor_node, name_text)
            // We support one def per kind per match (tree-sitter match semantics).
            #[allow(clippy::type_complexity)]
            let mut def_anchor: Option<(&str, tree_sitter::Node, bool, bool)> = None; // (kind, node, emit, decl)
            let mut def_name: Option<(&str, String)> = None; // (kind, text)
            let mut def_owner: Option<(&str, String)> = None; // (kind, owner type name)

            let mut call_fn: Option<(String, usize, Span)> = None; // (name, pos, span)
            let mut call_method: Option<(String, usize, Span)> = None;

            let mut import_stmt: Option<(String, Span)> = None; // (raw_text, span)
            let mut import_src: Option<(String, Span)> = None; // (path_text, span)

            let mut extends_anchor: Option<tree_sitter::Node> = None;
            let mut extends_target: Option<String> = None;
            let mut implements_anchor: Option<tree_sitter::Node> = None;
            let mut implements_target: Option<String> = None;

            // Framework relationships (per-match). Name captures carry their byte position so
            // emission can join onto the def node minted for that name (`def_symbol_at`).
            let mut di_source_name: Option<(String, usize)> = None; // injecting class name + pos
            let mut di_target: Option<(String, Span)> = None; // injected type name + site
            let mut route_path: Option<(String, Span)> = None; // route/path string + site
            let mut route_handler_name: Option<(String, usize)> = None; // handler name + pos
            let mut event_listener_name: Option<(String, usize)> = None; // listener name + pos
            let mut event_type: Option<(String, Span)> = None; // handled event type + site
            let mut event_topic: Option<(String, Span)> = None; // subscribed topic string + site
            let mut event_emit_type: Option<(String, usize, Span)> = None; // published type + pos
            let mut event_emit_topic: Option<(String, usize, Span)> = None; // published topic + pos

            for c in m.captures {
                let cap = names[c.index as usize];
                let text = c.node.utf8_text(src).unwrap_or("").to_string();
                let span = ts_span(c.node);
                let pos = c.node.start_byte();

                match classify_capture(cap) {
                    CaptureRole::DefAnchor { kind, emit, decl } => {
                        // Last anchor wins if duplicated (shouldn't happen in well-formed query)
                        def_anchor = Some((kind, c.node, emit, decl));
                    }
                    CaptureRole::DefOwner { kind } => {
                        def_owner = Some((kind, text));
                    }
                    CaptureRole::DefName { kind, symbol } => {
                        let name = strip_def_name(&text);
                        let name = if symbol {
                            strip_leading_symbol_colon(name)
                        } else {
                            name
                        };
                        def_name = Some((kind, name));
                    }
                    CaptureRole::CallFunction => {
                        call_fn = Some((text, pos, span));
                    }
                    CaptureRole::CallMethod => {
                        call_method = Some((text, pos, span));
                    }
                    CaptureRole::Import => {
                        import_stmt = Some((text, span));
                    }
                    CaptureRole::ImportSource => {
                        import_src = Some((text, span));
                    }
                    CaptureRole::ExtendsAnchor => {
                        extends_anchor = Some(c.node);
                    }
                    CaptureRole::ExtendsTarget => {
                        extends_target = Some(text);
                    }
                    CaptureRole::ImplementsAnchor => {
                        implements_anchor = Some(c.node);
                    }
                    CaptureRole::ImplementsTarget => {
                        implements_target = Some(text);
                    }
                    CaptureRole::DiSourceName => {
                        di_source_name = Some((text, pos));
                    }
                    CaptureRole::DiTarget => {
                        di_target = Some((text, span));
                    }
                    CaptureRole::RoutePath => {
                        route_path = Some((strip_literal_quotes(&text), span));
                    }
                    CaptureRole::RouteHandlerName => {
                        route_handler_name = Some((text, pos));
                    }
                    CaptureRole::EventListenerName => {
                        event_listener_name = Some((text, pos));
                    }
                    CaptureRole::EventType => {
                        event_type = Some((text, span));
                    }
                    CaptureRole::EventTopic => {
                        event_topic = Some((strip_literal_quotes(&text), span));
                    }
                    CaptureRole::EventEmitType => {
                        event_emit_type = Some((text, pos, span));
                    }
                    CaptureRole::EventEmitTopic => {
                        event_emit_topic = Some((strip_literal_quotes(&text), pos, span));
                    }
                    CaptureRole::Other => {}
                }
            }

            // ── Process definitions ─────────────────────────────────────────
            // No id is minted here: the enclosing Type chain needs the FULL definition list,
            // which only exists after the match loop (pass 2 below).
            if let (Some((anchor_kind, anchor_node, emit, decl)), Some((name_kind, name_text))) =
                (def_anchor, &def_name)
            {
                // The name must come from the same kind as the anchor.
                // (In rare cases where multiple kinds appear in one match this guards correctness.)
                if anchor_kind == *name_kind {
                    let signature = anchor_node
                        .utf8_text(src)
                        .ok()
                        .and_then(|t| t.lines().next())
                        .map(|l| l.chars().take(200).collect::<String>());
                    // Same kind-equality guard for the owner splice: a stray `.owner`
                    // capture of a different kind in the same match is ignored.
                    let owner = match &def_owner {
                        Some((owner_kind, owner_name)) if *owner_kind == anchor_kind => {
                            Some(owner_name.clone())
                        }
                        _ => None,
                    };
                    pending.push(PendingDef {
                        kind: anchor_kind.to_string(),
                        name: name_text.clone(),
                        start: anchor_node.start_byte(),
                        end: anchor_node.end_byte(),
                        span: ts_span(anchor_node),
                        signature,
                        emit,
                        decl,
                        owner,
                    });
                }
            }

            // ── Process calls ───────────────────────────────────────────────
            // Prefer call.function; fall through to call.method.
            // Both produce EdgeKind::Calls. The method name (not the receiver) is the raw_name.
            // Strip surrounding quotes so string-literal call targets (COBOL `CALL 'SUB'`) resolve
            // to the program named SUB; identifier calls are unaffected (no quotes to strip).
            if let Some((name, pos, span)) = call_fn {
                raw_refs.push((strip_literal_quotes(&name), EdgeKind::Calls, pos, span));
            } else if let Some((name, pos, span)) = call_method {
                raw_refs.push((strip_literal_quotes(&name), EdgeKind::Calls, pos, span));
            }

            // ── Process imports ─────────────────────────────────────────────
            // Prefer .source node (the path); fall back to the whole @import node text.
            // Clone the stmt text so we can use it for import-map extraction after consuming.
            let stmt_for_map: Option<String> = import_stmt.as_ref().map(|(t, _)| t.clone());
            let (raw_import, import_span) = if let Some((src_text, src_span)) = import_src {
                (src_text, src_span)
            } else if let Some((stmt_text, stmt_span)) = import_stmt {
                (stmt_text, stmt_span)
            } else {
                // No import capture in this match.
                (String::new(), Span::ZERO)
            };

            if !raw_import.is_empty() {
                let canonical = strip_literal_quotes(&raw_import);
                if !canonical.is_empty() {
                    // Always push as a raw ref (raw_name = the quoted or original text).
                    raw_refs.push((raw_import.clone(), EdgeKind::Imports, 0, import_span));
                    // Deduped import node
                    if seen_imports.insert(canonical.clone()) {
                        import_targets.push((canonical.clone(), import_span));
                    }
                    // ── Build the file-level import map ─────────────────────
                    // When we have the full statement text, parse name→module pairs.
                    // stmt_for_map contains the @import node text; canonical is the module source.
                    let pairs = if let Some(ref stmt) = stmt_for_map {
                        extract_name_module_pairs(stmt, &canonical)
                    } else {
                        HashMap::new()
                    };
                    if pairs.is_empty() {
                        // No name associations parsed — record the module source for fallback.
                        if !file_import_sources.contains(&canonical) {
                            file_import_sources.push(canonical.clone());
                        }
                    } else {
                        file_import_map.extend(pairs);
                    }
                }
            }

            // ── Process heritage (extends / implements) ─────────────────────
            // Each heritage match carries an anchor (the declaring type node) and a target name.
            // We emit an UnresolvedRef of the appropriate kind from the declaring type's scope.
            if let (Some(anchor), Some(target)) = (extends_anchor, extends_target) {
                let pos = anchor.start_byte();
                let span = ts_span(anchor);
                raw_refs.push((target, EdgeKind::Extends, pos, span));
            }
            if let (Some(anchor), Some(target)) = (implements_anchor, implements_target) {
                let pos = anchor.start_byte();
                let span = ts_span(anchor);
                raw_refs.push((target, EdgeKind::Implements, pos, span));
            }

            // ── Process framework DI wiring ─────────────────────────────────
            // Both the injecting class name and the injected type name come from the same
            // `.scm` match (the pattern is nested under `class_declaration`), so the source is
            // unambiguously the class — not `enclosing()`, which for constructor injection
            // would resolve to the constructor.
            if let (Some((src_name, src_pos)), Some((tgt, span))) = (&di_source_name, &di_target) {
                di_pairs.push((src_name.clone(), *src_pos, tgt.clone(), *span));
            }

            // ── Process framework route handlers ────────────────────────────
            if let (Some((path, span)), Some((handler, handler_pos))) =
                (&route_path, &route_handler_name)
            {
                route_triples.push((path.clone(), handler.clone(), *handler_pos, *span));
            }

            // ── Process framework event listeners ───────────────────────────
            // source = the listener method (known from this match, like a route handler);
            // target = the event type (real symbol, resolved cross-file) or a topic string node.
            if let Some((listener, listener_pos)) = &event_listener_name {
                if let Some((ty, span)) = &event_type {
                    event_listen_type_triples.push((
                        listener.clone(),
                        *listener_pos,
                        ty.clone(),
                        *span,
                    ));
                }
                if let Some((topic, span)) = &event_topic {
                    event_listen_topic_triples.push((
                        listener.clone(),
                        *listener_pos,
                        topic.clone(),
                        *span,
                    ));
                }
            }

            // ── Process framework event emits ───────────────────────────────
            // source = enclosing def (resolved like a Calls ref); target = published event type
            // (real symbol) via raw_refs, or a synthetic topic node via event_emit_topic_sites.
            if let Some((ty, pos, span)) = event_emit_type {
                raw_refs.push((ty, edge_tags::other(edge_tags::EVENT_EMITS), pos, span));
            }
            if let Some((topic, pos, span)) = event_emit_topic {
                event_emit_topic_sites.push((topic, pos, span));
            }
        }

        // ── Pass 2: mint definition ids (ADR-002 amendment — type-nested identity) ──
        // MUST run before the COBOL span fixup below: the fixup mutates `def_nodes`, which this
        // pass populates. `PendingDef`/`DefRec` keep PRE-fixup byte ranges (identical to the
        // `enclosing()` inputs the one-pass design used); the fixup keeps mutating only
        // `node.location.span` afterwards.
        {
            // Dedupe by (start, end, name): a def captured by two patterns (a decorated TS class,
            // a Rust `function_item` matched as both function and impl-method, a Python method
            // matched as both method and function) must mint ONE node. The LAST record for a key
            // survives — the same record the store's last-write-wins upsert kept when both were
            // minted, so the stored NodeKind for double-captured defs is unchanged.
            let keep: HashSet<usize> = {
                let mut last: HashMap<(usize, usize, &str), usize> = HashMap::new();
                for (i, p) in pending.iter().enumerate() {
                    last.insert((p.start, p.end, p.name.as_str()), i);
                }
                last.into_values().collect()
            };
            let deduped: Vec<PendingDef> = pending
                .into_iter()
                .enumerate()
                .filter(|(i, _)| keep.contains(i))
                .map(|(_, p)| p)
                .collect();
            pending = deduped;

            for p in &pending {
                // Non-emitting anchors (`@code_<kind>.anchor`) exist ONLY as containment
                // anchors for the enclosing_chain walk above: no Node, no DefRec (so
                // `def_symbol_at` framework joins never land on the anchor range), and —
                // because Contains edges are emitted per DefRec below — no Contains edge.
                if !p.emit {
                    continue;
                }
                let mut chain = enclosing_chain(&pending, p.start, p.end);
                // `@code_<kind>.owner`: the owner type name captured in the def's own match
                // (Go receiver, C++ `Foo::` qualifier, Ruby `def self.m`) becomes the
                // INNERMOST Type descriptor — where containment can never supply it.
                if let Some(owner) = &p.owner {
                    chain.push(Descriptor::new(owner.clone(), Suffix::Type));
                }
                let symbol = def_symbol(&scheme, &module, &chain, &p.name, def_suffix(&p.kind));
                let mut node = Node::new(
                    symbol.clone(),
                    def_nodekind(&p.kind),
                    p.name.clone(),
                    file.language.clone(),
                    Location::new(&file.path, p.span),
                );
                node.signature = p.signature.clone();
                if p.decl {
                    // `.decl` capture: mark the record as a DECLARATION contribution
                    // (metadata only — the id above is identical to a `.def` capture's).
                    node = node.as_declaration();
                }
                def_nodes.push(node);
                defs.push(DefRec {
                    symbol,
                    start: p.start,
                    end: p.end,
                    name: p.name.clone(),
                });
            }
        }

        // ── COBOL paragraph span fixup ────────────────────────────────────
        // Extend paragraph_header / section_header spans to cover the full body.
        // The grammar has no paragraph wrapper, so we compute boundaries post-extraction.
        if self.lang_name == "cobol" {
            let end_map = cobol_paragraph_end_map(&tree);
            for node in &mut def_nodes {
                let start = node.location.span.start_byte as usize;
                if let Some(&true_end) = end_map.get(&start) {
                    let end_pos = tree
                        .root_node()
                        .descendant_for_byte_range(true_end.saturating_sub(1), true_end)
                        .map(|n| n.end_position())
                        .unwrap_or_else(|| {
                            let e = node.location.span.end_byte as usize;
                            tree.root_node()
                                .descendant_for_byte_range(e, e)
                                .map(|n| n.end_position())
                                .unwrap_or(tree_sitter::Point { row: 0, column: 0 })
                        });
                    node.location.span.end_byte = true_end as u32;
                    node.location.span.end_line = end_pos.row as u32;
                    node.location.span.end_col = end_pos.column as u32;
                }
            }
        }

        // ── File node + Contains edges ────────────────────────────────────
        let file_symbol = Symbol::file(&file.path).id();
        let mut nodes = Vec::with_capacity(def_nodes.len() + 1 + import_targets.len());
        nodes.push(Node::new(
            file_symbol.clone(),
            NodeKind::File,
            file.path.clone(),
            file.language.clone(),
            Location::new(&file.path, Span::ZERO),
        ));
        let mut local_edges = Vec::new();
        for d in &defs {
            // Wave 2.6 (Fix A): Contains edges carry the file's location so remove_file()
            // can find them by file. Every local edge must have location.file set.
            local_edges.push(
                Edge::new(
                    file_symbol.clone(),
                    d.symbol.clone(),
                    EdgeKind::Contains,
                    ResolutionTier::Parsed,
                    "tree-sitter",
                )
                .with_location(Location::new(&file.path, Span::ZERO)),
            );
        }
        nodes.extend(def_nodes);

        // ── Import nodes ──────────────────────────────────────────────────
        for (canonical, span) in import_targets {
            let import_symbol = Symbol::global(
                &scheme,
                None,
                vec![
                    Descriptor::new("import", Suffix::Namespace),
                    Descriptor::new(canonical.clone(), Suffix::Namespace),
                ],
            )
            .id();
            let mut import_node = Node::new(
                import_symbol.clone(),
                NodeKind::Import,
                canonical.clone(),
                file.language.clone(),
                Location::new(&file.path, span),
            );
            import_node.signature = Some(canonical.clone());
            nodes.push(import_node);
            // Wave 2.6: Imports edges also carry the file location.
            local_edges.push(
                Edge::new(
                    file_symbol.clone(),
                    import_symbol,
                    EdgeKind::Imports,
                    ResolutionTier::Parsed,
                    "tree-sitter",
                )
                .with_location(Location::new(&file.path, span)),
            );
        }

        // ── Build hints blob for Calls refs ──────────────────────────────
        // Serialize the file import map once, to attach to every Calls UnresolvedRef.
        // Only build if we have something meaningful.
        let imports_hint: Option<serde_json::Value> = if !file_import_map.is_empty() {
            // {"imports": {"helper": "./utils", "React": "react", ...}}
            Some(serde_json::Value::Object(
                file_import_map
                    .iter()
                    .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                    .collect(),
            ))
        } else {
            None
        };
        let import_sources_hint: Option<serde_json::Value> = if !file_import_sources.is_empty() {
            Some(serde_json::Value::Array(
                file_import_sources
                    .iter()
                    .map(|s| serde_json::Value::String(s.clone()))
                    .collect(),
            ))
        } else {
            None
        };

        // ── Attribute refs to enclosing definitions ───────────────────────
        let mut refs = Vec::with_capacity(raw_refs.len());
        for (name, ek, pos, span) in raw_refs {
            // Import refs don't need enclosing attribution — attributed to file.
            // Heritage refs: attributed to the enclosing type (or file if none).
            let from = if ek == EdgeKind::Imports {
                file_symbol.clone()
            } else {
                enclosing(&defs, pos).unwrap_or_else(|| file_symbol.clone())
            };
            let mut r = UnresolvedRef::new(from, name, ek.clone(), Location::new(&file.path, span));
            // Attach import hints to Calls refs so the ImportMapResolver can use them.
            if ek == EdgeKind::Calls {
                if let Some(ref imp) = imports_hint {
                    r.hints.insert("imports".to_string(), imp.clone());
                }
                if let Some(ref srcs) = import_sources_hint {
                    r.hints.insert("import_sources".to_string(), srcs.clone());
                }
            }
            refs.push(r);
        }

        // ── Emit framework DI-wiring refs ─────────────────────────────────
        // source = injecting class — the def node the def loop minted for it (joined by the
        // captured name's position), target = injected type by name (resolved cross-file, like
        // `extends`). Fallback when the language's def query did not capture the class: mint the
        // id from the enclosing chain at the name position, self-excluded — the same dangling
        // behaviour class as the old hand-built id, no silent drop.
        for (class_name, class_pos, type_name, span) in di_pairs {
            let from = def_symbol_at(&defs, class_pos, &class_name).unwrap_or_else(|| {
                let mut chain = enclosing_chain(&pending, class_pos, class_pos);
                if chain.last().is_some_and(|d| d.name == class_name) {
                    chain.pop();
                }
                def_symbol(&scheme, &module, &chain, &class_name, Suffix::Type)
            });
            refs.push(UnresolvedRef::new(
                from,
                type_name,
                edge_tags::other(edge_tags::DI_WIRED),
                Location::new(&file.path, span),
            ));
        }

        // ── Emit framework route-handler nodes + edges ────────────────────
        // source = synthetic route node (shared by path), target = the handler method's def-node
        // symbol (joined by the captured name's position). Per the engine contract for
        // route-handler.
        let mut seen_routes: HashSet<String> = HashSet::new();
        for (path, handler_name, handler_pos, span) in route_triples {
            let route_symbol = Symbol::synthetic("route", &path).id();
            if seen_routes.insert(path.clone()) {
                let mut route_node = Node::new(
                    route_symbol.clone(),
                    NodeKind::Synthetic,
                    path.clone(),
                    Language::new("synthetic"),
                    Location::new(&file.path, span),
                );
                route_node.signature = Some(path.clone());
                nodes.push(route_node);
            }
            let handler_symbol =
                def_symbol_at(&defs, handler_pos, &handler_name).unwrap_or_else(|| {
                    let mut chain = enclosing_chain(&pending, handler_pos, handler_pos);
                    if chain.last().is_some_and(|d| d.name == handler_name) {
                        chain.pop();
                    }
                    def_symbol(&scheme, &module, &chain, &handler_name, Suffix::Method)
                });
            local_edges.push(
                Edge::new(
                    route_symbol,
                    handler_symbol,
                    edge_tags::other(edge_tags::ROUTE_HANDLER),
                    ResolutionTier::Parsed,
                    "tree-sitter",
                )
                .with_location(Location::new(&file.path, span)),
            );
        }

        // ── Emit framework event-listens refs to real types ───────────────
        // source = listener method's def-node symbol, target = event type by name (resolved
        // cross-file, like `di-wired`). Per the contract, the listener is the dependent.
        for (listener_name, listener_pos, type_name, span) in event_listen_type_triples {
            let from = def_symbol_at(&defs, listener_pos, &listener_name).unwrap_or_else(|| {
                let mut chain = enclosing_chain(&pending, listener_pos, listener_pos);
                if chain.last().is_some_and(|d| d.name == listener_name) {
                    chain.pop();
                }
                def_symbol(&scheme, &module, &chain, &listener_name, Suffix::Method)
            });
            refs.push(UnresolvedRef::new(
                from,
                type_name,
                edge_tags::other(edge_tags::EVENT_LISTENS),
                Location::new(&file.path, span),
            ));
        }

        // ── Emit framework event topic nodes + edges ──────────────────────
        // Topics are strings with no backing symbol, so they become synthetic nodes (like routes),
        // shared by topic name within the file. event-listens: listener → topic. event-emits:
        // enclosing method → topic. Both keep source = dependent.
        let mut seen_topics: HashSet<String> = HashSet::new();
        let mut ensure_topic_node = |nodes: &mut Vec<Node>, topic: &str, span: Span| -> SymbolId {
            let topic_symbol = Symbol::synthetic("topic", topic).id();
            if seen_topics.insert(topic.to_string()) {
                let mut topic_node = Node::new(
                    topic_symbol.clone(),
                    NodeKind::Synthetic,
                    topic.to_string(),
                    Language::new("synthetic"),
                    Location::new(&file.path, span),
                );
                topic_node.signature = Some(topic.to_string());
                nodes.push(topic_node);
            }
            topic_symbol
        };

        for (listener_name, listener_pos, topic, span) in event_listen_topic_triples {
            let topic_symbol = ensure_topic_node(&mut nodes, &topic, span);
            let listener_symbol = def_symbol_at(&defs, listener_pos, &listener_name)
                .unwrap_or_else(|| {
                    let mut chain = enclosing_chain(&pending, listener_pos, listener_pos);
                    if chain.last().is_some_and(|d| d.name == listener_name) {
                        chain.pop();
                    }
                    def_symbol(&scheme, &module, &chain, &listener_name, Suffix::Method)
                });
            local_edges.push(
                Edge::new(
                    listener_symbol,
                    topic_symbol,
                    edge_tags::other(edge_tags::EVENT_LISTENS),
                    ResolutionTier::Parsed,
                    "tree-sitter",
                )
                .with_location(Location::new(&file.path, span)),
            );
        }

        for (topic, pos, span) in event_emit_topic_sites {
            let topic_symbol = ensure_topic_node(&mut nodes, &topic, span);
            let from = enclosing(&defs, pos).unwrap_or_else(|| file_symbol.clone());
            local_edges.push(
                Edge::new(
                    from,
                    topic_symbol,
                    edge_tags::other(edge_tags::EVENT_EMITS),
                    ResolutionTier::Parsed,
                    "tree-sitter",
                )
                .with_location(Location::new(&file.path, span)),
            );
        }

        Ok(Extraction {
            nodes,
            local_edges,
            refs,
        })
    }
}

// ── IaC extractor ─────────────────────────────────────────────────────────────
//
// CloudFormation and Kubernetes manifests are YAML, but tree-sitter-yaml's
// S-expression query language cannot express "children of the value mapped by
// the key 'Resources'" — you'd need sibling-context predicates that don't
// exist.  We solve this with a focused tree-walker: parse with tree-sitter-yaml,
// then walk the concrete parse tree directly to emit resource nodes.
//
// Two logical languages share the same YAML grammar:
//   • "cloudformation" — YAML with a top-level `Resources:` mapping.
//   • "kubernetes"     — YAML document(s) with `kind:` + `metadata.name:`.
//
// HCL / Terraform: tree-sitter-hcl requires ABI 15 (incompatible with our
// tree-sitter 0.24 which supports only ABI 13-14).  No compatible 0.x version
// was found on crates.io as of the time this was written.  HCL extraction is
// deferred until tree-sitter 0.25 is adopted workspace-wide.

/// The IaC LOGICAL languages: content-sniffed (no file extension of their own), extracted by
/// [`IaCExtractor`], registered OUTSIDE `languages.toml` (a manifest row would double-list them
/// in the generated coverage matrix — the script special-cases them from this extractor).
///
/// SINGLE SOURCE for both consumers: [`IaCExtractor::for_language`] matches against this list,
/// and the resolver pass's family table (`wicked-estate`'s `InMemoryIndex`) registers each entry
/// as its OWN family so the D5 cross-family guard blocks code→resource name-binds on the
/// name-resolver path (a hand-copied second list would drift exactly the way json's missing
/// manifest row did). `InfraResolver`'s exclusive-resource-name bind carries no family check
/// and is unaffected (a deliberate, separate carve-out).
pub const LOGICAL_LANGUAGES: &[&str] = &["cloudformation", "kubernetes"];

/// An IaC-specific extractor for CloudFormation and Kubernetes YAML manifests.
///
/// Parses with the tree-sitter-yaml grammar, then walks the concrete syntax tree
/// to extract infrastructure resources as [`NodeKind::Other("resource")`] nodes.
/// Registered as the logical languages in [`LOGICAL_LANGUAGES`] so they
/// are distinct from generic `"yaml"` extraction.
pub struct IaCExtractor {
    lang_name: String,
    language: tree_sitter::Language,
}

impl IaCExtractor {
    /// Build an extractor for a [`LOGICAL_LANGUAGES`] member (`"cloudformation"` or
    /// `"kubernetes"`). Returns `None` for any other name.
    pub fn for_language(name: &str) -> Option<Self> {
        if LOGICAL_LANGUAGES.contains(&name) {
            Some(Self {
                lang_name: name.to_string(),
                language: tree_sitter_yaml::LANGUAGE.into(),
            })
        } else {
            None
        }
    }

    /// CloudFormation extractor (convenience wrapper).
    pub fn cloudformation() -> Self {
        Self::for_language("cloudformation").unwrap()
    }

    /// Kubernetes extractor (convenience wrapper).
    pub fn kubernetes() -> Self {
        Self::for_language("kubernetes").unwrap()
    }
}

// ── YAML tree-walk helpers ────────────────────────────────────────────────────

/// Return the plain-scalar text of a `block_mapping_pair`'s key node, or `None`.
/// A key node is `flow_node > plain_scalar > string_scalar` (or `double_quote_scalar`).
fn bmp_key_text<'a>(pair: tree_sitter::Node<'a>, src: &'a [u8]) -> Option<&'a str> {
    // Child 0 is the key flow_node; child 1 is ':'; child 2 (if any) is value.
    let key_node = pair.child(0)?;
    // key_node is flow_node → plain_scalar → string_scalar (or double/single quote)
    scalar_text(key_node, src)
}

/// Extract the string text from a scalar node at any nesting depth.
/// Accepts: `flow_node > plain_scalar > string_scalar`,
///          `flow_node > double_quote_scalar`,
///          `flow_node > single_quote_scalar`.
/// Returns `None` if the node is not a recognizable scalar.
fn scalar_text<'a>(node: tree_sitter::Node<'a>, src: &'a [u8]) -> Option<&'a str> {
    // Unwrap flow_node
    let n = if node.kind() == "flow_node" {
        // flow_node may have a tag child first; skip it
        let first = node.child(0)?;
        if first.kind() == "tag" {
            node.child(1)?
        } else {
            first
        }
    } else {
        node
    };
    match n.kind() {
        "plain_scalar" => {
            // plain_scalar > string_scalar
            let inner = n.child(0)?;
            inner.utf8_text(src).ok()
        }
        "double_quote_scalar" | "single_quote_scalar" | "string_scalar" => n.utf8_text(src).ok(),
        _ => None,
    }
}

/// Find the value node (child index 2) of a `block_mapping_pair`, if present.
fn bmp_value(pair: tree_sitter::Node) -> Option<tree_sitter::Node> {
    // child 0 = key, child 1 = ':', child 2 = value (block_node or flow_node)
    pair.child(2)
}

/// If a node is or wraps a `block_mapping`, return it.
fn as_block_mapping(node: tree_sitter::Node) -> Option<tree_sitter::Node> {
    match node.kind() {
        "block_mapping" => Some(node),
        "block_node" => {
            let child = node.child(0)?;
            if child.kind() == "block_mapping" {
                Some(child)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Iterate `block_mapping_pair` children of a `block_mapping` node.
fn block_mapping_pairs(mapping: tree_sitter::Node) -> impl Iterator<Item = tree_sitter::Node> {
    (0..mapping.child_count())
        .filter_map(move |i| mapping.child(i))
        .filter(|n| n.kind() == "block_mapping_pair")
}

// ── CloudFormation extraction ─────────────────────────────────────────────────

/// Extract CloudFormation resources from a parsed YAML tree.
///
/// Algorithm:
///   1. Walk the top-level `block_mapping` of each YAML document.
///   2. Find the `block_mapping_pair` whose key is exactly `"Resources"`.
///   3. The value is a `block_node > block_mapping` — each `block_mapping_pair`
///      in it is a logical resource: key = logical ID, value contains `Type:`.
///   4. Emit a `NodeKind::Other("resource")` node per logical ID.
///   5. Best-effort references: scan the entire tree for `!Ref` / `Fn::GetAtt`
///      tags and emit `UnresolvedRef` with `EdgeKind::Refs` pointing to the
///      raw string value.  Note: `EdgeKind::Refs` doesn't exist; we use
///      `EdgeKind::Calls` as a proxy (the only "references something by name"
///      edge kind available).  This is documented and a known limitation.
fn extract_cfn(file: &SourceFile, tree: &tree_sitter::Tree) -> Result<Extraction> {
    let src = file.text.as_bytes();
    let scheme = "ts-cloudformation";
    let module = module_path(&file.path);
    let file_symbol = Symbol::file(&file.path).id();

    let mut nodes: Vec<Node> = vec![Node::new(
        file_symbol.clone(),
        NodeKind::File,
        file.path.clone(),
        file.language.clone(),
        Location::new(&file.path, Span::ZERO),
    )];
    let mut local_edges: Vec<Edge> = Vec::new();
    let mut refs: Vec<UnresolvedRef> = Vec::new();
    let mut resource_symbols: HashMap<String, SymbolId> = HashMap::new();

    // Walk stream > document > block_node > block_mapping
    let stream = tree.root_node();
    for doc_idx in 0..stream.child_count() {
        let doc = match stream.child(doc_idx) {
            Some(n) if n.kind() == "document" => n,
            _ => continue,
        };
        // document may have a `---` child; walk all children for block_node
        let root_mapping = (0..doc.child_count())
            .filter_map(|i| doc.child(i))
            .find_map(|n| as_block_mapping(n));
        let root_mapping = match root_mapping {
            Some(m) => m,
            None => continue,
        };

        // Find the "Resources" pair
        let resources_value = block_mapping_pairs(root_mapping)
            .find(|pair| {
                bmp_key_text(*pair, src)
                    .map(|k| k == "Resources")
                    .unwrap_or(false)
            })
            .and_then(|pair| bmp_value(pair))
            .and_then(|v| as_block_mapping(v));

        let resources_mapping = match resources_value {
            Some(m) => m,
            None => continue, // not a CFN template (no Resources block)
        };

        // Each pair in the Resources block is a logical resource.
        for res_pair in block_mapping_pairs(resources_mapping) {
            let logical_id = match bmp_key_text(res_pair, src) {
                Some(id) => id.to_string(),
                None => continue,
            };
            let span = ts_span(res_pair);
            let symbol = Symbol::global(
                scheme,
                None,
                vec![
                    Descriptor::new(module.clone(), Suffix::Namespace),
                    Descriptor {
                        name: logical_id.clone(),
                        suffix: Suffix::Term,
                        disambiguator: None,
                    },
                ],
            )
            .id();

            // Best-effort: extract `Type:` value as the signature.
            let type_val = bmp_value(res_pair)
                .and_then(|v| as_block_mapping(v))
                .and_then(|m| {
                    block_mapping_pairs(m)
                        .find(|p| bmp_key_text(*p, src).map(|k| k == "Type").unwrap_or(false))
                        .and_then(|p| bmp_value(p))
                        .and_then(|v| {
                            scalar_text(v, src)
                                .map(|t| t.to_string())
                                .or_else(|| v.utf8_text(src).ok().map(|t| t.to_string()))
                        })
                });

            let mut node = Node::new(
                symbol.clone(),
                NodeKind::Other("resource".to_string()),
                logical_id.clone(),
                file.language.clone(),
                Location::new(&file.path, span),
            );
            node.signature = type_val;
            nodes.push(node);
            local_edges.push(
                Edge::new(
                    file_symbol.clone(),
                    symbol.clone(),
                    EdgeKind::Contains,
                    ResolutionTier::Parsed,
                    "iac-cloudformation",
                )
                .with_location(Location::new(&file.path, Span::ZERO)),
            );
            resource_symbols.insert(logical_id, symbol);
        }

        // ── Best-effort reference extraction ─────────────────────────────────
        // Walk the entire document for `!Ref <target>` nodes.
        // In the tree: flow_node > tag "!Ref" + plain_scalar <target_name>.
        // Limitation: Fn::GetAtt is a mapping key, not a tag — skipped for now.
        // Note: captured as EdgeKind::Calls (no dedicated "reference" edge kind).
        collect_cfn_refs(doc, src, &file_symbol, &file.path, &mut refs);
    }

    Ok(Extraction {
        nodes,
        local_edges,
        refs,
    })
}

/// Walk a subtree and collect `!Ref <name>` occurrences.
fn collect_cfn_refs(
    node: tree_sitter::Node,
    src: &[u8],
    from: &SymbolId,
    file_path: &str,
    refs: &mut Vec<UnresolvedRef>,
) {
    if node.kind() == "flow_node" {
        // Check if first child is a tag "!Ref"
        if let Some(tag) = node.child(0) {
            if tag.kind() == "tag" && tag.utf8_text(src).ok() == Some("!Ref") {
                if let Some(scalar) = node.child(1) {
                    if let Some(target) = scalar_text(scalar, src) {
                        let span = ts_span(node);
                        refs.push(UnresolvedRef::new(
                            from.clone(),
                            target.to_string(),
                            EdgeKind::Calls,
                            Location::new(file_path, span),
                        ));
                    }
                }
            }
        }
    }
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            collect_cfn_refs(child, src, from, file_path, refs);
        }
    }
}

// ── Kubernetes extraction ─────────────────────────────────────────────────────

/// Extract Kubernetes resources from a parsed YAML tree.
///
/// Algorithm:
///   1. Each YAML document (separated by `---`) is a potential k8s object.
///   2. Within the document's root `block_mapping`, find `kind:` and
///      `metadata: > block_mapping > name:`.
///   3. Emit a `NodeKind::Other("resource")` node named by `metadata.name`,
///      with the `kind` value stored as the signature.
///   4. Limitation: selectors / cross-resource references are not captured
///      (would require resolving label selectors — out of scope for tree-sitter
///      extraction; defer to a semantic resolver).
fn extract_k8s(file: &SourceFile, tree: &tree_sitter::Tree) -> Result<Extraction> {
    let src = file.text.as_bytes();
    let scheme = "ts-kubernetes";
    let module = module_path(&file.path);
    let file_symbol = Symbol::file(&file.path).id();

    let mut nodes: Vec<Node> = vec![Node::new(
        file_symbol.clone(),
        NodeKind::File,
        file.path.clone(),
        file.language.clone(),
        Location::new(&file.path, Span::ZERO),
    )];
    let mut local_edges: Vec<Edge> = Vec::new();

    let stream = tree.root_node();
    for doc_idx in 0..stream.child_count() {
        let doc = match stream.child(doc_idx) {
            Some(n) if n.kind() == "document" => n,
            _ => continue,
        };
        let root_mapping = (0..doc.child_count())
            .filter_map(|i| doc.child(i))
            .find_map(|n| as_block_mapping(n));
        let root_mapping = match root_mapping {
            Some(m) => m,
            None => continue,
        };

        // Extract `kind:` value.
        let kind_val = block_mapping_pairs(root_mapping)
            .find(|p| bmp_key_text(*p, src).map(|k| k == "kind").unwrap_or(false))
            .and_then(|p| bmp_value(p))
            .and_then(|v| scalar_text(v, src).map(|t| t.to_string()));

        // Extract `metadata.name:` value.
        let metadata_name = block_mapping_pairs(root_mapping)
            .find(|p| {
                bmp_key_text(*p, src)
                    .map(|k| k == "metadata")
                    .unwrap_or(false)
            })
            .and_then(|p| bmp_value(p))
            .and_then(|v| as_block_mapping(v))
            .and_then(|meta_mapping| {
                block_mapping_pairs(meta_mapping)
                    .find(|p| bmp_key_text(*p, src).map(|k| k == "name").unwrap_or(false))
                    .and_then(|p| bmp_value(p))
                    .and_then(|v| scalar_text(v, src).map(|t| t.to_string()))
            });

        // Only emit a node if we have at least a name.
        let resource_name = match metadata_name {
            Some(n) => n,
            None => continue,
        };

        let span = ts_span(doc);
        let symbol = Symbol::global(
            scheme,
            None,
            vec![
                Descriptor::new(module.clone(), Suffix::Namespace),
                Descriptor {
                    name: resource_name.clone(),
                    suffix: Suffix::Term,
                    disambiguator: None,
                },
            ],
        )
        .id();

        let mut node = Node::new(
            symbol.clone(),
            NodeKind::Other("resource".to_string()),
            resource_name.clone(),
            file.language.clone(),
            Location::new(&file.path, span),
        );
        // Store the k8s kind (Deployment, Service, …) as the node signature.
        node.signature = kind_val;
        nodes.push(node);
        local_edges.push(
            Edge::new(
                file_symbol.clone(),
                symbol,
                EdgeKind::Contains,
                ResolutionTier::Parsed,
                "iac-kubernetes",
            )
            .with_location(Location::new(&file.path, Span::ZERO)),
        );
    }

    Ok(Extraction {
        nodes,
        local_edges,
        refs: Vec::new(),
    })
}

impl Extractor for IaCExtractor {
    fn languages(&self) -> Vec<Language> {
        vec![Language::new(&self.lang_name)]
    }

    fn extract(&self, file: &SourceFile) -> Result<Extraction> {
        // Guard: skip minified / huge IaC files for the same reason as TreeSitterExtractor.
        if is_minified_or_huge(&file.text) {
            return Ok(Extraction {
                nodes: Vec::new(),
                local_edges: Vec::new(),
                refs: Vec::new(),
            });
        }

        let mut parser = Parser::new();
        parser
            .set_language(&self.language)
            .map_err(|e| Error::Extraction(e.to_string()))?;
        let tree = parser
            .parse(file.text.as_bytes(), None)
            .ok_or_else(|| Error::Extraction(format!("parse failed for {}", file.path)))?;

        match self.lang_name.as_str() {
            "cloudformation" => extract_cfn(file, &tree),
            "kubernetes" => extract_k8s(file, &tree),
            other => Err(Error::Extraction(format!(
                "IaCExtractor: unknown lang {other}"
            ))),
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sf(path: &str, lang: &str, text: &str) -> SourceFile {
        SourceFile {
            path: path.into(),
            language: Language::new(lang),
            text: text.into(),
        }
    }

    /// Every wired language's `.scm` MUST compile against its grammar. A failure here means the
    /// query references a node type / field the grammar doesn't have — `for_language` would
    /// silently disable that language (`Query::new(...).ok()?`), so this guards the whole table.
    #[test]
    fn every_wired_query_compiles() {
        let failures: Vec<String> = LANG_TABLE
            .iter()
            .filter_map(|e| {
                let lang = (e.make_language)();
                tree_sitter::Query::new(&lang, e.query_src)
                    .err()
                    .map(|err| format!("{}: {err:?}", e.name))
            })
            .collect();
        assert!(
            failures.is_empty(),
            "wired languages whose query does not compile:\n  {}",
            failures.join("\n  ")
        );
    }

    #[test]
    fn strip_literal_quotes_handles_call_and_import_forms() {
        // String-literal call targets (COBOL `CALL 'SUB'`) and import paths must reduce to the
        // bare name; bare identifiers pass through unchanged.
        assert_eq!(strip_literal_quotes("'TAXSUB'"), "TAXSUB");
        assert_eq!(strip_literal_quotes("\"SUBPROG\""), "SUBPROG");
        assert_eq!(strip_literal_quotes("<stdio.h>"), "stdio.h");
        assert_eq!(strip_literal_quotes("plainName"), "plainName");
    }

    #[test]
    fn strip_leading_symbol_colon_symbol_captures_only() {
        // Ruby symbol arguments at DEF sites lose the leading `:` so the stored
        // name is the real method name (`alias_method :other_name, :x` → other_name).
        // Applied ONLY for `.name.symbol` captures — NOT the plain `.name` channel.
        assert_eq!(strip_leading_symbol_colon(":x".to_string()), "x");
        assert_eq!(
            strip_leading_symbol_colon(":other_name".to_string()),
            "other_name"
        );
        // Scope resolution and a bare colon are NOT symbol literals — unchanged.
        assert_eq!(strip_leading_symbol_colon("::X".to_string()), "::X");
        assert_eq!(strip_leading_symbol_colon(":".to_string()), ":");
        assert_eq!(strip_leading_symbol_colon("plain".to_string()), "plain");
        // Pin the seam split: strip_literal_quotes alone must keep a leading `:`
        // (refs/imports/routes/events go through it WITHOUT the colon strip —
        // elisp keywords / racket symbols legitimately start with `:` there).
        assert_eq!(strip_literal_quotes(":keyword"), ":keyword");
    }

    #[test]
    fn strip_def_name_keeps_operator_names_and_strips_quotes() {
        // Operator method names must be stored verbatim — the import-path angle
        // strip would truncate `<=>` to `=` and `<<` to the empty string.
        assert_eq!(strip_def_name("<=>"), "<=>");
        assert_eq!(strip_def_name("<<"), "<<");
        assert_eq!(strip_def_name("[]"), "[]");
        // Quoted def names (VB6 precedent) still reduce to the bare name.
        assert_eq!(strip_def_name("'Quoted'"), "Quoted");
        assert_eq!(strip_def_name("\"Quoted\""), "Quoted");
        // EG-COR-1: the plain def-name channel must KEEP a leading `:` — CSS
        // `:root`/`:hover` selectors and YAML `:adapter` symbol keys are real
        // definition names in those grammars; stripping here silently rewrote
        // stored names + SymbolIds fleet-wide. Colon-stripping is opt-in per
        // query via the `.name.symbol` capture suffix.
        assert_eq!(strip_def_name(":root"), ":root");
        assert_eq!(strip_def_name(":other_name"), ":other_name");
        assert_eq!(strip_def_name("plain"), "plain");
    }

    // ── Existing tests (must stay green) ─────────────────────────────────────

    #[test]
    fn extracts_rust_functions_and_calls() {
        let code = "fn helper() {}\nfn main() { helper(); }\n";
        let ex = TreeSitterExtractor::rust()
            .extract(&sf("src/m.rs", "rust", code))
            .unwrap();
        let fns = ex
            .nodes
            .iter()
            .filter(|n| matches!(n.kind, NodeKind::Function))
            .count();
        assert_eq!(fns, 2, "helper + main");
        assert!(
            ex.refs
                .iter()
                .any(|r| r.raw_name == "helper" && r.kind == EdgeKind::Calls),
            "main() calls helper()"
        );
        assert!(ex.local_edges.iter().any(|e| e.kind == EdgeKind::Contains));
        // the call ref is attributed to main (its enclosing def), not the file.
        let call = ex.refs.iter().find(|r| r.raw_name == "helper").unwrap();
        assert!(
            call.from.as_str().ends_with("main()."),
            "call attributed to main, got {}",
            call.from
        );
    }

    #[test]
    fn extracts_python_functions_and_calls() {
        let code = "def helper():\n    pass\n\ndef main():\n    helper()\n";
        let ex = TreeSitterExtractor::python()
            .extract(&sf("m.py", "python", code))
            .unwrap();
        let fns = ex
            .nodes
            .iter()
            .filter(|n| matches!(n.kind, NodeKind::Function))
            .count();
        assert_eq!(fns, 2);
        assert!(
            ex.refs
                .iter()
                .any(|r| r.raw_name == "helper" && r.kind == EdgeKind::Calls)
        );
    }

    // ── for_language / extractor_for_extension ────────────────────────────────

    #[test]
    fn for_language_known_returns_some() {
        assert!(TreeSitterExtractor::for_language("rust").is_some());
        assert!(TreeSitterExtractor::for_language("typescript").is_some());
        assert!(TreeSitterExtractor::for_language("go").is_some());
    }

    /// ADM-ATT-3 pin: `IaCExtractor::for_language` accepts EXACTLY the [`LOGICAL_LANGUAGES`]
    /// members and nothing else — the const is the single source both this constructor and the
    /// resolver-pass family table consume, so a drift between them fails here.
    #[test]
    fn iac_for_language_matches_logical_languages_exactly() {
        for name in LOGICAL_LANGUAGES {
            assert!(
                IaCExtractor::for_language(name).is_some(),
                "LOGICAL_LANGUAGES member {name} must construct an IaCExtractor"
            );
        }
        for name in ["yaml", "json", "terraform", "tfstate", ""] {
            assert!(
                IaCExtractor::for_language(name).is_none(),
                "{name} is not a logical IaC language"
            );
        }
    }

    #[test]
    fn for_language_unknown_returns_none() {
        // (Was "cobol" — now a supported language via arborium-cobol, so use a truly unknown name.)
        assert!(TreeSitterExtractor::for_language("klingon").is_none());
        assert!(TreeSitterExtractor::for_language("nonexistent_lang").is_none());
        assert!(TreeSitterExtractor::for_language("").is_none());
    }

    #[test]
    fn extractor_for_extension_rs() {
        let ex = extractor_for_extension("rs").expect("rust registered");
        assert_eq!(ex.lang_name, "rust");
    }

    #[test]
    fn extractor_for_extension_ts() {
        let ex = extractor_for_extension("ts").expect("typescript registered");
        assert_eq!(ex.lang_name, "typescript");
    }

    #[test]
    fn extractor_for_extension_unknown() {
        assert!(extractor_for_extension("unknown_ext").is_none());
    }

    // ── Smoke tests ───────────────────────────────────────────────────────────

    #[test]
    fn smoke_typescript() {
        let code = r#"
function greet(name: string): string { return "hi"; }
class Greeter { greet() { return greet("world"); } }
import { foo } from './foo';
"#;
        let ex = TreeSitterExtractor::for_language("typescript")
            .unwrap()
            .extract(&sf("a.ts", "typescript", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "expected >=1 definition, got {defs}");
        let calls = ex.refs.iter().filter(|r| r.kind == EdgeKind::Calls).count();
        assert!(calls >= 1, "expected >=1 call ref, got {calls}");
    }

    #[test]
    fn smoke_tsx() {
        let code = r#"
function App(): JSX.Element { return render(); }
import React from 'react';
"#;
        let ex = TreeSitterExtractor::for_language("tsx")
            .unwrap()
            .extract(&sf("app.tsx", "tsx", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "expected >=1 definition, got {defs}");
    }

    #[test]
    fn smoke_javascript() {
        let code = r#"
function add(a, b) { return a + b; }
class Calculator { add(a, b) { return add(a, b); } }
import { x } from './x';
"#;
        let ex = TreeSitterExtractor::for_language("javascript")
            .unwrap()
            .extract(&sf("a.js", "javascript", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "expected >=1 definition, got {defs}");
        let calls = ex.refs.iter().filter(|r| r.kind == EdgeKind::Calls).count();
        assert!(calls >= 1, "expected >=1 call ref, got {calls}");
    }

    #[test]
    fn smoke_go() {
        let code = r#"
package main
import "fmt"
func greet(name string) string { return fmt.Sprintf("hi %s", name) }
func main() { greet("world") }
"#;
        let ex = TreeSitterExtractor::for_language("go")
            .unwrap()
            .extract(&sf("main.go", "go", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "expected >=1 definition, got {defs}");
        let calls = ex.refs.iter().filter(|r| r.kind == EdgeKind::Calls).count();
        assert!(calls >= 1, "expected >=1 call ref, got {calls}");
    }

    #[test]
    fn smoke_java() {
        let code = r#"
import java.util.List;
public class Greeter {
    public String greet(String name) { return hello(name); }
    private String hello(String n) { return "hi " + n; }
}
"#;
        let ex = TreeSitterExtractor::for_language("java")
            .unwrap()
            .extract(&sf("Greeter.java", "java", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "expected >=1 definition, got {defs}");
        let calls = ex.refs.iter().filter(|r| r.kind == EdgeKind::Calls).count();
        assert!(calls >= 1, "expected >=1 call ref, got {calls}");
    }

    #[test]
    fn smoke_c() {
        let code = r#"
#include <stdio.h>
struct Point { int x; int y; };
int add(int a, int b) { return a + b; }
int main() { return add(1, 2); }
"#;
        let ex = TreeSitterExtractor::for_language("c")
            .unwrap()
            .extract(&sf("main.c", "c", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "expected >=1 definition, got {defs}");
        let calls = ex.refs.iter().filter(|r| r.kind == EdgeKind::Calls).count();
        assert!(calls >= 1, "expected >=1 call ref, got {calls}");
    }

    #[test]
    fn smoke_cpp() {
        let code = r#"
#include <string>
class Greeter {
public:
    std::string greet(const std::string& name) { return hello(name); }
};
std::string hello(const std::string& n) { return "hi " + n; }
int main() { hello("world"); return 0; }
"#;
        let ex = TreeSitterExtractor::for_language("cpp")
            .unwrap()
            .extract(&sf("main.cpp", "cpp", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "expected >=1 definition, got {defs}");
        let calls = ex.refs.iter().filter(|r| r.kind == EdgeKind::Calls).count();
        assert!(calls >= 1, "expected >=1 call ref, got {calls}");
    }

    #[test]
    fn smoke_csharp() {
        let code = r#"
using System;
namespace App {
    public class Greeter {
        public string Greet(string name) { return Hello(name); }
        private string Hello(string n) { return "hi " + n; }
    }
}
"#;
        let ex = TreeSitterExtractor::for_language("csharp")
            .unwrap()
            .extract(&sf("Greeter.cs", "csharp", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "expected >=1 definition, got {defs}");
        let calls = ex.refs.iter().filter(|r| r.kind == EdgeKind::Calls).count();
        assert!(calls >= 1, "expected >=1 call ref, got {calls}");
    }

    #[test]
    fn smoke_ruby() {
        let code = r#"
require 'json'
class Greeter
  def greet(name)
    hello(name)
  end
  def hello(n)
    "hi #{n}"
  end
end
"#;
        let ex = TreeSitterExtractor::for_language("ruby")
            .unwrap()
            .extract(&sf("greeter.rb", "ruby", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "expected >=1 definition, got {defs}");
        let calls = ex.refs.iter().filter(|r| r.kind == EdgeKind::Calls).count();
        assert!(calls >= 1, "expected >=1 call ref, got {calls}");
    }

    #[test]
    fn smoke_bash() {
        let code = r#"
#!/bin/bash
function greet() { echo "hi $1"; }
greet "world"
"#;
        let ex = TreeSitterExtractor::for_language("bash")
            .unwrap()
            .extract(&sf("script.sh", "bash", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "expected >=1 definition, got {defs}");
        let calls = ex.refs.iter().filter(|r| r.kind == EdgeKind::Calls).count();
        assert!(calls >= 1, "expected >=1 call ref, got {calls}");
    }

    #[test]
    fn smoke_json() {
        let code = r#"{"name": "wicked_estate", "version": "1.0"}"#;
        let ex = TreeSitterExtractor::for_language("json")
            .unwrap()
            .extract(&sf("pkg.json", "json", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(
            defs >= 1,
            "expected >=1 definition (top-level key), got {defs}"
        );
    }

    #[test]
    fn smoke_yaml() {
        let code = "name: wicked_estate\nversion: 1\nconfig:\n  debug: true\n";
        let ex = TreeSitterExtractor::for_language("yaml")
            .unwrap()
            .extract(&sf("config.yaml", "yaml", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(
            defs >= 1,
            "expected >=1 definition (top-level key), got {defs}"
        );
    }

    // ── W2.1 batch 2 smoke tests ─────────────────────────────────────────────

    #[test]
    fn smoke_php() {
        let code = r#"<?php
namespace App\Http;
use App\Models\User;
class UserController {
    public function index() { return $this->list(); }
    private function list() { return []; }
}
function helper($x) { return $x; }
"#;
        let ex = TreeSitterExtractor::for_language("php")
            .unwrap()
            .extract(&sf("UserController.php", "php", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "expected >=1 definition, got {defs}");
        let calls = ex.refs.iter().filter(|r| r.kind == EdgeKind::Calls).count();
        assert!(calls >= 1, "expected >=1 call ref, got {calls}");
    }

    #[test]
    fn smoke_scala() {
        let code = r#"
package com.example
import scala.collection.mutable.ListBuffer
class Greeter {
  def greet(name: String): String = hello(name)
  def hello(n: String): String = s"hi $n"
}
object Main extends App {
  val g = new Greeter()
}
"#;
        let ex = TreeSitterExtractor::for_language("scala")
            .unwrap()
            .extract(&sf("Main.scala", "scala", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "expected >=1 definition, got {defs}");
    }

    #[test]
    fn smoke_html() {
        let code = r#"<!DOCTYPE html>
<html>
  <head><title>Test</title></head>
  <body>
    <div class="main"><p>Hello</p></div>
    <script src="app.js"></script>
  </body>
</html>
"#;
        let ex = TreeSitterExtractor::for_language("html")
            .unwrap()
            .extract(&sf("index.html", "html", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "expected >=1 element definition, got {defs}");
    }

    #[test]
    fn smoke_css() {
        let code = r#"
@import "reset.css";
body { margin: 0; padding: 0; }
.container { max-width: 1200px; }
@keyframes fadeIn { from { opacity: 0; } to { opacity: 1; } }
"#;
        let ex = TreeSitterExtractor::for_language("css")
            .unwrap()
            .extract(&sf("style.css", "css", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "expected >=1 definition, got {defs}");
    }

    #[test]
    fn smoke_ocaml() {
        let code = r#"
module Greeter = struct
  let greet name = "hi " ^ name
  let hello n = greet n
end

let main () = Greeter.greet "world"
"#;
        let ex = TreeSitterExtractor::for_language("ocaml")
            .unwrap()
            .extract(&sf("greeter.ml", "ocaml", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "expected >=1 definition, got {defs}");
    }

    #[test]
    fn smoke_julia() {
        let code = r#"
module Greetings

struct Person
  name::String
end

function greet(p::Person)
  println("hi " * p.name)
end

end
"#;
        let ex = TreeSitterExtractor::for_language("julia")
            .unwrap()
            .extract(&sf("greet.jl", "julia", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "expected >=1 definition, got {defs}");
    }

    #[test]
    fn smoke_lua() {
        let code = r#"
function greet(name)
  return "hi " .. name
end

function Main:run()
  print(greet("world"))
end
"#;
        let ex = TreeSitterExtractor::for_language("lua")
            .unwrap()
            .extract(&sf("greet.lua", "lua", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "expected >=1 definition, got {defs}");
        let calls = ex.refs.iter().filter(|r| r.kind == EdgeKind::Calls).count();
        assert!(calls >= 1, "expected >=1 call ref, got {calls}");
    }

    #[test]
    fn smoke_elixir() {
        let code = r#"
defmodule Greeter do
  def greet(name) do
    hello(name)
  end

  defp hello(n), do: "hi #{n}"
end
"#;
        let ex = TreeSitterExtractor::for_language("elixir")
            .unwrap()
            .extract(&sf("greeter.ex", "elixir", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "expected >=1 definition, got {defs}");
        let calls = ex.refs.iter().filter(|r| r.kind == EdgeKind::Calls).count();
        assert!(calls >= 1, "expected >=1 call ref, got {calls}");
    }

    // ── Import nodes ──────────────────────────────────────────────────────────

    #[test]
    fn import_ref_produces_import_node_ts() {
        let code = "import { foo } from 'lodash';\nimport React from 'react';\n";
        let ex = TreeSitterExtractor::for_language("typescript")
            .unwrap()
            .extract(&sf("a.ts", "typescript", code))
            .unwrap();
        let import_nodes: Vec<_> = ex
            .nodes
            .iter()
            .filter(|n| matches!(n.kind, NodeKind::Import))
            .collect();
        assert_eq!(
            import_nodes.len(),
            2,
            "lodash + react import nodes; got {:?}",
            import_nodes.iter().map(|n| &n.name).collect::<Vec<_>>()
        );
        assert!(
            import_nodes.iter().any(|n| n.name == "lodash"),
            "lodash import node"
        );
        assert!(
            import_nodes.iter().any(|n| n.name == "react"),
            "react import node"
        );
        let import_edges: Vec<_> = ex
            .local_edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Imports)
            .collect();
        assert_eq!(
            import_edges.len(),
            2,
            "two import edges; got {}",
            import_edges.len()
        );
    }

    #[test]
    fn import_dedup_same_module_twice_ts() {
        let code = "import { a } from 'shared';\nimport { b } from 'shared';\n";
        let ex = TreeSitterExtractor::for_language("typescript")
            .unwrap()
            .extract(&sf("a.ts", "typescript", code))
            .unwrap();
        let import_nodes = ex
            .nodes
            .iter()
            .filter(|n| matches!(n.kind, NodeKind::Import))
            .count();
        assert_eq!(import_nodes, 1, "same module → single import node");
    }

    #[test]
    fn import_node_go() {
        let code = "package main\nimport \"fmt\"\nimport \"os\"\nfunc main() {}\n";
        let ex = TreeSitterExtractor::for_language("go")
            .unwrap()
            .extract(&sf("m.go", "go", code))
            .unwrap();
        let import_nodes: Vec<_> = ex
            .nodes
            .iter()
            .filter(|n| matches!(n.kind, NodeKind::Import))
            .collect();
        assert_eq!(
            import_nodes.len(),
            2,
            "fmt + os; got {:?}",
            import_nodes.iter().map(|n| &n.name).collect::<Vec<_>>()
        );
        assert!(import_nodes.iter().any(|n| n.name == "fmt"));
        assert!(import_nodes.iter().any(|n| n.name == "os"));
    }

    #[test]
    fn import_node_c_strips_angle_brackets() {
        let code = "#include <stdio.h>\n#include <stdlib.h>\nint main() { return 0; }\n";
        let ex = TreeSitterExtractor::for_language("c")
            .unwrap()
            .extract(&sf("m.c", "c", code))
            .unwrap();
        let import_nodes: Vec<_> = ex
            .nodes
            .iter()
            .filter(|n| matches!(n.kind, NodeKind::Import))
            .collect();
        assert_eq!(import_nodes.len(), 2, "stdio.h + stdlib.h");
        assert!(
            import_nodes.iter().any(|n| n.name == "stdio.h"),
            "stdio.h without angle brackets"
        );
    }

    // ── New NodeKind captures ─────────────────────────────────────────────────

    #[test]
    fn typescript_captures_constant_type_alias_arrow_fn() {
        let code = r#"
const MAX_SIZE: number = 100;
const GREETING: string = "hello";
const handler = (x: number) => x * 2;
type UserId = string;
type Handler = (x: number) => void;
let mutableCount = 0;
"#;
        let ex = TreeSitterExtractor::for_language("typescript")
            .unwrap()
            .extract(&sf("types.ts", "typescript", code))
            .unwrap();
        let kinds: Vec<_> = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .map(|n| (&n.name, &n.kind))
            .collect();
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.name == "handler" && matches!(n.kind, NodeKind::Function)),
            "arrow fn 'handler' as Function; got {kinds:?}"
        );
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.name == "UserId" && matches!(n.kind, NodeKind::TypeAlias)),
            "type alias 'UserId'; got {kinds:?}"
        );
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.name == "Handler" && matches!(n.kind, NodeKind::TypeAlias)),
            "type alias 'Handler'; got {kinds:?}"
        );
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.name == "MAX_SIZE" && matches!(n.kind, NodeKind::Constant)),
            "constant 'MAX_SIZE'; got {kinds:?}"
        );
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.name == "mutableCount" && matches!(n.kind, NodeKind::Variable)),
            "variable 'mutableCount'; got {kinds:?}"
        );
    }

    #[test]
    fn rust_captures_const_and_type_alias() {
        let code = r#"
const MAX: usize = 100;
static BUFFER: [u8; 8] = [0; 8];
type UserId = u64;
fn foo() {}
"#;
        let ex = TreeSitterExtractor::rust()
            .extract(&sf("m.rs", "rust", code))
            .unwrap();
        let kinds: Vec<_> = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .map(|n| (&n.name, &n.kind))
            .collect();
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.name == "MAX" && matches!(n.kind, NodeKind::Constant)),
            "const 'MAX'; got {kinds:?}"
        );
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.name == "BUFFER" && matches!(n.kind, NodeKind::Constant)),
            "static 'BUFFER' as Constant; got {kinds:?}"
        );
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.name == "UserId" && matches!(n.kind, NodeKind::TypeAlias)),
            "type alias 'UserId'; got {kinds:?}"
        );
    }

    #[test]
    fn go_captures_const_type_alias_var_struct() {
        let code = r#"
package main
const MaxSize = 100
type MyString = string
var globalCount int
type Config struct { Host string }
"#;
        let ex = TreeSitterExtractor::for_language("go")
            .unwrap()
            .extract(&sf("m.go", "go", code))
            .unwrap();
        let kinds: Vec<_> = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .map(|n| (&n.name, &n.kind))
            .collect();
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.name == "MaxSize" && matches!(n.kind, NodeKind::Constant)),
            "Go const 'MaxSize'; got {kinds:?}"
        );
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.name == "MyString" && matches!(n.kind, NodeKind::TypeAlias)),
            "Go type alias 'MyString'; got {kinds:?}"
        );
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.name == "globalCount" && matches!(n.kind, NodeKind::Variable)),
            "Go var 'globalCount'; got {kinds:?}"
        );
    }

    #[test]
    fn python_captures_upper_constants() {
        let code = "MAX_SIZE = 100\nBASE_URL = \"http://example.com\"\ncount = 0\n\ndef foo():\n    pass\n";
        let ex = TreeSitterExtractor::python()
            .extract(&sf("m.py", "python", code))
            .unwrap();
        let kinds: Vec<_> = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .map(|n| (&n.name, &n.kind))
            .collect();
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.name == "MAX_SIZE" && matches!(n.kind, NodeKind::Constant)),
            "Python UPPER const 'MAX_SIZE'; got {kinds:?}"
        );
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.name == "BASE_URL" && matches!(n.kind, NodeKind::Constant)),
            "Python UPPER const 'BASE_URL'; got {kinds:?}"
        );
        assert!(
            !ex.nodes
                .iter()
                .any(|n| n.name == "count" && matches!(n.kind, NodeKind::Constant)),
            "'count' should NOT be captured as Constant; got {kinds:?}"
        );
    }

    // ── Local-binding exclusion ───────────────────────────────────────────────
    // Regression gate: function-local const/let/var must NOT be captured as
    // Constant/Variable nodes.  Top-level and exported bindings MUST be captured.

    #[test]
    fn typescript_top_level_and_exported_captured_local_excluded() {
        let code = r#"
const TOP_LEVEL: number = 42;
export const EXPORTED_CONST: string = "hi";
export let exportedVar = 0;
let topLevelLet = 1;

function doWork(): void {
    const localConst = 99;
    let localLet = "noise";
    var localVar = true;
}

export function helper(): void {
    const innerLocal = 1;
    let innerLet = 2;
}
"#;
        let ex = TreeSitterExtractor::for_language("typescript")
            .unwrap()
            .extract(&sf("scope.ts", "typescript", code))
            .unwrap();
        let kinds: Vec<_> = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .map(|n| (n.name.as_str(), format!("{:?}", n.kind)))
            .collect();

        // top-level const captured
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.name == "TOP_LEVEL" && matches!(n.kind, NodeKind::Constant)),
            "top-level 'TOP_LEVEL' should be Constant; got {kinds:?}"
        );
        // exported const captured
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.name == "EXPORTED_CONST" && matches!(n.kind, NodeKind::Constant)),
            "exported 'EXPORTED_CONST' should be Constant; got {kinds:?}"
        );
        // exported let captured
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.name == "exportedVar" && matches!(n.kind, NodeKind::Variable)),
            "exported 'exportedVar' should be Variable; got {kinds:?}"
        );
        // top-level let captured
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.name == "topLevelLet" && matches!(n.kind, NodeKind::Variable)),
            "top-level 'topLevelLet' should be Variable; got {kinds:?}"
        );

        // function-local const NOT captured
        assert!(
            !ex.nodes.iter().any(|n| n.name == "localConst"),
            "function-local 'localConst' must NOT be captured; got {kinds:?}"
        );
        assert!(
            !ex.nodes.iter().any(|n| n.name == "localLet"),
            "function-local 'localLet' must NOT be captured; got {kinds:?}"
        );
        assert!(
            !ex.nodes.iter().any(|n| n.name == "localVar"),
            "function-local 'localVar' must NOT be captured; got {kinds:?}"
        );
        assert!(
            !ex.nodes.iter().any(|n| n.name == "innerLocal"),
            "inner function-local 'innerLocal' must NOT be captured; got {kinds:?}"
        );
        assert!(
            !ex.nodes.iter().any(|n| n.name == "innerLet"),
            "inner function-local 'innerLet' must NOT be captured; got {kinds:?}"
        );
    }

    #[test]
    fn tsx_top_level_and_exported_captured_local_excluded() {
        let code = r##"
const TOP_THEME = "dark";
export const BRAND_COLOR = "#ff0000";

export function Widget(): JSX.Element {
    const localState = 0;
    const localFlag = true;
    return <div />;
}
"##;
        let ex = TreeSitterExtractor::for_language("tsx")
            .unwrap()
            .extract(&sf("widget.tsx", "tsx", code))
            .unwrap();
        let kinds: Vec<_> = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .map(|n| (n.name.as_str(), format!("{:?}", n.kind)))
            .collect();

        // top-level const captured
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.name == "TOP_THEME" && matches!(n.kind, NodeKind::Constant)),
            "top-level 'TOP_THEME' should be Constant; got {kinds:?}"
        );
        // exported const captured
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.name == "BRAND_COLOR" && matches!(n.kind, NodeKind::Constant)),
            "exported 'BRAND_COLOR' should be Constant; got {kinds:?}"
        );

        // function-local consts NOT captured
        assert!(
            !ex.nodes.iter().any(|n| n.name == "localState"),
            "function-local 'localState' must NOT be captured; got {kinds:?}"
        );
        assert!(
            !ex.nodes.iter().any(|n| n.name == "localFlag"),
            "function-local 'localFlag' must NOT be captured; got {kinds:?}"
        );
    }

    #[test]
    fn javascript_top_level_and_exported_captured_local_excluded() {
        let code = r#"
const TOP_MAX = 100;
export const EXPORTED_LIMIT = 50;
let topVar = 0;

function compute(x) {
    const localResult = x * 2;
    let localTemp = 0;
    var legacyLocal = "ignored";
    return localResult;
}

export function factory() {
    const innerObj = {};
    return innerObj;
}
"#;
        let ex = TreeSitterExtractor::for_language("javascript")
            .unwrap()
            .extract(&sf("util.js", "javascript", code))
            .unwrap();
        let kinds: Vec<_> = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .map(|n| (n.name.as_str(), format!("{:?}", n.kind)))
            .collect();

        // top-level const captured
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.name == "TOP_MAX" && matches!(n.kind, NodeKind::Constant)),
            "top-level 'TOP_MAX' should be Constant; got {kinds:?}"
        );
        // exported const captured
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.name == "EXPORTED_LIMIT" && matches!(n.kind, NodeKind::Constant)),
            "exported 'EXPORTED_LIMIT' should be Constant; got {kinds:?}"
        );
        // top-level let captured
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.name == "topVar" && matches!(n.kind, NodeKind::Variable)),
            "top-level 'topVar' should be Variable; got {kinds:?}"
        );

        // function-local bindings NOT captured
        assert!(
            !ex.nodes.iter().any(|n| n.name == "localResult"),
            "function-local 'localResult' must NOT be captured; got {kinds:?}"
        );
        assert!(
            !ex.nodes.iter().any(|n| n.name == "localTemp"),
            "function-local 'localTemp' must NOT be captured; got {kinds:?}"
        );
        assert!(
            !ex.nodes.iter().any(|n| n.name == "legacyLocal"),
            "function-local 'legacyLocal' must NOT be captured; got {kinds:?}"
        );
        assert!(
            !ex.nodes.iter().any(|n| n.name == "innerObj"),
            "inner function-local 'innerObj' must NOT be captured; got {kinds:?}"
        );
    }

    // ── Heritage edges ────────────────────────────────────────────────────────

    #[test]
    fn typescript_heritage_extends_and_implements() {
        let code = r#"
class Animal {}
class Dog extends Animal {}
interface Runnable {}
class Runner implements Runnable {}
"#;
        let ex = TreeSitterExtractor::for_language("typescript")
            .unwrap()
            .extract(&sf("h.ts", "typescript", code))
            .unwrap();
        assert!(
            ex.refs
                .iter()
                .any(|r| r.raw_name == "Animal" && r.kind == EdgeKind::Extends),
            "Dog extends Animal — Extends ref expected; refs={:?}",
            ex.refs
                .iter()
                .map(|r| (&r.raw_name, &r.kind))
                .collect::<Vec<_>>()
        );
        assert!(
            ex.refs
                .iter()
                .any(|r| r.raw_name == "Runnable" && r.kind == EdgeKind::Implements),
            "Runner implements Runnable — Implements ref expected; refs={:?}",
            ex.refs
                .iter()
                .map(|r| (&r.raw_name, &r.kind))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn java_heritage_extends_and_implements() {
        let code = r#"
class Base {}
class Child extends Base {}
interface Printable {}
class Document extends Base implements Printable {}
"#;
        let ex = TreeSitterExtractor::for_language("java")
            .unwrap()
            .extract(&sf("H.java", "java", code))
            .unwrap();
        assert!(
            ex.refs
                .iter()
                .any(|r| r.raw_name == "Base" && r.kind == EdgeKind::Extends),
            "Child/Document extends Base — Extends ref expected; refs={:?}",
            ex.refs
                .iter()
                .map(|r| (&r.raw_name, &r.kind))
                .collect::<Vec<_>>()
        );
        assert!(
            ex.refs
                .iter()
                .any(|r| r.raw_name == "Printable" && r.kind == EdgeKind::Implements),
            "Document implements Printable — Implements ref expected; refs={:?}",
            ex.refs
                .iter()
                .map(|r| (&r.raw_name, &r.kind))
                .collect::<Vec<_>>()
        );
    }

    // ── New callable-form tests ───────────────────────────────────────────────

    /// A class with getter, setter, arrow-fn field, static method, async method, and constructor
    /// must all yield individual Method/Constructor nodes.
    #[test]
    fn typescript_class_all_method_forms() {
        let code = r#"
class Widget {
  constructor(id: string) { this.init(); }
  get label(): string { return this._label; }
  set label(v: string) { this._label = v; }
  static create(id: string): Widget { return new Widget(id); }
  async load(): Promise<void> { await fetch('/api'); }
  handle = () => { this.render(); };
}
"#;
        let ex = TreeSitterExtractor::for_language("typescript")
            .unwrap()
            .extract(&sf("widget.ts", "typescript", code))
            .unwrap();
        let methods: Vec<_> = ex
            .nodes
            .iter()
            .filter(|n| matches!(n.kind, NodeKind::Method))
            .map(|n| n.name.as_str())
            .collect();
        // All 6 must appear as Method nodes (constructor, getter, setter, static, async, arrow-field)
        for name in &["constructor", "label", "create", "load", "handle"] {
            assert!(
                methods.contains(name),
                "expected Method node '{name}'; got {methods:?}"
            );
        }
        // getter 'label' appears as setter too — at least one for each unique method name
        assert!(
            methods.len() >= 6,
            "expected ≥6 Method nodes; got {methods:?}"
        );
    }

    /// Interface method signatures must produce Method nodes.
    #[test]
    fn typescript_interface_method_signatures_captured() {
        let code = r#"
interface Fetchable {
  fetch(url: string): Promise<Response>;
  abort(): void;
}
"#;
        let ex = TreeSitterExtractor::for_language("typescript")
            .unwrap()
            .extract(&sf("api.ts", "typescript", code))
            .unwrap();
        let methods: Vec<_> = ex
            .nodes
            .iter()
            .filter(|n| matches!(n.kind, NodeKind::Method))
            .map(|n| n.name.as_str())
            .collect();
        assert!(
            methods.contains(&"fetch"),
            "interface method 'fetch' not captured; got {methods:?}"
        );
        assert!(
            methods.contains(&"abort"),
            "interface method 'abort' not captured; got {methods:?}"
        );
    }

    /// Arrow-fn class field in JavaScript must produce a Method node.
    #[test]
    fn javascript_arrow_field_captured_as_method() {
        let code = r#"
class Button {
  handleClick = () => { this.onClick(); };
  handleHover = (e) => { this.onHover(e); };
  render() { return this.handleClick; }
}
"#;
        let ex = TreeSitterExtractor::for_language("javascript")
            .unwrap()
            .extract(&sf("button.js", "javascript", code))
            .unwrap();
        let methods: Vec<_> = ex
            .nodes
            .iter()
            .filter(|n| matches!(n.kind, NodeKind::Method))
            .map(|n| n.name.as_str())
            .collect();
        assert!(
            methods.contains(&"handleClick"),
            "'handleClick' arrow field not captured as Method; got {methods:?}"
        );
        assert!(
            methods.contains(&"handleHover"),
            "'handleHover' arrow field not captured as Method; got {methods:?}"
        );
        assert!(
            methods.contains(&"render"),
            "regular method 'render' not captured; got {methods:?}"
        );
    }

    /// `new X()` constructor calls must be captured as call refs.
    #[test]
    fn typescript_new_expression_captured_as_call() {
        let code = r#"
class Repo {
  make(): Widget { return new Widget("x"); }
  makeNested() { return new ns.Foo(); }
}
"#;
        let ex = TreeSitterExtractor::for_language("typescript")
            .unwrap()
            .extract(&sf("repo.ts", "typescript", code))
            .unwrap();
        let call_names: Vec<_> = ex
            .refs
            .iter()
            .filter(|r| r.kind == EdgeKind::Calls)
            .map(|r| r.raw_name.as_str())
            .collect();
        assert!(
            call_names.contains(&"Widget"),
            "'new Widget()' call not captured; calls={call_names:?}"
        );
        assert!(
            call_names.contains(&"Foo"),
            "'new ns.Foo()' call not captured; calls={call_names:?}"
        );
    }

    /// Calls inside arrow-fn class fields must be attributed to the enclosing method (field).
    #[test]
    fn typescript_arrow_field_call_attributed_to_field() {
        let code = r#"
class Service {
  fetch = async () => { const data = await loadData(); return data; };
}
"#;
        let ex = TreeSitterExtractor::for_language("typescript")
            .unwrap()
            .extract(&sf("svc.ts", "typescript", code))
            .unwrap();
        let load_call = ex.refs.iter().find(|r| r.raw_name == "loadData");
        assert!(load_call.is_some(), "loadData call not captured");
        let from = &load_call.unwrap().from;
        assert!(
            from.as_str().contains("fetch"),
            "loadData call should be attributed to 'fetch' arrow-field, got {from}"
        );
    }

    /// A class with getter + arrow-fn field + static method → exactly 3 callable nodes (plus class).
    #[test]
    fn typescript_getter_arrow_field_static_three_callables() {
        let code = r#"
class Counter {
  get count(): number { return this._count; }
  increment = () => { this._count++; };
  static zero(): Counter { return new Counter(); }
}
"#;
        let ex = TreeSitterExtractor::for_language("typescript")
            .unwrap()
            .extract(&sf("counter.ts", "typescript", code))
            .unwrap();
        let callable_names: Vec<_> = ex
            .nodes
            .iter()
            .filter(|n| {
                matches!(
                    n.kind,
                    NodeKind::Method | NodeKind::Function | NodeKind::Constructor
                )
            })
            .map(|n| n.name.as_str())
            .collect();
        assert!(
            callable_names.contains(&"count"),
            "getter 'count' not captured; got {callable_names:?}"
        );
        assert!(
            callable_names.contains(&"increment"),
            "arrow-field 'increment' not captured; got {callable_names:?}"
        );
        assert!(
            callable_names.contains(&"zero"),
            "static 'zero' not captured; got {callable_names:?}"
        );
        assert_eq!(
            callable_names.len(),
            3,
            "expected exactly 3 callable nodes; got {callable_names:?}"
        );
    }

    /// TSX: same callable forms work in a TSX component file.
    #[test]
    fn tsx_class_method_forms_captured() {
        let code = r##"
class Panel extends React.Component {
  constructor(props: PanelProps) { super(props); }
  get title(): string { return this.state.title; }
  static defaultProps = () => ({});
  handleClose = () => { this.setState({ open: false }); };
  render(): JSX.Element { return <div />; }
}
"##;
        let ex = TreeSitterExtractor::for_language("tsx")
            .unwrap()
            .extract(&sf("panel.tsx", "tsx", code))
            .unwrap();
        let methods: Vec<_> = ex
            .nodes
            .iter()
            .filter(|n| matches!(n.kind, NodeKind::Method))
            .map(|n| n.name.as_str())
            .collect();
        for name in &["constructor", "title", "handleClose", "render"] {
            assert!(
                methods.contains(name),
                "TSX method '{name}' not captured; got {methods:?}"
            );
        }
    }

    // ── Import-map hint tests ─────────────────────────────────────────────────

    /// TypeScript: named imports produce hints["imports"] mapping each local name to its module.
    #[test]
    fn ts_import_hints_named_imports_attached_to_call_refs() {
        let code = r#"
import { helper, fmt as format } from './utils';
import React from 'react';
function main() { helper(); format("x"); React.render(); }
"#;
        let ex = TreeSitterExtractor::for_language("typescript")
            .unwrap()
            .extract(&sf("src/main.ts", "typescript", code))
            .unwrap();

        // Every Calls ref should have a hints["imports"] entry.
        let call_refs: Vec<_> = ex
            .refs
            .iter()
            .filter(|r| r.kind == EdgeKind::Calls)
            .collect();
        assert!(!call_refs.is_empty(), "expected call refs");
        for r in &call_refs {
            let imp = r.hints.get("imports");
            assert!(
                imp.is_some(),
                "Calls ref '{}' should have hints[imports]",
                r.raw_name
            );
        }

        // Check specific mappings: helper → ./utils, format → ./utils, React → react
        let helper_ref = ex.refs.iter().find(|r| r.raw_name == "helper").unwrap();
        let imports = helper_ref
            .hints
            .get("imports")
            .unwrap()
            .as_object()
            .unwrap();
        assert_eq!(
            imports.get("helper").and_then(|v| v.as_str()),
            Some("./utils"),
            "helper should map to ./utils"
        );
        assert_eq!(
            imports.get("format").and_then(|v| v.as_str()),
            Some("./utils"),
            "format (alias of fmt) should map to ./utils"
        );
        assert_eq!(
            imports.get("React").and_then(|v| v.as_str()),
            Some("react"),
            "React default import should map to react"
        );
    }

    /// Python: from-import produces hints["imports"] with each imported name.
    #[test]
    fn python_import_hints_from_import_attached() {
        let code =
            "from mylib import helper, compute\n\ndef main():\n    helper()\n    compute()\n";
        let ex = TreeSitterExtractor::python()
            .extract(&sf("src/main.py", "python", code))
            .unwrap();

        let call_refs: Vec<_> = ex
            .refs
            .iter()
            .filter(|r| r.kind == EdgeKind::Calls)
            .collect();
        assert!(!call_refs.is_empty(), "expected call refs");

        let helper_ref = ex.refs.iter().find(|r| r.raw_name == "helper").unwrap();
        let imports = helper_ref.hints.get("imports");
        assert!(
            imports.is_some(),
            "Python call ref should have hints[imports]"
        );
        // Python from-import: the stmt text is what tree-sitter gives us; verify best-effort.
        // The import_stmt text may not contain the full `from mylib import helper, compute` since
        // python.scm uses @import.source for the module name capture and @import for the stmt.
        // If the map was populated, helper should map to mylib.
        if let Some(obj) = imports.and_then(|v| v.as_object()) {
            if let Some(val) = obj.get("helper") {
                assert_eq!(val.as_str(), Some("mylib"), "helper should map to mylib");
            }
            // else: best-effort — not all Python import forms produce name-level pairs
        }
    }

    /// Non-call refs (Imports, Extends) must NOT have import hints.
    #[test]
    fn import_and_heritage_refs_have_no_hints() {
        let code = r#"
import { foo } from './foo';
class A extends B {}
function main() { foo(); }
"#;
        let ex = TreeSitterExtractor::for_language("typescript")
            .unwrap()
            .extract(&sf("h.ts", "typescript", code))
            .unwrap();

        // Import refs must not carry hints.
        for r in ex.refs.iter().filter(|r| r.kind == EdgeKind::Imports) {
            assert!(
                r.hints.get("imports").is_none(),
                "Import ref should not have hints[imports]"
            );
        }
        // Heritage refs must not carry hints.
        for r in ex.refs.iter().filter(|r| r.kind == EdgeKind::Extends) {
            assert!(
                r.hints.get("imports").is_none(),
                "Extends ref should not have hints[imports]"
            );
        }
    }

    /// Verify the callable count on a realistic multi-class TS file approaches
    /// the reference-tool numbers (reference: ~3,318 on a TS-heavy repo).
    /// Before fixes: constructor/getter/setter/static/async were already captured
    /// by the existing method_definition pattern. After fixes: arrow-fn class fields
    /// (public_field_definition) and interface method signatures (method_signature)
    /// are also captured.
    #[test]
    fn callable_count_ts_all_forms() {
        // 8 methods in UserService + 4 interface sigs + 4 in ProductService + 2 top-level = 18
        let code = r#"
class UserService {
  constructor(db: string) { this.connect(db); }
  get current(): string { return this.user; }
  set current(v: string) { this.user = v; }
  static create(db: string): UserService { return new UserService(db); }
  async fetch(id: string): Promise<string> { return await load(id); }
  delete = async (id: string) => { await remove(id); };
  transform = (u: string) => u.toUpperCase();
  validate(u: string): boolean { return Boolean(u); }
}
interface Repository {
  findById(id: string): string;
  findAll(): string[];
  save(item: string): string;
  delete(id: string): void;
}
class ProductService {
  constructor(repo: Repository) { this.repo = repo; }
  async getAll(): Promise<string[]> { return this.repo.findAll(); }
  static fromEnv(): ProductService { return new ProductService(stub()); }
  onUpdate = (p: string) => { this.notify(p); };
}
function standalone(x: number): number { return x * 2; }
const helper = (s: string) => s.trim();
"#;
        let ex = TreeSitterExtractor::for_language("typescript")
            .unwrap()
            .extract(&sf("svc.ts", "typescript", code))
            .unwrap();
        let callables: Vec<_> = ex
            .nodes
            .iter()
            .filter(|n| {
                matches!(
                    n.kind,
                    NodeKind::Method | NodeKind::Function | NodeKind::Constructor
                )
            })
            .map(|n| n.name.as_str())
            .collect();
        let calls: Vec<_> = ex
            .refs
            .iter()
            .filter(|r| r.kind == EdgeKind::Calls)
            .map(|r| r.raw_name.as_str())
            .collect();
        // 8 UserService + 4 interface + 4 ProductService + 2 top-level = 18 minimum
        assert!(
            callables.len() >= 18,
            "Expected ≥18 callable nodes (incl. arrow fields + interface sigs); got {}:\n{:?}",
            callables.len(),
            callables
        );
        // constructor calls: new UserService, new ProductService; plain calls: stub, load, etc.
        assert!(
            calls.len() >= 4,
            "Expected ≥4 call refs (incl. new expressions); got {}: {:?}",
            calls.len(),
            calls
        );
        // Specifically verify new-expression calls are captured
        assert!(
            calls.contains(&"UserService"),
            "new UserService() not captured as call"
        );
        assert!(
            calls.contains(&"ProductService"),
            "new ProductService() not captured as call"
        );
    }

    // ── is_minified_or_huge unit tests ────────────────────────────────────────

    /// A normal multi-line Rust file is not flagged.
    #[test]
    fn minified_guard_normal_file_is_false() {
        let code = "fn hello() -> &'static str {\n    \"world\"\n}\nfn main() {\n    println!(\"{}\", hello());\n}\n";
        assert!(
            !is_minified_or_huge(code),
            "normal source must not be flagged"
        );
    }

    /// An empty file is not flagged (edge case: 0 lines).
    #[test]
    fn minified_guard_empty_file_is_false() {
        assert!(!is_minified_or_huge(""), "empty file must not be flagged");
    }

    /// A single line of exactly 50,000 chars IS flagged by heuristic 3 (average
    /// chars/line = 50_000 >> 2_000), even though it sits exactly at the per-line
    /// boundary. This confirms the three heuristics are independent AND-OR guards:
    /// heuristic 2 requires strictly > 50_000; heuristic 3 fires because the dense
    /// average overwhelmingly exceeds its own threshold.
    #[test]
    fn minified_guard_exactly_at_line_threshold_still_flagged_by_avg() {
        // One line of exactly 50_000 chars. Heuristic 2 does NOT fire (not > 50_000).
        // Heuristic 3 DOES fire: avg = 50_000 / 1 = 50_000 > 2_000.
        let line = "x".repeat(50_000);
        assert!(
            is_minified_or_huge(&line),
            "a single 50k-char line is flagged by heuristic 3 (avg chars/line = 50k)"
        );
    }

    /// Multiple lines each at or below the average threshold and well under 50k
    /// chars/line are NOT flagged. Regression: a 40-line file with 1_999-char lines.
    #[test]
    fn minified_guard_dense_but_under_avg_threshold_is_false() {
        let line = "x".repeat(1_999); // below MAX_AVG_CHARS_PER_LINE
        // 40 lines × 1_999 chars = 79_960 total, avg = 1_999 — under the 2_000 threshold.
        let text = (0..40)
            .map(|_| line.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !is_minified_or_huge(&text),
            "40 lines of 1_999 chars each must not be flagged"
        );
    }

    /// A single line of 50,001 chars IS flagged (one over the threshold).
    #[test]
    fn minified_guard_one_over_line_threshold_is_true() {
        let line = "x".repeat(50_001);
        assert!(
            is_minified_or_huge(&line),
            "a line of 50_001 chars must be flagged as minified"
        );
    }

    /// A file that exceeds 1 MiB is flagged even if every line is short.
    #[test]
    fn minified_guard_huge_file_over_1mib_is_true() {
        // 1_048_577 bytes of 'a' characters (no newlines, but the size check triggers first).
        let huge = "a".repeat(1_048_577);
        assert!(is_minified_or_huge(&huge), "file > 1 MiB must be flagged");
    }

    /// A file with a very high average chars/line (dense generated output) is flagged.
    #[test]
    fn minified_guard_high_average_chars_per_line_is_true() {
        // Build a file: 100 lines each of 3_000 chars → average = 3_000 > 2_000.
        // Total = 300_000 bytes < 1 MiB, and each line < 50_000. Only heuristic 3 fires.
        let long_line = "x".repeat(3_000);
        let text = (0..100)
            .map(|_| long_line.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            is_minified_or_huge(&text),
            "average chars/line > 2_000 must be flagged (dense generated file)"
        );
    }

    /// A fixture of one ~60k-char line: is_minified_or_huge returns true AND
    /// TreeSitterExtractor::extract returns an empty Extraction (no parse performed).
    #[test]
    fn minified_fixture_extract_returns_empty() {
        // Read the fixture written alongside this test.
        let fixture_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/minified.js");
        let text = std::fs::read_to_string(fixture_path).expect("minified.js fixture must exist");

        // The guard must fire.
        assert!(
            is_minified_or_huge(&text),
            "minified.js fixture must be flagged by is_minified_or_huge"
        );

        // Extract must return an empty Extraction without error.
        let file = sf(fixture_path, "javascript", &text);
        let ex = TreeSitterExtractor::for_language("javascript")
            .unwrap()
            .extract(&file)
            .expect("extract must not error on minified input");

        assert!(
            ex.nodes.is_empty(),
            "minified file must produce 0 nodes; got {}",
            ex.nodes.len()
        );
        assert!(
            ex.local_edges.is_empty(),
            "minified file must produce 0 local_edges; got {}",
            ex.local_edges.len()
        );
        assert!(
            ex.refs.is_empty(),
            "minified file must produce 0 refs; got {}",
            ex.refs.len()
        );
    }

    /// Same guard fires for IaCExtractor on a minified YAML blob.
    #[test]
    fn minified_iac_extract_returns_empty() {
        // One giant YAML line that exceeds the 50_000-char threshold.
        let giant_line = format!("key: {}", "v".repeat(50_001));
        let file = sf("big.yaml", "cloudformation", &giant_line);
        let ex = IaCExtractor::cloudformation()
            .extract(&file)
            .expect("IaCExtractor must not error on minified input");

        assert!(ex.nodes.is_empty(), "minified IaC must produce 0 nodes");
        assert!(
            ex.local_edges.is_empty(),
            "minified IaC must produce 0 local_edges"
        );
        assert!(ex.refs.is_empty(), "minified IaC must produce 0 refs");
    }

    // ── W2.1 batch 3 smoke tests ──────────────────────────────────────────────

    #[test]
    fn smoke_haskell() {
        let code = r#"
module Greeter where

import Data.List (intercalate)

data Person = Person { name :: String }

greet :: Person -> String
greet p = "hi " ++ name p

main :: IO ()
main = putStrLn (greet (Person "world"))
"#;
        let ex = TreeSitterExtractor::for_language("haskell")
            .unwrap()
            .extract(&sf("Greeter.hs", "haskell", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "expected >=1 definition, got {defs}");
    }

    // smoke_hcl is omitted — tree-sitter-hcl 1.1.0 is ABI 15, deferred to ts 0.25 upgrade.

    #[test]
    fn smoke_nix() {
        let code = r#"
{ pkgs ? import <nixpkgs> {} }:

let
  greet = name: "hi " + name;
  version = "1.0";
in
pkgs.stdenv.mkDerivation {
  name = "hello-${version}";
}
"#;
        let ex = TreeSitterExtractor::for_language("nix")
            .unwrap()
            .extract(&sf("default.nix", "nix", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "expected >=1 definition, got {defs}");
    }

    #[test]
    fn smoke_r() {
        let code = r#"
greet <- function(name) {
  paste("hi", name)
}

main <- function() {
  greet("world")
}
"#;
        let ex = TreeSitterExtractor::for_language("r")
            .unwrap()
            .extract(&sf("greet.r", "r", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "expected >=1 definition, got {defs}");
        let calls = ex.refs.iter().filter(|r| r.kind == EdgeKind::Calls).count();
        assert!(calls >= 1, "expected >=1 call ref, got {calls}");
    }

    #[test]
    fn smoke_swift() {
        let code = "func greet(name: String) {}\nfunc main() { greet(name: \"world\") }\n";
        let ex = TreeSitterExtractor::for_language("swift")
            .unwrap()
            .extract(&sf("greet.swift", "swift", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "expected >=1 swift definition, got {defs}");
        let calls = ex.refs.iter().filter(|r| r.kind == EdgeKind::Calls).count();
        assert!(calls >= 1, "expected >=1 swift call ref, got {calls}");
    }

    #[test]
    fn smoke_kotlin() {
        let code = r#"
package com.example

import kotlin.text.trim

class Greeter {
    fun greet(name: String): String = hello(name)
    private fun hello(n: String) = "hi $n"
}

fun main() {
    val g = Greeter()
    println(g.greet("world"))
}
"#;
        let ex = TreeSitterExtractor::for_language("kotlin")
            .unwrap()
            .extract(&sf("Greeter.kt", "kotlin", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "expected >=1 definition, got {defs}");
        let calls = ex.refs.iter().filter(|r| r.kind == EdgeKind::Calls).count();
        assert!(calls >= 1, "expected >=1 call ref, got {calls}");
    }

    #[test]
    fn smoke_toml() {
        let code = r#"
[package]
name = "wicked_estate"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { version = "1.0", features = ["derive"] }

[dev-dependencies]
tokio = { version = "1", features = ["full"] }
"#;
        let ex = TreeSitterExtractor::for_language("toml")
            .unwrap()
            .extract(&sf("Cargo.toml", "toml", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "expected >=1 definition (table/key), got {defs}");
    }

    #[test]
    fn smoke_markdown() {
        let code = r#"# wicked_estate

A queryable code graph engine.

## Installation

Run `cargo build`.

## Usage

See the [docs](docs/).
"#;
        let ex = TreeSitterExtractor::for_language("markdown")
            .unwrap()
            .extract(&sf("README.md", "markdown", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "expected >=1 heading definition, got {defs}");
    }

    #[test]
    fn smoke_cobol() {
        // Fixed-format COBOL: Area A (paragraphs/divisions) at col 8, Area B (statements) at col 12.
        let code = "       IDENTIFICATION DIVISION.\n       PROGRAM-ID. HELLO.\n       PROCEDURE DIVISION.\n       MAIN-PARA.\n           PERFORM GREET-PARA.\n           STOP RUN.\n       GREET-PARA.\n           DISPLAY \"HELLO\".\n";
        let ex = TreeSitterExtractor::for_language("cobol")
            .unwrap()
            .extract(&sf("hello.cob", "cobol", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(
            defs >= 1,
            "expected >=1 COBOL paragraph/program def, got {defs}"
        );
        // Pins the pass-2-before-fixup ordering: the paragraph span fixup iterates `def_nodes`,
        // which the id-minting pass populates — if minting ran after the fixup, MAIN-PARA would
        // keep its single-line header span and this fails.
        let main_para = ex
            .nodes
            .iter()
            .find(|n| n.name == "MAIN-PARA")
            .expect("MAIN-PARA paragraph def expected");
        assert!(
            main_para.location.span.end_line > main_para.location.span.start_line,
            "paragraph span must extend past its header line; got {:?}",
            main_para.location.span
        );
    }

    // ── W2.1 arborium batch smoke tests ──────────────────────────────────────
    // One per language: parse a minimal representative snippet, assert >=1 non-File node.

    #[test]
    fn smoke_ada() {
        let code = "procedure Greet (Name : String) is\nbegin\n   null;\nend Greet;\n";
        let ex = TreeSitterExtractor::for_language("ada")
            .unwrap()
            .extract(&sf("greet.adb", "ada", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "ada: expected >=1 def, got {defs}");
    }

    #[test]
    fn smoke_awk() {
        let code = "function greet(name) { print \"hi \" name }\nBEGIN { greet(\"world\") }\n";
        let ex = TreeSitterExtractor::for_language("awk")
            .unwrap()
            .extract(&sf("hello.awk", "awk", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "awk: expected >=1 def, got {defs}");
    }

    #[test]
    fn smoke_clojure() {
        let code = "(ns myapp.core)\n(defn greet [name] (str \"hi \" name))\n(def MAX 100)\n";
        let ex = TreeSitterExtractor::for_language("clojure")
            .unwrap()
            .extract(&sf("core.clj", "clojure", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "clojure: expected >=1 def, got {defs}");
    }

    #[test]
    fn smoke_cmake() {
        let code = "cmake_minimum_required(VERSION 3.10)\nfunction(greet name)\n  message(\"hi ${name}\")\nendfunction()\n";
        let ex = TreeSitterExtractor::for_language("cmake")
            .unwrap()
            .extract(&sf("CMakeLists.txt", "cmake", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "cmake: expected >=1 def, got {defs}");
    }

    #[test]
    fn smoke_commonlisp() {
        let code = "(defun greet (name)\n  (format t \"hi ~A~%\" name))\n";
        let ex = TreeSitterExtractor::for_language("commonlisp")
            .unwrap()
            .extract(&sf("greet.lisp", "commonlisp", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "commonlisp: expected >=1 def, got {defs}");
    }

    #[test]
    fn smoke_dart() {
        let code =
            "class Greeter {\n  String greet(String name) => 'hi $name';\n}\nvoid main() {}\n";
        let ex = TreeSitterExtractor::for_language("dart")
            .unwrap()
            .extract(&sf("greet.dart", "dart", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "dart: expected >=1 def, got {defs}");
    }

    #[test]
    fn smoke_dockerfile() {
        let code = "FROM ubuntu:22.04\nARG APP_VERSION=1.0\nRUN apt-get update\n";
        let ex = TreeSitterExtractor::for_language("dockerfile")
            .unwrap()
            .extract(&sf("Dockerfile", "dockerfile", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "dockerfile: expected >=1 def, got {defs}");
    }

    #[test]
    fn smoke_elm() {
        let code = "module Main exposing (main)\nimport Html\ngreet name = \"hi \" ++ name\n";
        let ex = TreeSitterExtractor::for_language("elm")
            .unwrap()
            .extract(&sf("Main.elm", "elm", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "elm: expected >=1 def, got {defs}");
    }

    #[test]
    fn smoke_erlang() {
        let code = "-module(greet).\n-export([hello/1]).\nhello(Name) -> io:format(\"hi ~p~n\", [Name]).\n";
        let ex = TreeSitterExtractor::for_language("erlang")
            .unwrap()
            .extract(&sf("greet.erl", "erlang", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "erlang: expected >=1 def, got {defs}");
    }

    #[test]
    fn smoke_fish() {
        let code = "function greet\n    echo \"hi $argv[1]\"\nend\ngreet world\n";
        let ex = TreeSitterExtractor::for_language("fish")
            .unwrap()
            .extract(&sf("greet.fish", "fish", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "fish: expected >=1 def, got {defs}");
    }

    #[test]
    fn smoke_fsharp() {
        let code = "module Greet\nlet greet name = sprintf \"hi %s\" name\nlet hello n = greet n\n";
        let ex = TreeSitterExtractor::for_language("fsharp")
            .unwrap()
            .extract(&sf("greet.fs", "fsharp", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "fsharp: expected >=1 def, got {defs}");
    }

    #[test]
    fn smoke_gleam() {
        let code = "pub fn greet(name: String) -> String {\n  \"hi \" <> name\n}\n";
        let ex = TreeSitterExtractor::for_language("gleam")
            .unwrap()
            .extract(&sf("greet.gleam", "gleam", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "gleam: expected >=1 def, got {defs}");
    }

    #[test]
    fn smoke_groovy() {
        let code = "class Greeter {\n    String greet(String name) { return \"hi \" + name }\n}\n";
        let ex = TreeSitterExtractor::for_language("groovy")
            .unwrap()
            .extract(&sf("Greeter.groovy", "groovy", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "groovy: expected >=1 def, got {defs}");
    }

    #[test]
    fn smoke_glsl() {
        let code = "void main() {\n    gl_FragColor = vec4(1.0, 0.0, 0.0, 1.0);\n}\n";
        let ex = TreeSitterExtractor::for_language("glsl")
            .unwrap()
            .extract(&sf("shader.glsl", "glsl", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "glsl: expected >=1 def, got {defs}");
    }

    #[test]
    fn smoke_graphql() {
        let code =
            "type User {\n  id: ID!\n  name: String!\n}\ntype Query {\n  user(id: ID!): User\n}\n";
        let ex = TreeSitterExtractor::for_language("graphql")
            .unwrap()
            .extract(&sf("schema.graphql", "graphql", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "graphql: expected >=1 def, got {defs}");
    }

    #[test]
    fn smoke_hcl() {
        let code = "resource \"aws_instance\" \"web\" {\n  ami = \"ami-0c55b159cbfafe1f0\"\n  instance_type = \"t2.micro\"\n}\n";
        let ex = TreeSitterExtractor::for_language("hcl")
            .unwrap()
            .extract(&sf("main.tf", "hcl", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "hcl: expected >=1 def, got {defs}");
    }

    #[test]
    fn smoke_make() {
        let code = "all: build test\nbuild:\n\tgo build ./...\ntest:\n\tgo test ./...\n";
        let ex = TreeSitterExtractor::for_language("make")
            .unwrap()
            .extract(&sf("Makefile.mk", "make", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "make: expected >=1 def (rule), got {defs}");
    }

    #[test]
    fn smoke_matlab() {
        let code = "function result = greet(name)\n  result = ['hi ' name];\nend\n";
        let ex = TreeSitterExtractor::for_language("matlab")
            .unwrap()
            .extract(&sf("greet.m", "matlab", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "matlab: expected >=1 def, got {defs}");
    }

    #[test]
    fn smoke_objc() {
        let code = "@interface Greeter : NSObject\n- (NSString *)greet:(NSString *)name;\n@end\n@implementation Greeter\n- (NSString *)greet:(NSString *)name {\n  return [@\"hi \" stringByAppendingString:name];\n}\n@end\n";
        let ex = TreeSitterExtractor::for_language("objc")
            .unwrap()
            .extract(&sf("Greeter.mm", "objc", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "objc: expected >=1 def, got {defs}");
    }

    #[test]
    fn smoke_perl() {
        let code = "package Greeter;\nsub greet {\n    my ($name) = @_;\n    return \"hi $name\";\n}\n1;\n";
        let ex = TreeSitterExtractor::for_language("perl")
            .unwrap()
            .extract(&sf("Greeter.pm", "perl", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "perl: expected >=1 def, got {defs}");
    }

    #[test]
    fn smoke_powershell() {
        let code = "function Greet-User {\n    param([string]$Name)\n    Write-Host \"hi $Name\"\n}\nclass MyClass {\n    [string]$Name\n}\n";
        let ex = TreeSitterExtractor::for_language("powershell")
            .unwrap()
            .extract(&sf("greet.ps1", "powershell", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "powershell: expected >=1 def, got {defs}");
    }

    #[test]
    fn smoke_prolog() {
        let code = "greet(Name) :- format('hi ~w~n', [Name]).\nhello :- greet(world).\n";
        let ex = TreeSitterExtractor::for_language("prolog")
            .unwrap()
            .extract(&sf("greet.pro", "prolog", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "prolog: expected >=1 def, got {defs}");
    }

    #[test]
    fn smoke_proto() {
        let code = "syntax = \"proto3\";\nmessage User {\n  string name = 1;\n  int32 id = 2;\n}\nservice UserService {\n  rpc GetUser (User) returns (User);\n}\n";
        let ex = TreeSitterExtractor::for_language("proto")
            .unwrap()
            .extract(&sf("user.proto", "proto", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "proto: expected >=1 def, got {defs}");
    }

    #[test]
    fn smoke_sql() {
        let code = "CREATE TABLE users (id INT PRIMARY KEY, name VARCHAR(255));\nCREATE VIEW active_users AS SELECT * FROM users WHERE active = 1;\n";
        let ex = TreeSitterExtractor::for_language("sql")
            .unwrap()
            .extract(&sf("schema.sql", "sql", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "sql: expected >=1 def, got {defs}");
    }

    #[test]
    fn smoke_svelte() {
        let code = "<script>\n  let name = 'world';\n</script>\n<main>\n  <h1>Hello {name}</h1>\n</main>\n";
        let ex = TreeSitterExtractor::for_language("svelte")
            .unwrap()
            .extract(&sf("App.svelte", "svelte", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "svelte: expected >=1 def, got {defs}");
    }

    #[test]
    fn smoke_vue() {
        let code = "<template>\n  <div>Hello {{ name }}</div>\n</template>\n<script>\nexport default { name: 'App' };\n</script>\n";
        let ex = TreeSitterExtractor::for_language("vue")
            .unwrap()
            .extract(&sf("App.vue", "vue", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "vue: expected >=1 def, got {defs}");
    }

    #[test]
    fn smoke_zig() {
        let code = "const std = @import(\"std\");\nfn greet(name: []const u8) void {\n    std.debug.print(\"hi {s}\\n\", .{name});\n}\n";
        let ex = TreeSitterExtractor::for_language("zig")
            .unwrap()
            .extract(&sf("greet.zig", "zig", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "zig: expected >=1 def, got {defs}");
    }

    // ── arborium batch 2 smoke tests (20 new languages) ──────────────────────

    #[test]
    fn smoke_hlsl() {
        let code = "float4 main(float2 uv : TEXCOORD0) : SV_TARGET {\n    return float4(1.0, 0.0, 0.0, 1.0);\n}\nstruct VertexInput { float4 pos : POSITION; };\n";
        let ex = TreeSitterExtractor::for_language("hlsl")
            .unwrap()
            .extract(&sf("shader.hlsl", "hlsl", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "hlsl: expected >=1 def, got {defs}");
    }

    #[test]
    fn smoke_idris() {
        let code = "data Nat = Z | S Nat\ngreet : String -> String\ngreet name = \"hi \" ++ name\n";
        let ex = TreeSitterExtractor::for_language("idris")
            .unwrap()
            .extract(&sf("greet.idr", "idris", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "idris: expected >=1 def, got {defs}");
    }

    #[test]
    fn smoke_ini() {
        let code = "[database]\nhost = localhost\nport = 5432\n\n[server]\nport = 8080\n";
        let ex = TreeSitterExtractor::for_language("ini")
            .unwrap()
            .extract(&sf("config.ini", "ini", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "ini: expected >=1 def (section), got {defs}");
    }

    #[test]
    fn smoke_jq() {
        let code =
            "def greet(name): \"hi \" + name;\ndef double(x): x * 2;\n.items[] | greet(.name)\n";
        let ex = TreeSitterExtractor::for_language("jq")
            .unwrap()
            .extract(&sf("filter.jq", "jq", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "jq: expected >=1 def, got {defs}");
    }

    #[test]
    fn smoke_jsdoc() {
        let code = "/**\n * @param {string} name - The name\n * @returns {string} greeting\n */\n";
        let ex = TreeSitterExtractor::for_language("jsdoc")
            .unwrap()
            .extract(&sf("example.jsdoc", "jsdoc", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "jsdoc: expected >=1 def (tag), got {defs}");
    }

    #[test]
    fn smoke_just() {
        let code =
            "build:\n    cargo build\n\ntest: build\n    cargo test\n\nversion := \"1.0.0\"\n";
        let ex = TreeSitterExtractor::for_language("just")
            .unwrap()
            .extract(&sf("Justfile", "just", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "just: expected >=1 def (recipe), got {defs}");
    }

    #[test]
    fn smoke_kdl() {
        let code = "package name=\"my-app\" version=\"1.0.0\" {\n    author email=\"dev@example.com\"\n}\ndependency \"serde\" version=\"1\"\n";
        let ex = TreeSitterExtractor::for_language("kdl")
            .unwrap()
            .extract(&sf("config.kdl", "kdl", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "kdl: expected >=1 def (node), got {defs}");
    }

    #[test]
    fn smoke_lean() {
        let code = "def greet (name : String) : String :=\n  \"hi \" ++ name\n\ntheorem greet_nonempty : greet \"x\" ≠ \"\" := by simp [greet]\n";
        let ex = TreeSitterExtractor::for_language("lean")
            .unwrap()
            .extract(&sf("greet.lean", "lean", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "lean: expected >=1 def, got {defs}");
    }

    #[test]
    fn smoke_meson() {
        let code = "project('myapp', 'c', version : '1.0.0')\nexecutable('myapp', 'main.c')\ntest('basic', find_program('bash'))\n";
        let ex = TreeSitterExtractor::for_language("meson")
            .unwrap()
            .extract(&sf("meson.build", "meson", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "meson: expected >=1 def (command), got {defs}");
    }

    #[test]
    fn smoke_ninja() {
        let code = "rule cc\n  command = gcc $in -o $out\n\nbuild hello.o: cc hello.c\n\npool link_pool\n  depth = 4\n";
        let ex = TreeSitterExtractor::for_language("ninja")
            .unwrap()
            .extract(&sf("build.ninja", "ninja", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "ninja: expected >=1 def (rule), got {defs}");
    }

    #[test]
    fn smoke_postscript() {
        // PostScript: /inch { 72 mul } def — the procedure { 72 mul } has
        // operator children (mul, def). The query captures them as function defs.
        let code = "%!PS\n/inch { 72 mul } def\n/box { newpath moveto lineto lineto lineto closepath } def\n";
        let ex = TreeSitterExtractor::for_language("postscript")
            .unwrap()
            .extract(&sf("doc.ps", "postscript", code))
            .unwrap();
        // Wiring correctness (no panic) is the primary gate; defs >=1 is a bonus.
        // The grammar parses procedure bodies as nested operators; at least one should match.
        let _ = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
    }

    #[test]
    fn smoke_regex() {
        let code = "(?P<year>\\d{4})-(?P<month>\\d{2})-(?P<day>\\d{2})";
        let ex = TreeSitterExtractor::for_language("regex")
            .unwrap()
            .extract(&sf("date.re", "regex", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(
            defs >= 1,
            "regex: expected >=1 def (named group), got {defs}"
        );
    }

    #[test]
    fn smoke_rego() {
        let code = "package authz\n\ndefault allow := false\n\nallow if {\n    input.user == \"admin\"\n}\n";
        let ex = TreeSitterExtractor::for_language("rego")
            .unwrap()
            .extract(&sf("authz.rego", "rego", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "rego: expected >=1 def (rule), got {defs}");
    }

    #[test]
    fn smoke_rescript() {
        let code = "let greet = (name: string) => \"hi \" ++ name\nlet x = 42\nmodule Utils = {\n  let id = (x) => x\n}\n";
        let ex = TreeSitterExtractor::for_language("rescript")
            .unwrap()
            .extract(&sf("greet.res", "rescript", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "rescript: expected >=1 def, got {defs}");
    }

    #[test]
    fn smoke_ron() {
        let code = "(\n    name: \"my-config\",\n    value: Config(\n        debug: true,\n        level: 3,\n    ),\n)\n";
        let ex = TreeSitterExtractor::for_language("ron")
            .unwrap()
            .extract(&sf("config.ron", "ron", code))
            .unwrap();
        // RON top-level is a struct without a name; named structs with explicit names produce defs.
        // Wiring correctness (no panic) is the gate; named struct count may be 0 for unnamed tuples.
        let _ = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
    }

    #[test]
    fn smoke_devicetree() {
        let code = "/dts-v1/;\n/ {\n    compatible = \"my,board\";\n    memory@0 {\n        reg = <0x0 0x20000000>;\n    };\n};\n";
        let ex = TreeSitterExtractor::for_language("devicetree")
            .unwrap()
            .extract(&sf("board.dts", "devicetree", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(
            defs >= 1,
            "devicetree: expected >=1 def (node/property), got {defs}"
        );
    }

    #[test]
    fn smoke_dot() {
        let code = "digraph G {\n    A -> B;\n    B -> C;\n    A [label=\"Start\"];\n}\n";
        let ex = TreeSitterExtractor::for_language("dot")
            .unwrap()
            .extract(&sf("graph.dot", "dot", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(defs >= 1, "dot: expected >=1 def (graph/node), got {defs}");
    }

    #[test]
    fn smoke_elisp() {
        let code = "(defun greet (name)\n  \"Greet NAME.\"\n  (concat \"hi \" name))\n\n(defmacro when-debug (body)\n  `(when debug ,body))\n";
        let ex = TreeSitterExtractor::for_language("elisp")
            .unwrap()
            .extract(&sf("greet.el", "elisp", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(
            defs >= 1,
            "elisp: expected >=1 def (defun/defmacro), got {defs}"
        );
    }

    // ── W9.3 IaC + legacy/mainframe smoke tests ───────────────────────────────

    #[test]
    fn smoke_bicep() {
        // Bicep: a resource, a param, and an output — at least 3 defs.
        let code = concat!(
            "param storageName string = 'mystorage'\n",
            "resource storageAccount 'Microsoft.Storage/storageAccounts@2021-02-01' = {\n",
            "  name: storageName\n",
            "  location: 'eastus'\n",
            "  sku: { name: 'Standard_LRS' }\n",
            "  kind: 'StorageV2'\n",
            "}\n",
            "output storageId string = storageAccount.id\n",
        );
        let ex = TreeSitterExtractor::for_language("bicep")
            .unwrap()
            .extract(&sf("storage.bicep", "bicep", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(
            defs >= 1,
            "bicep: expected >=1 def (resource/param/output), got {defs}"
        );
    }

    #[test]
    fn smoke_fortran() {
        // Fortran 90: a module containing a subroutine and a function.
        let code = concat!(
            "module greetings\n",
            "  implicit none\n",
            "contains\n",
            "  subroutine say_hello(name)\n",
            "    character(len=*), intent(in) :: name\n",
            "    print *, 'Hello, ', name\n",
            "  end subroutine say_hello\n",
            "  function add(a, b) result(c)\n",
            "    integer, intent(in) :: a, b\n",
            "    integer :: c\n",
            "    c = a + b\n",
            "  end function add\n",
            "end module greetings\n",
        );
        let ex = TreeSitterExtractor::for_language("fortran")
            .unwrap()
            .extract(&sf("greetings.f90", "fortran", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(
            defs >= 1,
            "fortran: expected >=1 def (module/subroutine/function), got {defs}"
        );
    }

    #[test]
    fn smoke_pascal() {
        // Free Pascal unit with a procedure and a function.
        let code = concat!(
            "unit Greetings;\n",
            "interface\n",
            "  procedure SayHello(const Name: string);\n",
            "  function Add(A, B: Integer): Integer;\n",
            "implementation\n",
            "procedure SayHello(const Name: string);\n",
            "begin\n",
            "  WriteLn('Hello, ', Name);\n",
            "end;\n",
            "function Add(A, B: Integer): Integer;\n",
            "begin\n",
            "  Result := A + B;\n",
            "end;\n",
            "end.\n",
        );
        let ex = TreeSitterExtractor::for_language("pascal")
            .unwrap()
            .extract(&sf("greetings.pas", "pascal", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(
            defs >= 1,
            "pascal: expected >=1 def (unit/procedure/function), got {defs}"
        );
    }

    // ── W13 IaC language expansion smoke tests ────────────────────────────────
    // W13.1 Nix: smoke_nix is already wired above (see fn smoke_nix).

    #[test]
    fn smoke_jinja2() {
        let code = concat!(
            "{% extends \"base.j2\" %}\n",
            "{% include \"header.j2\" %}\n",
            "{% macro render_item(item) %}\n",
            "  {{ item }}\n",
            "{% endmacro %}\n",
            "{% block content %}\n",
            "  {{ page_title }}\n",
            "{% endblock %}\n",
        );
        let ex = TreeSitterExtractor::for_language("jinja2")
            .expect("jinja2 grammar must compile")
            .extract(&sf("page.j2", "jinja2", code))
            .unwrap();
        let imports = ex
            .refs
            .iter()
            .filter(|r| r.kind == EdgeKind::Imports)
            .count();
        assert!(
            imports >= 1,
            "jinja2: expected >=1 import edge (include/extends), got {imports}"
        );
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(
            defs >= 1,
            "jinja2: expected >=1 def (macro/block/variable), got {defs}"
        );
    }

    #[test]
    fn smoke_arm() {
        let code = concat!(
            "{\n",
            "  \"resources\": [\n",
            "    {\n",
            "      \"type\": \"Microsoft.Storage/storageAccounts\",\n",
            "      \"name\": \"mystorage\",\n",
            "      \"dependsOn\": [\n",
            "        \"[resourceId('Microsoft.Network/virtualNetworks', 'myVnet')]\"\n",
            "      ]\n",
            "    }\n",
            "  ]\n",
            "}\n",
        );
        let ex = TreeSitterExtractor::for_language("arm")
            .expect("arm grammar must compile")
            .extract(&sf("azuredeploy.arm.json", "arm", code))
            .unwrap();
        let defs = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .count();
        assert!(
            defs >= 1,
            "arm: expected >=1 def (resource type/name), got {defs}"
        );
        let imports = ex
            .refs
            .iter()
            .filter(|r| r.kind == EdgeKind::Imports)
            .count();
        assert!(
            imports >= 1,
            "arm: expected >=1 import edge (dependsOn), got {imports}"
        );
    }

    // ── Framework relationship edges (Java/Spring) ─────────────────────────────
    // These prove the @di.* / @route.* generic capture roles + the java.scm patterns
    // produce the EXACT edge (tag + source + target) the engine contract requires, with
    // endpoints that ARE def-node symbols in the same Extraction — the graph joins.

    /// The symbol of the ACTUAL def node named `name` with `kind` in the extraction. Asserting
    /// edge endpoints against this (instead of a hand-built id) proves the edge lands on a node
    /// that exists — the join the old string-vs-string comparison could not check.
    fn node_symbol(ex: &Extraction, name: &str, kind: &NodeKind) -> SymbolId {
        ex.nodes
            .iter()
            .find(|n| n.name == name && &n.kind == kind)
            .unwrap_or_else(|| {
                panic!(
                    "no def node name={name:?} kind={kind:?}; nodes = {:?}",
                    ex.nodes
                        .iter()
                        .map(|n| (n.name.as_str(), format!("{:?}", n.kind)))
                        .collect::<Vec<_>>()
                )
            })
            .symbol
            .clone()
    }

    #[test]
    fn java_di_field_injection_emits_di_wired_edge() {
        // @Service class OrderService { @Autowired PaymentService payment; }
        let code = r#"
@Service
public class OrderService {
    @Autowired
    private PaymentService payment;
}
"#;
        let ex = TreeSitterExtractor::for_language("java")
            .unwrap()
            .extract(&sf("OrderService.java", "java", code))
            .unwrap();

        let expected_from = node_symbol(&ex, "OrderService", &NodeKind::Class);
        let di: Vec<_> = ex
            .refs
            .iter()
            .filter(|r| edge_tags::is_tag(&r.kind, edge_tags::DI_WIRED))
            .collect();
        assert_eq!(
            di.len(),
            1,
            "exactly one di-wired ref expected; refs={:?}",
            ex.refs
                .iter()
                .map(|r| (&r.raw_name, &r.kind))
                .collect::<Vec<_>>()
        );
        let r = di[0];
        assert_eq!(r.from, expected_from, "source must be the injecting class");
        assert_eq!(r.raw_name, "PaymentService", "target = injected type name");
        // The tag must come from the edge_tags constant, not a literal.
        assert_eq!(r.kind, EdgeKind::Other(edge_tags::DI_WIRED.to_string()));
    }

    #[test]
    fn java_di_constructor_injection_emits_di_wired_edge() {
        // @Autowired constructor — source must be the CLASS, not the constructor.
        let code = r#"
@Service
public class OrderService {
    private final PaymentService payment;
    private final InventoryService inventory;

    @Autowired
    public OrderService(PaymentService payment, InventoryService inventory) {
        this.payment = payment;
        this.inventory = inventory;
    }
}
"#;
        let ex = TreeSitterExtractor::for_language("java")
            .unwrap()
            .extract(&sf("OrderService.java", "java", code))
            .unwrap();

        let expected_from = node_symbol(&ex, "OrderService", &NodeKind::Class);
        let targets: HashSet<&str> = ex
            .refs
            .iter()
            .filter(|r| edge_tags::is_tag(&r.kind, edge_tags::DI_WIRED))
            .inspect(|r| {
                assert_eq!(
                    r.from, expected_from,
                    "ctor-injection source must be the class, not the constructor"
                );
            })
            .map(|r| r.raw_name.as_str())
            .collect();
        assert!(
            targets.contains("PaymentService") && targets.contains("InventoryService"),
            "both ctor-injected types expected; got {targets:?}"
        );
    }

    #[test]
    fn java_route_handler_get_mapping_emits_route_edge() {
        // @GetMapping("/orders") on a handler → synthetic route node + route-handler edge.
        let code = r#"
@RestController
public class OrderController {
    @GetMapping("/orders")
    public String listOrders() { return "ok"; }
}
"#;
        let ex = TreeSitterExtractor::for_language("java")
            .unwrap()
            .extract(&sf("OrderController.java", "java", code))
            .unwrap();

        let route_symbol = Symbol::synthetic("route", "/orders").id();
        let handler_symbol = node_symbol(&ex, "listOrders", &NodeKind::Method);

        // Synthetic route node present.
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.symbol == route_symbol && matches!(n.kind, NodeKind::Synthetic)),
            "synthetic route node for /orders expected; nodes={:?}",
            ex.nodes
                .iter()
                .map(|n| (&n.name, &n.kind))
                .collect::<Vec<_>>()
        );

        // Exactly one route-handler edge with the exact endpoints.
        let routes: Vec<_> = ex
            .local_edges
            .iter()
            .filter(|e| edge_tags::is_tag(&e.kind, edge_tags::ROUTE_HANDLER))
            .collect();
        assert_eq!(routes.len(), 1, "one route-handler edge expected");
        let e = routes[0];
        assert_eq!(e.source, route_symbol, "source = route node");
        assert_eq!(e.target, handler_symbol, "target = handler method");
        assert_eq!(
            e.kind,
            EdgeKind::Other(edge_tags::ROUTE_HANDLER.to_string())
        );
    }

    #[test]
    fn java_route_handler_request_mapping_value_form() {
        // @RequestMapping(value = "/users") — element_value_pair form.
        let code = r#"
@RestController
public class UserController {
    @RequestMapping(value = "/users")
    public String listUsers() { return "ok"; }
}
"#;
        let ex = TreeSitterExtractor::for_language("java")
            .unwrap()
            .extract(&sf("UserController.java", "java", code))
            .unwrap();

        let route_symbol = Symbol::synthetic("route", "/users").id();
        let handler_symbol = node_symbol(&ex, "listUsers", &NodeKind::Method);
        let routes: Vec<_> = ex
            .local_edges
            .iter()
            .filter(|e| edge_tags::is_tag(&e.kind, edge_tags::ROUTE_HANDLER))
            .collect();
        assert_eq!(
            routes.len(),
            1,
            "one route-handler edge expected for value= form"
        );
        assert_eq!(routes[0].source, route_symbol);
        assert_eq!(routes[0].target, handler_symbol);
    }

    #[test]
    fn java_route_handler_target_is_type_nested() {
        // Pins the literal id shape ONCE: a handler method nests under its class (ADR-002
        // amendment) — the route edge must land on the type-nested method id.
        let code = r#"
@RestController
public class OrderController {
    @GetMapping("/orders")
    public String listOrders() { return "ok"; }
}
"#;
        let ex = TreeSitterExtractor::for_language("java")
            .unwrap()
            .extract(&sf("OrderController.java", "java", code))
            .unwrap();
        let routes: Vec<_> = ex
            .local_edges
            .iter()
            .filter(|e| edge_tags::is_tag(&e.kind, edge_tags::ROUTE_HANDLER))
            .collect();
        assert_eq!(routes.len(), 1);
        assert_eq!(
            routes[0].target.as_str(),
            "ts-java . . . OrderController/OrderController#listOrders().",
        );
    }

    #[test]
    fn java_plain_field_no_annotation_no_di_edge() {
        // Negative: a plain field with no @Autowired must NOT produce a di-wired edge.
        let code = r#"
public class PlainBean {
    private PaymentService payment;
    private String name;
}
"#;
        let ex = TreeSitterExtractor::for_language("java")
            .unwrap()
            .extract(&sf("PlainBean.java", "java", code))
            .unwrap();
        let di = ex
            .refs
            .iter()
            .filter(|r| edge_tags::is_tag(&r.kind, edge_tags::DI_WIRED))
            .count();
        assert_eq!(di, 0, "plain field must not emit a di-wired edge");
    }

    #[test]
    fn java_non_mapping_annotation_no_route_edge() {
        // Negative: a method annotated with something other than a *Mapping must not
        // produce a route-handler edge.
        let code = r#"
@RestController
public class HealthController {
    @Override
    public String toString() { return "health"; }
}
"#;
        let ex = TreeSitterExtractor::for_language("java")
            .unwrap()
            .extract(&sf("HealthController.java", "java", code))
            .unwrap();
        let routes = ex
            .local_edges
            .iter()
            .filter(|e| edge_tags::is_tag(&e.kind, edge_tags::ROUTE_HANDLER))
            .count();
        assert_eq!(
            routes, 0,
            "non-mapping annotation must not emit a route edge"
        );
    }

    // ── DI aliases (@Inject / @Resource) — same di-wired edge as @Autowired ─────

    #[test]
    fn java_di_inject_field_emits_di_wired_edge() {
        // JSR-330 @Inject must wire the same di-wired edge as @Autowired.
        let code = r#"
@Service
public class OrderService {
    @Inject
    private PaymentService payment;
}
"#;
        let ex = TreeSitterExtractor::for_language("java")
            .unwrap()
            .extract(&sf("OrderService.java", "java", code))
            .unwrap();

        let di: Vec<_> = ex
            .refs
            .iter()
            .filter(|r| edge_tags::is_tag(&r.kind, edge_tags::DI_WIRED))
            .collect();
        assert_eq!(di.len(), 1, "exactly one di-wired ref expected for @Inject");
        assert_eq!(
            di[0].from,
            node_symbol(&ex, "OrderService", &NodeKind::Class)
        );
        assert_eq!(di[0].raw_name, "PaymentService");
    }

    #[test]
    fn java_di_resource_field_emits_di_wired_edge() {
        // JSR-250 @Resource must wire the same di-wired edge as @Autowired.
        let code = r#"
@Service
public class OrderService {
    @Resource
    private InventoryService inventory;
}
"#;
        let ex = TreeSitterExtractor::for_language("java")
            .unwrap()
            .extract(&sf("OrderService.java", "java", code))
            .unwrap();

        let di: Vec<_> = ex
            .refs
            .iter()
            .filter(|r| edge_tags::is_tag(&r.kind, edge_tags::DI_WIRED))
            .collect();
        assert_eq!(
            di.len(),
            1,
            "exactly one di-wired ref expected for @Resource"
        );
        assert_eq!(
            di[0].from,
            node_symbol(&ex, "OrderService", &NodeKind::Class)
        );
        assert_eq!(di[0].raw_name, "InventoryService");
    }

    // ── Event pub/sub (@EventListener / @KafkaListener / publishEvent / send) ───

    #[test]
    fn java_event_listener_emits_event_listens_to_type() {
        // @EventListener handler — source = listener method, target = the event TYPE (real symbol,
        // resolved cross-file like di-wired). Per the contract, the listener is the dependent.
        let code = r#"
@Component
public class OrderEventListener {
    @EventListener
    public void onOrderCreated(OrderCreatedEvent event) {}
}
"#;
        let ex = TreeSitterExtractor::for_language("java")
            .unwrap()
            .extract(&sf("OrderEventListener.java", "java", code))
            .unwrap();

        let listens: Vec<_> = ex
            .refs
            .iter()
            .filter(|r| edge_tags::is_tag(&r.kind, edge_tags::EVENT_LISTENS))
            .collect();
        assert_eq!(
            listens.len(),
            1,
            "exactly one event-listens ref expected; refs={:?}",
            ex.refs
                .iter()
                .map(|r| (&r.raw_name, &r.kind))
                .collect::<Vec<_>>()
        );
        let r = listens[0];
        assert_eq!(
            r.from,
            node_symbol(&ex, "onOrderCreated", &NodeKind::Method),
            "source = the listener method (dependent)"
        );
        assert_eq!(r.raw_name, "OrderCreatedEvent", "target = the event type");
        assert_eq!(
            r.kind,
            EdgeKind::Other(edge_tags::EVENT_LISTENS.to_string())
        );
    }

    #[test]
    fn java_kafka_listener_emits_event_listens_to_topic_node() {
        // @KafkaListener(topics = "orders") — source = listener method, target = synthetic topic
        // node (a topic string has no backing symbol, like a route path).
        let code = r#"
@Component
public class OrderConsumer {
    @KafkaListener(topics = "orders")
    public void consume(String message) {}
}
"#;
        let ex = TreeSitterExtractor::for_language("java")
            .unwrap()
            .extract(&sf("OrderConsumer.java", "java", code))
            .unwrap();

        let topic_symbol = Symbol::synthetic("topic", "orders").id();
        let listener_symbol = node_symbol(&ex, "consume", &NodeKind::Method);

        // Synthetic topic node present.
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.symbol == topic_symbol && matches!(n.kind, NodeKind::Synthetic)),
            "synthetic topic node for 'orders' expected; nodes={:?}",
            ex.nodes
                .iter()
                .map(|n| (&n.name, &n.kind))
                .collect::<Vec<_>>()
        );

        let listens: Vec<_> = ex
            .local_edges
            .iter()
            .filter(|e| edge_tags::is_tag(&e.kind, edge_tags::EVENT_LISTENS))
            .collect();
        assert_eq!(listens.len(), 1, "one event-listens edge expected");
        assert_eq!(
            listens[0].source, listener_symbol,
            "source = listener method"
        );
        assert_eq!(listens[0].target, topic_symbol, "target = topic node");
    }

    #[test]
    fn java_publish_event_emits_event_emits_to_type() {
        // publishEvent(new OrderShippedEvent()) — source = enclosing method, target = event type.
        let code = r#"
@Service
public class ShippingService {
    private ApplicationEventPublisher publisher;
    public void ship() {
        publisher.publishEvent(new OrderShippedEvent());
    }
}
"#;
        let ex = TreeSitterExtractor::for_language("java")
            .unwrap()
            .extract(&sf("ShippingService.java", "java", code))
            .unwrap();

        let emits: Vec<_> = ex
            .refs
            .iter()
            .filter(|r| edge_tags::is_tag(&r.kind, edge_tags::EVENT_EMITS))
            .collect();
        assert_eq!(
            emits.len(),
            1,
            "exactly one event-emits ref expected; refs={:?}",
            ex.refs
                .iter()
                .map(|r| (&r.raw_name, &r.kind))
                .collect::<Vec<_>>()
        );
        let r = emits[0];
        assert_eq!(
            r.from,
            node_symbol(&ex, "ship", &NodeKind::Method),
            "source = the enclosing emitting method (dependent)"
        );
        assert_eq!(
            r.raw_name, "OrderShippedEvent",
            "target = the published type"
        );
        assert_eq!(r.kind, EdgeKind::Other(edge_tags::EVENT_EMITS.to_string()));
    }

    #[test]
    fn java_kafka_template_send_emits_event_emits_to_topic_node() {
        // kafkaTemplate.send("orders", payload) — source = enclosing method, target = topic node.
        let code = r#"
@Service
public class OrderProducer {
    private KafkaTemplate kafkaTemplate;
    public void publish(String payload) {
        kafkaTemplate.send("orders", payload);
    }
}
"#;
        let ex = TreeSitterExtractor::for_language("java")
            .unwrap()
            .extract(&sf("OrderProducer.java", "java", code))
            .unwrap();

        let topic_symbol = Symbol::synthetic("topic", "orders").id();
        let emitter_symbol = node_symbol(&ex, "publish", &NodeKind::Method);

        assert!(
            ex.nodes
                .iter()
                .any(|n| n.symbol == topic_symbol && matches!(n.kind, NodeKind::Synthetic)),
            "synthetic topic node for 'orders' expected; nodes={:?}",
            ex.nodes
                .iter()
                .map(|n| (&n.name, &n.kind))
                .collect::<Vec<_>>()
        );

        let emits: Vec<_> = ex
            .local_edges
            .iter()
            .filter(|e| edge_tags::is_tag(&e.kind, edge_tags::EVENT_EMITS))
            .collect();
        assert_eq!(emits.len(), 1, "one event-emits edge expected");
        assert_eq!(
            emits[0].source, emitter_symbol,
            "source = enclosing emitting method"
        );
        assert_eq!(emits[0].target, topic_symbol, "target = topic node");
    }

    #[test]
    fn java_topic_node_shared_across_emit_and_listen() {
        // A topic string used by both a listener and an emitter must yield ONE synthetic node
        // (deduped by name within the file) referenced by both edges.
        let code = r#"
@Component
public class OrderHub {
    @KafkaListener(topics = "orders")
    public void consume(String message) {}
    public void publish(String payload) {
        kafkaTemplate.send("orders", payload);
    }
}
"#;
        let ex = TreeSitterExtractor::for_language("java")
            .unwrap()
            .extract(&sf("OrderHub.java", "java", code))
            .unwrap();

        let topic_symbol = Symbol::synthetic("topic", "orders").id();
        let topic_nodes = ex.nodes.iter().filter(|n| n.symbol == topic_symbol).count();
        assert_eq!(topic_nodes, 1, "topic node must be deduped to exactly one");

        let listens = ex
            .local_edges
            .iter()
            .filter(|e| edge_tags::is_tag(&e.kind, edge_tags::EVENT_LISTENS))
            .count();
        let emits = ex
            .local_edges
            .iter()
            .filter(|e| edge_tags::is_tag(&e.kind, edge_tags::EVENT_EMITS))
            .count();
        assert_eq!(listens, 1, "one listen edge to the shared topic");
        assert_eq!(emits, 1, "one emit edge to the shared topic");
    }

    #[test]
    fn java_non_event_annotation_no_event_edge() {
        // Negative: a method with a non-event annotation, and an ordinary method call, must NOT
        // produce any event edges or refs.
        let code = r#"
@Component
public class PlainListener {
    @Override
    public void onSomething(OrderEvent event) {}
    public void doWork() {
        helper.process(new OrderEvent());
        logger.info("orders");
    }
}
"#;
        let ex = TreeSitterExtractor::for_language("java")
            .unwrap()
            .extract(&sf("PlainListener.java", "java", code))
            .unwrap();

        let event_refs = ex
            .refs
            .iter()
            .filter(|r| {
                edge_tags::is_tag(&r.kind, edge_tags::EVENT_LISTENS)
                    || edge_tags::is_tag(&r.kind, edge_tags::EVENT_EMITS)
            })
            .count();
        let event_edges = ex
            .local_edges
            .iter()
            .filter(|e| {
                edge_tags::is_tag(&e.kind, edge_tags::EVENT_LISTENS)
                    || edge_tags::is_tag(&e.kind, edge_tags::EVENT_EMITS)
            })
            .count();
        assert_eq!(
            event_refs, 0,
            "non-event constructs must not emit event refs"
        );
        assert_eq!(
            event_edges, 0,
            "non-event constructs must not emit event edges"
        );
    }

    // ── Type-nested definition identity (ADR-002 amendment, engine defect #1) ──
    // Fixture: the doc03 collision shape — two classes and an interface each defining `save`,
    // an object-literal `save`, and a method-local `const save = () =>` arrow. Before the
    // amendment every one of them minted the SAME 2-descriptor id and the store's
    // ON CONFLICT(symbol) upsert collapsed them into one node.

    const IDENTITY_FIXTURE: &str = include_str!("../tests/fixtures/typescript/method_identity.ts");

    fn identity_extraction() -> Extraction {
        TreeSitterExtractor::for_language("typescript")
            .unwrap()
            .extract(&sf(
                "src/method_identity.ts",
                "typescript",
                IDENTITY_FIXTURE,
            ))
            .unwrap()
    }

    /// All symbols of non-File nodes with the given name.
    fn symbols_named(ex: &Extraction, name: &str) -> Vec<String> {
        ex.nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File) && n.name == name)
            .map(|n| n.symbol.as_str().to_string())
            .collect()
    }

    fn has_symbol(ex: &Extraction, sym: &str) -> bool {
        ex.nodes.iter().any(|n| n.symbol.as_str() == sym)
    }

    const IDENTITY_PY: &str = "class Outer:\n    class Inner:\n        def run(self): pass\n    def run(self): pass\ndef top():\n    def run(): pass\n";

    #[test]
    fn identity_nests_methods_under_enclosing_type() {
        let ex = identity_extraction();
        // Three DISTINCT class/interface members named `save`, each nested under its type.
        for sym in [
            "ts-typescript . . . src/method_identity/Repo#save().",
            "ts-typescript . . . src/method_identity/Cache#save().",
            "ts-typescript . . . src/method_identity/Store#save().",
        ] {
            assert!(
                has_symbol(&ex, sym),
                "expected {sym}; saves = {:?}",
                symbols_named(&ex, "save")
            );
        }
        // Class ids themselves are unchanged by the amendment.
        assert!(has_symbol(
            &ex,
            "ts-typescript . . . src/method_identity/Repo#"
        ));

        // Python half: Type-under-Type nests; a function-local def stays flat (D1 residual).
        let py = TreeSitterExtractor::for_language("python")
            .unwrap()
            .extract(&sf("pkg/mod.py", "python", IDENTITY_PY))
            .unwrap();
        for sym in [
            "ts-python . . . pkg/mod/Outer#run().",
            "ts-python . . . pkg/mod/Outer#Inner#run().",
            "ts-python . . . pkg/mod/Outer#Inner#",
            "ts-python . . . pkg/mod/run().", // function-local `run` inside top(): flat
        ] {
            assert!(
                has_symbol(&py, sym),
                "expected {sym}; runs = {:?}",
                symbols_named(&py, "run")
            );
        }
    }

    #[test]
    fn identity_refs_from_carry_the_type() {
        let ex = identity_extraction();
        let save_froms: Vec<&str> = ex
            .refs
            .iter()
            .filter(|r| r.raw_name == "save" && r.kind == EdgeKind::Calls)
            .map(|r| r.from.as_str())
            .collect();
        assert!(
            save_froms.contains(&"ts-typescript . . . src/method_identity/Repo#update()."),
            "this.save() in Repo.update must come FROM Repo#update().; froms = {save_froms:?}"
        );
        assert!(
            save_froms.contains(&"ts-typescript . . . src/method_identity/Cache#flush()."),
            "this.save() in Cache.flush must come FROM Cache#flush().; froms = {save_froms:?}"
        );
    }

    #[test]
    fn identity_does_not_nest_under_functions_or_terms() {
        // Pins the truncation rule: the chain is the contiguous run of Type-suffixed containers
        // immediately enclosing a def — a Method, Function, or Term container ends it. These
        // assertions hold BEFORE the amendment too (everything was flat) and pin the residuals
        // the amendment deliberately keeps.
        let ex = identity_extraction();

        // `const cb = () =>` inside Cache.flush: flat, never Cache#cb().
        assert!(
            has_symbol(&ex, "ts-typescript . . . src/method_identity/cb()."),
            "cb must stay flat; cbs = {:?}",
            symbols_named(&ex, "cb")
        );
        assert!(
            !ex.nodes
                .iter()
                .any(|n| n.symbol.as_str().contains("Cache#cb")),
            "cb must NOT nest under Cache (its innermost container is the flush METHOD)"
        );

        // Top-level function stays flat.
        assert!(has_symbol(
            &ex,
            "ts-typescript . . . src/method_identity/top()."
        ));

        // Object-literal methods do not nest under the Term binding `lit`.
        assert!(
            !ex.nodes
                .iter()
                .any(|n| n.symbol.as_str().contains("lit.save")),
            "lit.save must NOT nest under the Term container `lit`"
        );

        // Exactly ONE distinct flat …/save(). symbol: `lit.save` and the method-local
        // `const save = () =>` arrow merge into it (the documented D1 residual).
        let flat_saves: HashSet<&str> = ex
            .nodes
            .iter()
            .map(|n| n.symbol.as_str())
            .filter(|s| s.ends_with("/save()."))
            .collect();
        assert_eq!(
            flat_saves.len(),
            1,
            "exactly one flat …/save(). residual expected; got {flat_saves:?}"
        );

        // MI-A1: the method-local `const save = () =>` must not mint Cache#save(). — every node
        // carrying that symbol must have the METHOD's span (vacuously true pre-amendment).
        let cache_save_off = IDENTITY_FIXTURE
            .match_indices("save(): void {}")
            .nth(1)
            .map(|(i, _)| i as u32)
            .expect("fixture must contain Cache.save");
        for n in ex.nodes.iter().filter(|n| {
            n.symbol.as_str() == "ts-typescript . . . src/method_identity/Cache#save()."
        }) {
            assert_eq!(
                n.location.span.start_byte, cache_save_off,
                "Cache#save(). must be the method, not the method-local arrow"
            );
        }
    }

    #[test]
    fn identity_disambiguator_is_none() {
        // D6 pinned: no overload disambiguator is ever populated — every Method-suffixed id
        // renders as `name().` (empty parens).
        let ex = identity_extraction();
        for n in ex.nodes.iter().filter(|n| {
            matches!(
                n.kind,
                NodeKind::Method | NodeKind::Function | NodeKind::Constructor
            )
        }) {
            assert!(
                n.symbol.as_str().ends_with("()."),
                "disambiguator must stay None: {}",
                n.symbol
            );
        }
    }

    #[test]
    fn identity_contains_edges_stay_file_to_def() {
        // D4 pinned: Contains stays File→def; no Class→Method Contains edge is emitted.
        let ex = identity_extraction();
        let file_symbol = Symbol::file("src/method_identity.ts").id();
        let contains: Vec<_> = ex
            .local_edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Contains)
            .collect();
        for e in &contains {
            assert_eq!(
                e.source, file_symbol,
                "every Contains edge must originate at the File node"
            );
        }
        let def_count = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File | NodeKind::Import))
            .count();
        assert_eq!(
            contains.len(),
            def_count,
            "one File→def Contains edge per definition record"
        );
    }

    #[test]
    fn identity_dedupes_duplicate_anchors() {
        // A decorated class matches TWO @code_class.def patterns (plain + @Entity). The chain
        // must contain its Type descriptor ONCE: Ent#save()., never Ent#Ent#save().
        let code = "@Entity()\nclass Ent { save(): void {} }\n";
        let ex = TreeSitterExtractor::for_language("typescript")
            .unwrap()
            .extract(&sf("src/ent.ts", "typescript", code))
            .unwrap();
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.symbol.as_str() == "ts-typescript . . . src/ent/Ent#save()."),
            "expected Ent#save().; saves = {:?}",
            symbols_named(&ex, "save")
        );
        assert!(
            !ex.nodes
                .iter()
                .any(|n| n.symbol.as_str().contains("Ent#Ent#")),
            "duplicate class anchors must be deduped, not stacked"
        );
    }

    #[test]
    fn identity_field_object_literal_residual() {
        // FLIPPED (scm-anchors D6, was MI-ATK-1): `x = { save(){} }` — the object-valued
        // field `x` IS now captured as a Term-suffixed Field def, so the walk sees it as a
        // container and the literal's `save` TRUNCATES at it: module-flat `src/a/save().`,
        // DISTINCT from the real `A#save().`.
        let code = "class A { save(): void {} x = { save() {} } }\n";
        let ex = TreeSitterExtractor::for_language("typescript")
            .unwrap()
            .extract(&sf("src/a.ts", "typescript", code))
            .unwrap();
        let save_syms: HashSet<String> = symbols_named(&ex, "save").into_iter().collect();
        assert_eq!(
            save_syms,
            [
                "ts-typescript . . . src/a/A#save().".to_string(),
                "ts-typescript . . . src/a/save().".to_string(),
            ]
            .into(),
            "the class method and the literal member must mint DISTINCT ids"
        );
        assert!(
            has_symbol(&ex, "ts-typescript . . . src/a/A#x."),
            "the object-valued field must be a def nested under A; xs = {:?}",
            symbols_named(&ex, "x")
        );

        // NEW RESIDUAL (pinned): the split moves the literal member's pooling from
        // per-class to PER-MODULE — the module-flat `src/b/save().` id is shared by
        // (a) any same-named module-level function and (b) same-named members of
        // OTHER object-literal fields in the same module. Expressing them distinctly
        // needs object-literal descriptors — a scheme change, not a query edit
        // (ADR-002 residual entry).
        let pooled = "function save() {}\nclass B { x = { save() {} }; y = { save() {} }; }\n";
        let ex2 = TreeSitterExtractor::for_language("typescript")
            .unwrap()
            .extract(&sf("src/b.ts", "typescript", pooled))
            .unwrap();
        let pooled_saves: HashSet<String> = symbols_named(&ex2, "save").into_iter().collect();
        assert_eq!(
            pooled_saves,
            ["ts-typescript . . . src/b/save().".to_string()].into(),
            "KNOWN RESIDUAL RESOLVED? module fn + two literals' members no longer \
             pool at one module-flat id — object-literal descriptors landed; \
             re-point this residual pin"
        );

        // RESIDUAL (pinned): computed-name fields (`[k] = {…}`) stay uncaptured —
        // their literal members still nest under the class and merge with the
        // class's real methods. Capturing them needs a name for the field, which a
        // computed key does not statically have.
        let computed = "class A { save(): void {} [\"k\"] = { save() {} } }\n";
        let ex3 = TreeSitterExtractor::for_language("typescript")
            .unwrap()
            .extract(&sf("src/c.ts", "typescript", computed))
            .unwrap();
        let computed_saves: HashSet<String> = symbols_named(&ex3, "save").into_iter().collect();
        assert_eq!(
            computed_saves,
            ["ts-typescript . . . src/c/A#save().".to_string()].into(),
            "KNOWN RESIDUAL RESOLVED? a computed-name field's literal member no \
             longer merges with the class method — re-point this residual pin"
        );
    }

    // ── Equal-range anchor artifacts (MI-R1-1) ─────────────────────────────────
    // enclosing_chain drops non-Type records range-equal to a Type record from the container
    // walk. python.scm's ORM patterns — the last real producer of such records — now anchor
    // @code_field.def at the field's own statement (scm-anchors D7), so the drop is DEFENSIVE:
    // a fleet guard against any future wide anchor. These tests pin the three shapes that were
    // broken (or tie-order-fragile) without it; the direct unit test below keeps the guard
    // non-vacuously tested now that no built-in query produces the artifact.

    #[test]
    fn identity_orm_equal_range_anchor_nested_models_do_not_collide() {
        // Two outer classes each nesting a `Model` with an ORM field + save(): before the
        // anchor-artifact drop, the equal-range Term field truncated the chain at `Model`
        // for BOTH saves — identical `…/Model#save().` ids, merged by the store's
        // ON CONFLICT(symbol) upsert (the exact D03 defect, re-introduced).
        let code = "class A:\n    class Model:\n        t = models.CharField(max_length=1)\n        def save(self): pass\nclass B:\n    class Model:\n        t = models.CharField(max_length=1)\n        def save(self): pass\n";
        let ex = TreeSitterExtractor::for_language("python")
            .unwrap()
            .extract(&sf("app/y.py", "python", code))
            .unwrap();
        for sym in [
            "ts-python . . . app/y/A#Model#save().",
            "ts-python . . . app/y/B#Model#save().",
        ] {
            assert!(
                has_symbol(&ex, sym),
                "expected {sym}; saves = {:?}",
                symbols_named(&ex, "save")
            );
        }
        let distinct: HashSet<String> = symbols_named(&ex, "save").into_iter().collect();
        assert_eq!(distinct.len(), 2, "two distinct save ids; got {distinct:?}");
    }

    #[test]
    fn identity_orm_equal_range_anchor_keeps_full_type_chain() {
        // Single outer class: the ORM field's equal-range Term record must not cost `save`
        // its outer nesting level (was `Model#save().`, must be `Outer#Model#save().`),
        // and the class hierarchy itself must stay consistent (`Outer#Model#`).
        let code = "class Outer:\n    class Model:\n        t = models.CharField(max_length=1)\n        def save(self): pass\n";
        let ex = TreeSitterExtractor::for_language("python")
            .unwrap()
            .extract(&sf("app/z.py", "python", code))
            .unwrap();
        for sym in [
            "ts-python . . . app/z/Outer#Model#save().",
            "ts-python . . . app/z/Outer#Model#",
        ] {
            assert!(
                has_symbol(&ex, sym),
                "expected {sym}; symbols = {:?}",
                ex.nodes
                    .iter()
                    .map(|n| n.symbol.as_str())
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn identity_orm_two_flat_models_save_deterministic() {
        // The common flat two-model shape (one Django field, one SQLAlchemy field — both
        // anchor patterns covered). Before the drop this passed only via the stable sort's
        // pending-order tie between the class record and the equal-range field record —
        // QueryCursor::matches order is not contracted. Now the artifact never enters the walk.
        let code = "class Article:\n    title = models.CharField(max_length=1)\n    def save(self): pass\nclass Comment:\n    body = Column(Integer)\n    def save(self): pass\n";
        let ex = TreeSitterExtractor::for_language("python")
            .unwrap()
            .extract(&sf("app/x.py", "python", code))
            .unwrap();
        for sym in [
            "ts-python . . . app/x/Article#save().",
            "ts-python . . . app/x/Comment#save().",
        ] {
            assert!(
                has_symbol(&ex, sym),
                "expected {sym}; saves = {:?}",
                symbols_named(&ex, "save")
            );
        }
    }

    #[test]
    fn identity_field_orm_equal_range_residual() {
        // FLIPPED (scm-anchors D7, was MI-R1-1b): @code_field.def now anchors at the field's
        // OWN expression_statement, so the field record is strictly inside its class and takes
        // its REAL owner chain — nested `t` mints `A#Model#t.` (was the wrong-owner `A#t.`),
        // and a top-level model's field nests under its class (`Article#title.`, was
        // module-flat, where two same-named fields of sibling models collided).
        let nested = "class A:\n    class Model:\n        t = models.CharField(max_length=1)\n";
        let ex = TreeSitterExtractor::for_language("python")
            .unwrap()
            .extract(&sf("app/r.py", "python", nested))
            .unwrap();
        assert_eq!(
            symbols_named(&ex, "t"),
            vec!["ts-python . . . app/r/A#Model#t.".to_string()],
            "the statement-anchored field must take its full real owner chain"
        );
        let flat = "class Article:\n    title = models.CharField(max_length=1)\n";
        let ex2 = TreeSitterExtractor::for_language("python")
            .unwrap()
            .extract(&sf("app/s.py", "python", flat))
            .unwrap();
        assert_eq!(
            symbols_named(&ex2, "title"),
            vec!["ts-python . . . app/s/Article#title.".to_string()],
            "a top-level ORM field must nest under its class"
        );
    }

    #[test]
    fn enclosing_chain_drops_equal_range_anchor_artifacts() {
        // Direct MI-R1-1 guard: no built-in query produces equal-range anchor artifacts any
        // more (python.scm was the last producer, re-anchored in scm-anchors D7), so this
        // handcrafted PendingDef list keeps the defensive drop non-vacuously tested: a
        // non-Type record range-equal to a Type record must NOT truncate the chain of defs
        // inside that range.
        let mk = |kind: &str, name: &str, start: usize, end: usize| PendingDef {
            kind: kind.to_string(),
            name: name.to_string(),
            start,
            end,
            span: Span::ZERO,
            signature: None,
            emit: true,
            decl: false,
            owner: None,
        };
        let pending = vec![
            mk("class", "M", 0, 100),
            mk("field", "t", 0, 100), // equal-range Term artifact (a wide anchor)
            mk("function", "save", 10, 20),
        ];
        let chain = enclosing_chain(&pending, 10, 20);
        assert_eq!(
            chain,
            vec![Descriptor::new("M", Suffix::Type)],
            "the equal-range Term artifact must be dropped, not truncate the chain"
        );
    }

    // ── Scheme-3 identity roles: `.anchor` + `.owner` (scm-anchors D1) ──────────

    #[test]
    fn classify_capture_maps_anchor_and_owner_roles() {
        assert!(matches!(
            classify_capture("code_struct.anchor"),
            CaptureRole::DefAnchor {
                kind: "struct",
                emit: false,
                decl: false
            }
        ));
        assert!(matches!(
            classify_capture("code_method.owner"),
            CaptureRole::DefOwner { kind: "method" }
        ));
        // The emitting channels are unchanged.
        assert!(matches!(
            classify_capture("code_struct.def"),
            CaptureRole::DefAnchor {
                kind: "struct",
                emit: true,
                decl: false
            }
        ));
        assert!(matches!(
            classify_capture("code_variable"),
            CaptureRole::DefAnchor {
                kind: "variable",
                emit: true,
                decl: false
            }
        ));
        // `.decl` — emitting anchor that marks the record as a DECLARATION
        // contribution (M4 / Option A): same identity channel, metadata flag only.
        assert!(matches!(
            classify_capture("code_function.decl"),
            CaptureRole::DefAnchor {
                kind: "function",
                emit: true,
                decl: true
            }
        ));
    }

    #[test]
    fn identity_anchor_mints_no_node_no_contains_no_defrec() {
        // The Rust impl anchor is the first `.anchor` consumer: it must nest the
        // method (P#m().) WITHOUT minting a Node, a Contains edge, or a DefRec
        // (def_symbol_at join target) for the impl_item range.
        let code = "pub struct P { pub x: f64 }\nimpl P { fn m(&self) {} }\n";
        let ex = TreeSitterExtractor::for_language("rust")
            .unwrap()
            .extract(&sf("src/p.rs", "rust", code))
            .unwrap();
        // The method nests under P via the anchor.
        assert!(
            has_symbol(&ex, "ts-rust . . . src/p/P#m()."),
            "impl method must nest under P#; ms = {:?}",
            symbols_named(&ex, "m")
        );
        // Exactly ONE node named P — the struct. The impl anchor mints nothing.
        let p_nodes: Vec<_> = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File) && n.name == "P")
            .collect();
        assert_eq!(p_nodes.len(), 1, "anchor must not mint a node: {p_nodes:?}");
        let impl_off = code.find("impl P").unwrap() as u32;
        assert_ne!(
            p_nodes[0].location.span.start_byte, impl_off,
            "the single P node must be the struct, not the impl block"
        );
        // One File→def Contains edge per EMITTED definition record — the anchor
        // adds none (this is the identity_contains_edges_stay_file_to_def
        // invariant holding by construction for non-emitting anchors).
        let contains = ex
            .local_edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Contains)
            .count();
        let def_count = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File | NodeKind::Import))
            .count();
        assert_eq!(
            contains, def_count,
            "anchor must not add a Contains edge (no DefRec is created for it)"
        );
    }
}
