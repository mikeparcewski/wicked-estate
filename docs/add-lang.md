# Adding a Language — W6.3 Zero-Core-Change Workflow

**W6.3** — adding a new language to wicked-estate requires **zero changes to core crates**. It is
four steps: a manifest row, a query file, a grammar crate + `LANG_TABLE` entry, and a
characterization test.

This is the rules-as-data architecture from the design notes.
The archived a major per-language parser project was abandoned partly because every new language required
compiled-in dispatch logic; wicked-estate routes through data instead.

---

## The four steps

### Step 1 — Add a row to `languages.toml`

`crates/wicked-estate-extract/languages.toml` is the language manifest. It is embedded at compile time
(`include_str!`) and is the source of truth for the generated capability matrix
(`docs/language-coverage-matrix.md`).

Add a `[[language]]` section. All fields are required except `ext` (which may be empty while
you wire the grammar):

```toml
[[language]]
name = "kotlin"
ext = ["kt", "kts"]
grammar = "tree-sitter-kotlin"
tier = "structural"
caps = ["symbols", "calls", "imports", "extends"]
```

**`tier` values:**

| Tier | Meaning |
|------|---------|
| `document` | Config/markup — symbols only (YAML, JSON, HTML, HCL, …). |
| `tags` | Symbol definitions only (no call/import edges). |
| `structural` | Symbols + calls + imports from tree-sitter (+ heritage if the grammar expresses it). |
| `precise` | Structural + cross-file resolved refs via SCIP/TSG/LSP. Set this when a SCIP indexer is wired (W2.2). |

**`caps` values:** `symbols`, `calls`, `imports`, `extends`, `implements`. These must match what
your `.scm` file actually captures — they feed the generated matrix, not the extractor itself.

The `≥73` parity test in `crates/wicked-estate-extract/src/lib.rs::covers_prior art_language_parity`
asserts `registry().len() >= 73`. Adding a row keeps it green; removing one will fail it.

### Step 2 — Write `crates/wicked-estate-extract/src/queries/<name>.scm`

The query file uses the [prior art capture convention][cqconv] — the same `.scm` format
prior art and prior art use. One format drives all 73 languages; the extractor is generic.

**Capture families and what they produce:**

| Capture | Produces |
|---------|----------|
| `@code_<kind>.def` | Definition anchor (the whole node). Paired with `.name`. |
| `@code_<kind>.name` | The identifier child of a definition. Paired with `.def`. |
| `@code_<kind>.arrow` | Arrow-function variant anchor (JS/TS). Paired with `.name`. |
| `@call.function` | Direct function-call name → `EdgeKind::Calls`. |
| `@call.method` | Method-call name → `EdgeKind::Calls`. |
| `@import` | Import statement node (raw text used if no `.source`). |
| `@import.source` | The path string inside an import → used as the module name. |
| `@code_extends.def` + `@code_extends.target` | Class hierarchy → `EdgeKind::Extends`. |
| `@code_implements.def` + `@code_implements.target` | Interface impl → `EdgeKind::Implements`. |

**`<kind>` values** the extractor recognises: `function`, `method`, `class`, `struct`, `enum`,
`trait`, `interface`, `module`, `namespace`, `constructor`, `constant`, `variable`, `field`,
`property`, `type_alias`, `type`, `enum_member`, `macro`. Any other string becomes
`NodeKind::Other(string)`.

**Minimal example — `kotlin.scm`:**

```scheme
; wicked_estate Kotlin extraction queries — @code_* convention.

; Class declarations
(class_declaration
  name: (simple_identifier) @code_class.name
) @code_class.def

; Function declarations
(function_declaration
  name: (simple_identifier) @code_function.name
) @code_function.def

; Function calls
(call_expression
  (navigation_expression
    (simple_identifier) @call.method))

(call_expression
  calleeExpression: (simple_identifier) @call.function)

; Import statements
(import_header
  identifier: (identifier) @import.source
) @import

; Class inheritance
(delegation_specifier
  (user_type (simple_identifier) @code_extends.target)
) @code_extends.def
```

Run `cargo test --workspace` after writing the file to catch query compilation errors. A
malformed `.scm` causes `TreeSitterExtractor::for_language` to return `None` (broken query →
language unavailable, not a panic).

### Step 3 — Add the grammar crate and a `LANG_TABLE` entry

**3a. Add the grammar crate to `Cargo.toml`**

In `crates/wicked-estate-extract/Cargo.toml`, add a dependency on the grammar crate:

```toml
tree-sitter-kotlin = "0.3"  # must be ABI-14 compatible — see ABI constraint below
```

**3b. Add a language fn in `treesitter.rs`**

```rust
fn lang_kotlin() -> tree_sitter::Language {
    tree_sitter_kotlin::LANGUAGE.into()
}
```

**3c. Add the embedded query constant**

At the top of `treesitter.rs`, alongside the other `include_str!` constants:

```rust
const KOTLIN_QUERY: &str = include_str!("queries/kotlin.scm");
```

**3d. Add the `LANG_TABLE` entry**

In the `static LANG_TABLE: &[LangEntry]` array:

```rust
LangEntry {
    name: "kotlin",
    ext: &["kt", "kts"],
    make_language: lang_kotlin,
    query_src: KOTLIN_QUERY,
},
```

