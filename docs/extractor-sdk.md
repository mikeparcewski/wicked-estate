# Extractor SDK — Adding Languages and Non-Code Edges

**W6.1 / W6.3 / W8.4** — this document covers two extension points:

1. **Adding a language** — a `languages.toml` row + a `<name>.scm` query file + a `LangEntry`
   in `treesitter.rs` + a smoke test. Zero changes to core crates (`wicked-estate-core`, `wicked-estate-store`, etc.).

2. **Drop-in non-code edges** — the `ExtraEdgeExtractor` (`crates/wicked-estate-extract/src/extra_edge.rs`)
   injects event-bus, command-dispatch, framework-hook, and other domain edges into the graph
   from a plain TOML rule file. Also zero core changes.

---

## Part 1 — Adding a Language

The rules-as-data architecture is the lesson from the archived a major per-language parser project, which
required compiled-in dispatch for every language and was eventually abandoned. In wicked-estate,
**a new language is a new grammar crate + a `.scm` query file + a data row**. The extractor
pipeline is generic over that data.

See `docs/add-lang.md` for the full step-by-step workflow and ABI compatibility checklist. This
section provides the conceptual model and a reference for the capture convention.

### The three data artefacts

| Artefact | Location | Role |
|----------|----------|------|
| `[[language]]` row | `crates/wicked-estate-extract/languages.toml` | Source of truth for the capability matrix generator. `name`, `ext`, `grammar`, `tier`, `caps` fields. |
| `<name>.scm` | `crates/wicked-estate-extract/src/queries/<name>.scm` | Tree-sitter query that maps parse-tree nodes to wicked-estate captures. |
| `LangEntry` | `static LANG_TABLE` in `crates/wicked-estate-extract/src/treesitter.rs` | Wires the grammar crate + compiled query into the runtime dispatch table. Also holds the `ext` field used for extension-to-extractor routing. |

The `registry()` function in `crates/wicked-estate-extract/src/lib.rs` iterates `LANG_TABLE` and exposes
it to the pipeline. The `covers_prior art_language_parity` test asserts `registry().len() >= 73`
— this is the regression gate that prevents silent drops.

### Language count as of this writing

75 languages are wired in `LANG_TABLE` (verified by counting `LangEntry` items). 98 rows exist
in `languages.toml` (the manifest also tracks future candidates and deferred grammars).

The WAVE-PLAN headline "76 languages" includes COBOL (wired via `arborium-cobol`). The precise
wired count is in `LANG_TABLE`; the manifest count is aspirational and includes rows for
grammars that are deferred pending the tree-sitter 0.25 workspace upgrade.

### Grammar ABI split: tree-sitter 0.24 vs arborium (ABI-15 / 0.25)

The workspace currently runs **tree-sitter 0.24**, which supports grammar ABI 13 and 14 only.

Many community grammars have moved to ABI 15 (the tree-sitter 0.25 grammar API). The
`arborium` family of crates repackages these ABI-15 grammars so they compile against the 0.25
runtime that arborium bundles — allowing them to coexist in the same Cargo workspace as the
0.24-based grammars.

**Rule of thumb:**

- Grammar available on crates.io at ABI 14 → use the crate directly (e.g. `tree-sitter-rust`,
  `tree-sitter-python`, `tree-sitter-go 0.21.x`).
- Grammar only available at ABI 15 → use the `arborium-<name>` crate (e.g. `arborium-cobol`,
  `arborium-hcl`, `arborium-ada`, and ~49 others already in use).
- Grammar crate causes a link error `"expected ABI 14, got 15"` → the pinned version is wrong;
  use the last ABI-14 release or switch to the arborium variant.

COBOL is wired via `arborium-cobol` (ABI 15), making it a concrete example of the arborium
path. The `treesitter.rs` comment block on each `lang_*` function documents the exact ABI.

When the workspace adopts tree-sitter 0.25 workspace-wide, the ABI-14 crates can be upgraded
and the `arborium-*` indirection becomes optional for those languages. That change touches
`Cargo.toml` and the `lang_*` functions, not the `.scm` files or `languages.toml`.

### The `.scm` capture convention

All query files use the `@code_<kind>.def / .name` convention shared with prior art. The
extractor is generic — it does not know about individual languages; it reads the captures.

**Capture families:**