The `ext` field here drives runtime extension→extractor dispatch; it should match what you put
in `languages.toml`. (The two lists are intentionally decoupled — `LANG_TABLE.ext` is what is
*actually wired*; `languages.toml.ext` is the aspirational manifest used by the coverage matrix
generator. Keep them in sync.)

### Step 4 — Add a characterization test

Add a test in `crates/wicked-estate-extract/src/treesitter.rs` (or a separate test module) that:

1. Constructs a minimal snippet of the new language.
2. Calls `TreeSitterExtractor::for_language("<name>")`.
3. Asserts that `for_language` returns `Some` (grammar + query compiled).
4. Asserts the expected node counts and/or edge kinds.

**Example:**

```rust
#[test]
fn smoke_kotlin() {
    let code = r#"
class Greeter(val name: String) {
    fun greet(): String = hello(name)
    private fun hello(n: String): String = "hi $n"
}
import kotlin.io.println
"#;
    let ex = TreeSitterExtractor::for_language("kotlin")
        .expect("kotlin grammar must compile")
        .extract(&sf("Greeter.kt", "kotlin", code))
        .unwrap();

    let defs = ex.nodes.iter().filter(|n| !matches!(n.kind, NodeKind::File)).count();
    assert!(defs >= 1, "expected >=1 definition, got {defs}");

    let calls = ex.refs.iter().filter(|r| r.kind == EdgeKind::Calls).count();
    assert!(calls >= 1, "expected >=1 call ref, got {calls}");
}
```

This test is the regression gate. The `≥73` parity test (`covers_prior art_language_parity`)
ensures the manifest count never drops; per-language smoke tests ensure extraction actually
works. Both must be green before you mark the language done.

---

## ABI-14 constraint

**tree-sitter 0.24 supports ABI 13 and 14 only.** Grammar crates that have adopted ABI 15
(tree-sitter 0.25 grammar API) cannot be linked against our tree-sitter 0.24 workspace.

Known ABI-15 grammars you cannot use *yet*:

| Grammar crate | Last ABI-14 version |
|---------------|---------------------|
| `tree-sitter-go` | 0.21.x |
| `tree-sitter-bash` | 0.21.x |
| `tree-sitter-javascript` | 0.21.x |
| `tree-sitter-c` | 0.21.x |
| `tree-sitter-c-sharp` | 0.21.x (`language()` fn, not `LANGUAGE` const) |
| `tree-sitter-hcl` | none available on crates.io as of 2026-06 |

The comment in `treesitter.rs` documents the exact version where each was dropped. The
`c-sharp` entry is the lived example — it uses the old `language()` fn instead of `LANGUAGE.into()`
because the newer crate dropped ABI 14.

When tree-sitter 0.25 is adopted workspace-wide, update `Cargo.toml` and the language
functions in one change, and these entries can be upgraded.

**How to check:** if a grammar crate causes a link error like `"incompatible grammar ABI"` or
`"expected ABI 14, got 15"`, the crate version is too new. Pin to an earlier version or wait
for the workspace tree-sitter upgrade.

---

## Regenerate the coverage matrix

After adding the language, regenerate the docs matrix:

```bash
python3 scripts/gen-coverage-matrix.py
```

This reads `languages.toml` and greps `LANG_TABLE` in `treesitter.rs`, cross-references them,
and writes `docs/language-coverage-matrix.md`. The `--check` flag exits 1 if the file is stale
(useful in CI):

```bash
python3 scripts/gen-coverage-matrix.py --check
```

---

## Checklist

```
[ ] Row added to crates/wicked-estate-extract/languages.toml (name, ext, grammar, tier, caps)
[ ] crates/wicked-estate-extract/src/queries/<name>.scm written with @code_*/call/import captures
[ ] Grammar crate added to crates/wicked-estate-extract/Cargo.toml (ABI-14 compatible version)
[ ] lang_<name>() fn added in treesitter.rs
[ ] <NAME>_QUERY constant added in treesitter.rs (include_str!)
[ ] LANG_TABLE entry added in treesitter.rs
[ ] Smoke / characterization test added
[ ] cargo build --workspace  → 0 warnings
[ ] cargo test --workspace   → covers_prior art_language_parity passes + new smoke test passes
[ ] cargo clippy --workspace --all-targets -- -D warnings  → clean
[ ] python3 scripts/gen-coverage-matrix.py  → docs/language-coverage-matrix.md regenerated
```

---

## References

- `crates/wicked-estate-extract/languages.toml` — the manifest
- `crates/wicked-estate-extract/src/treesitter.rs` — `LANG_TABLE`, existing language fns, `IaCExtractor`
- `crates/wicked-estate-extract/src/lib.rs` — `registry()`, `covers_prior art_language_parity` test
- `docs/language-coverage-matrix.md` — generated matrix (do not edit by hand)
- `scripts/gen-coverage-matrix.py` — matrix generator
- the design notes — rules-as-data rationale
- `docs/plan/WAVE-PLAN.md` W2.1, W6.1, W6.3 — language fan-out + extractor SDK

[cqconv]: https://github.com/tree-sitter/tree-sitter/blob/master/docs/syntax-highlighting.md
"Tree-sitter capture name conventions"