| Capture | Produces |
|---------|----------|
| `@code_<kind>.def` | Definition anchor (the whole node span). Must be paired with `.name`. |
| `@code_<kind>.name` | The identifier child of a definition. Paired with `.def`. |
| `@code_<kind>.arrow` | Arrow-function variant anchor (JS/TS). Paired with `.name`. |
| `@call.function` | Direct function-call name → `EdgeKind::Calls`. |
| `@call.method` | Method-call name → `EdgeKind::Calls`. |
| `@import` | Import statement node (raw text used if no `.source`). |
| `@import.source` | The path string inside an import → used as the module name. |
| `@code_extends.def` + `@code_extends.target` | Heritage → `EdgeKind::Extends`. |
| `@code_implements.def` + `@code_implements.target` | Interface impl → `EdgeKind::Implements`. |

**`<kind>` values** the extractor recognises (becomes `NodeKind`): `function`, `method`,
`class`, `struct`, `enum`, `trait`, `interface`, `module`, `namespace`, `constructor`,
`constant`, `variable`, `field`, `property`, `type_alias`, `type`, `enum_member`, `macro`.
Any other string becomes `NodeKind::Other(string)`.

**Minimal example — Kotlin:**

```scheme
; Kotlin extraction queries — @code_* convention.
(class_declaration
  name: (simple_identifier) @code_class.name
) @code_class.def

(function_declaration
  name: (simple_identifier) @code_function.name
) @code_function.def

(call_expression
  (navigation_expression
    (simple_identifier) @call.method))

(import_header
  identifier: (identifier) @import.source
) @import
```

### Smoke test requirement

Every wired language needs a smoke test in `crates/wicked-estate-extract/src/treesitter.rs`:

```rust
#[test]
fn smoke_kotlin() {
    let code = r#"
class Greeter(val name: String) {
    fun greet(): String = hello(name)
}
import kotlin.io.println
"#;
    let ex = TreeSitterExtractor::for_language("kotlin")
        .expect("kotlin grammar must compile")
        .extract(&sf("Greeter.kt", "kotlin", code))
        .unwrap();

    let defs = ex.nodes.iter().filter(|n| !matches!(n.kind, NodeKind::File)).count();
    assert!(defs >= 1, "expected >=1 definition, got {defs}");
}
```

`TreeSitterExtractor::for_language` returns `None` if the grammar or query fails to compile.
The test asserts `Some` and checks extraction produces at least the expected node/edge counts.

### Quick checklist

```
[ ] [[language]] row in crates/wicked-estate-extract/languages.toml
[ ] crates/wicked-estate-extract/src/queries/<name>.scm  (captures per convention above)
[ ] Grammar crate in crates/wicked-estate-extract/Cargo.toml  (ABI-14 crate or arborium-* for ABI-15)
[ ] lang_<name>() fn in treesitter.rs
[ ] <NAME>_QUERY constant (include_str!)
[ ] LangEntry in LANG_TABLE
[ ] Smoke test passing
[ ] cargo build --workspace  0 warnings
[ ] cargo test --workspace   covers_prior art_language_parity passes
[ ] cargo clippy --workspace --all-targets -- -D warnings  clean
```

---

## Part 2 — ExtraEdgeExtractor: Non-Code Domain Edges

`crates/wicked-estate-extract/src/extra_edge.rs` provides a config-driven extractor for injecting
**domain-specific relationships** into the graph without touching any core crate.

Use cases:

- **Event-bus** — mark every `bus.emit("topic.name")` and `bus.subscribe("topic.name")` call so
  the graph knows which files produce and consume each topic. Blast-radius then crosses the bus.
- **Command→agent dispatch** — a CLI `dispatch("agent-name", ...)` pattern: inject edges from
  the dispatcher to the synthetic agent node.
- **Framework hooks** — `registerHook("before_save", handler)`: inject edges from the handler
  file to a synthetic hook node so blast-radius knows what fires when `before_save` triggers.
- **Capability edges** — any pattern where one file "claims" a capability or "depends on" a
  named feature that isn't a code symbol.

### How it works

Rules are declared in a TOML file (typically `.wicked-estate-extractors/<name>.toml` in your repo,
or passed directly via API). **`wicked-estate index <path>` auto-discovers every
`.wicked-estate-extractors/*.toml` under the indexed root** and runs the rules as part of the
pipeline — no separate step. Three discovery behaviors worth knowing:

- **Hidden files are admitted for rules.** The main walk skips dotfiles, but a file matched by a
  rule's `file_glob` (e.g. `.claude-plugin/archetypes.json`) is indexed anyway — it gets a File
  node plus the rule-injected nodes/edges (no language extraction unless its extension is
  supported).
- **Editing the rules forces a full re-extract** (tracked via the `extra_rules_digest` store meta
  key), so edges injected by an old rule set never linger on unchanged files.
- **Dangling injected edges are pruned.** An edge whose target (e.g. a `target_kind = "file"`
  path that doesn't exist in the graph) or synthetic source is missing is removed by the
  indexer's dangling-edge prune — a declared-but-missing target never fabricates a relationship.

Each `[[rule]]` block specifies:

| Field | Required | Meaning |
|-------|----------|---------|
| `name` | yes | Human name; also used as the default `node_scheme` and as `Provenance::Extractor(name)`. |
| `file_glob` | yes | Glob filter — `*` within a segment, `**` across separators. |
| `pattern` | yes | Regex applied over the file text. Named captures become template variables. |
| `[rule.emit_node]` | optional | Injects a synthetic node per match. |
| `[rule.emit_edge]` | optional | Injects an edge from the file node to the synthetic target. |

**`[rule.emit_node]` fields:**

| Field | Required | Meaning |
|-------|----------|---------|
| `id_template` | yes | Template with `{capture_name}` placeholders. Expands to the synthetic node's stable ID. |
| `label_capture` | yes | Which capture is the human-readable label. |
| `kind` | no | `"synthetic"` (default) or `"other:<tag>"`. |
| `node_scheme` | no | Scheme for `Symbol::Synthetic { scheme, id }`. **Set the same value in every rule that should converge on the same node** (e.g. emit + consume rules for the same topic). Defaults to `rule.name`. |

**`[rule.emit_edge]` fields:**

| Field | Required | Meaning |
|-------|----------|---------|
| `kind` | yes | `"other:<tag>"` or a built-in kind (`"calls"`, `"imports"`, …). |
| `target_id_template` | yes | Must expand to the same value as `emit_node.id_template` so both rules hit the same `SymbolId`. With `target_kind = "file"` it is the repo-relative path of the target file instead. |
| `target_node_scheme` | no | Must match `emit_node.node_scheme`. Defaults to `rule.name`. Ignored when `target_kind = "file"`. |
| `target_kind` | no | `"synthetic"` (default) or `"file"`. With `"file"`, the edge lands on the **literal file node** at the expanded path — if that file is not in the graph the edge is pruned as dangling (file-existence guard). |
| `source_id_template` | no | When set, the edge STARTS at the synthetic node `Symbol::Synthetic { scheme: source_node_scheme, id: expanded }` instead of the matched file's node. The synthetic source must be emitted by this or a sibling rule's `emit_node`, or the edge is pruned. |
| `source_node_scheme` | no | Scheme for the synthetic source. Defaults to `rule.name`. Only read when `source_id_template` is set. |

### Event-bus example

The canonical use case — producer and consumer calls land on the **same** synthetic topic node
because both rules share `node_scheme = "event-bus-topic"`. Blast-radius on a topic then reaches
both producers and consumers.

```toml
# .wicked-estate-extractors/event-bus.toml

[[rule]]
name      = "event-bus-emit"
file_glob = "**/*.js"
pattern   = "emit\\([\"'](?P<topic>[\\w.]+)"

[rule.emit_node]
id_template   = "topic:{topic}"
label_capture = "topic"
kind          = "synthetic"
node_scheme   = "event-bus-topic"   # same scheme across emit + consume

[rule.emit_edge]
kind               = "other:emits"
target_id_template = "topic:{topic}"
target_node_scheme = "event-bus-topic"

[[rule]]
name      = "event-bus-consume"
file_glob = "**/*.js"
pattern   = "subscribe\\([\"'](?P<topic>[\\w.]+)"

[rule.emit_node]
id_template   = "topic:{topic}"
label_capture = "topic"
kind          = "synthetic"
node_scheme   = "event-bus-topic"   # same scheme → same SymbolId as the emit rule

[rule.emit_edge]
kind               = "other:consumes"
target_id_template = "topic:{topic}"
target_node_scheme = "event-bus-topic"
```

With these rules applied, `blast-radius orders.created` returns every file that emits or
consumes the `orders.created` topic — the graph now sees through the event bus.

### Catalog → file example (source/target overrides)

A JSON catalog wires names to files by *convention* (no symbol reference). Two rules over the
same catalog: rule 1 emits a synthetic node per catalog key (+ a `contains` edge from the catalog
file); rule 2 emits an edge FROM that synthetic node TO the **literal file node** the convention
names. This is the wicked-garden archetype→playbook shape (ADR 0005 there):

```toml
# .wicked-estate-extractors/archetype.toml

[[rule]]
name      = "archetype-declare"
file_glob = ".claude-plugin/archetypes.json"
pattern   = '(?m)^ {4}"(?P<name>[a-z][a-z0-9_-]*)":\s*\{'

[rule.emit_node]
id_template   = "archetype:{name}"
label_capture = "name"
kind          = "other:archetype"
node_scheme   = "archetype"

[rule.emit_edge]
kind               = "contains"
target_id_template = "archetype:{name}"
target_node_scheme = "archetype"

[[rule]]
name      = "archetype-playbook"
file_glob = ".claude-plugin/archetypes.json"
pattern   = '(?m)^ {4}"(?P<name>[a-z][a-z0-9_-]*)":\s*\{'

[rule.emit_edge]
kind               = "references"
source_id_template = "archetype:{name}"   # start at rule 1's synthetic node
source_node_scheme = "archetype"
target_kind        = "file"               # land on the LITERAL playbook file node
target_id_template = "skills/archetype/refs/{name}.md"
```

`blast-radius skills/archetype/refs/triage.md` now surfaces `archetype:triage` and, transitively,
the catalog. A catalog key whose playbook file is missing keeps its archetype node (queryable as
"declared but playbook-less") but the file edge is pruned — never fabricated.

### Stable synthetic IDs

Synthetic node IDs are:
```
Symbol::Synthetic { scheme: node_scheme, id: expanded_id_template }
```

Because the ID is derived from the **captured value** (e.g. the topic name) and not from file
path or line number, two files that emit/consume the same topic produce the same `SymbolId`.
This is how the graph connects producers to consumers through a shared topic node. The
`node_scheme` field is the convergence key: set it identically in every rule that should
contribute to the same logical node pool.

### Running ExtraEdgeExtractor

```rust
use wicked_estate_extract::extra_edge::ExtraEdgeExtractor;
use wicked_estate_core::SourceFile;

let toml_src = std::fs::read_to_string(".wicked-estate-extractors/event-bus.toml")?;
let extractor = ExtraEdgeExtractor::from_toml(&toml_src)?;

let file = SourceFile { path: "src/orders.js".into(), language: Language::new("javascript"), text: src };
let extra = extractor.extract_extra(&file);
// extra.nodes — synthetic topic nodes
// extra.edges — emits/consumes edges (from file node to topic node)

// Merge into your Extraction or apply directly to the store.
```

The returned `ExtraExtraction` contains `nodes` (synthetic) and `edges` (domain). These can be
merged into an `Extraction` struct or written directly to the `GraphStore` via `upsert_node` /
`upsert_edge`. The `GraphStore` deduplication by `symbol` PK (nodes) and `dedup_key` (edges)
makes repeated runs idempotent.

### Idempotency guarantee

`ExtraEdgeExtractor::extract_extra` is deterministic: same input → same synthetic `SymbolId`s
and edges. The `GraphStore` deduplication layer (ON CONFLICT REPLACE with higher-confidence-wins
for edges) makes the result idempotent across re-runs.

---

## References

- `crates/wicked-estate-extract/src/extra_edge.rs` — `ExtraEdgeExtractor` source + inline doc
- `crates/wicked-estate-extract/src/treesitter.rs` — `LANG_TABLE`, `LangEntry`, language fns
- `crates/wicked-estate-extract/languages.toml` — language manifest (98 rows; 75 wired as of this writing)
- `docs/add-lang.md` — full add-language checklist + ABI constraint table
- `docs/language-coverage-matrix.md` — auto-generated matrix
- the design notes — rules-as-data rationale
- `docs/plan/WAVE-PLAN.md` W2.1, W6.1, W6.3 — language fan-out + extractor SDK tasks
