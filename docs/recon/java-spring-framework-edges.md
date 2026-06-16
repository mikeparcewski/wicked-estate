# Recon: Java/Spring framework-relationship edges (DI wiring, route handlers)

Scope: add `Other("di-wired")` and `Other("route-handler")` edges for Java/Spring to the
`wicked-estate-extract` crate. Seam already committed in core (`edge_tags.rs`) at HEAD 79c9205.

## How edges flow today (verified by reading `src/treesitter.rs`)

The generic tree-sitter `Extractor::extract` is one implementation for all languages, driven by
per-language `.scm` query files using the `@code_*` / `@call.*` / `@import*` / heritage capture
convention. There is **no `match lang { ... }`** for relationship logic — that is forbidden
("Rules as DATA", CLAUDE.md). Relationships are produced two ways:

- **`UnresolvedRef`** (target by *name*, resolved cross-file later): `Calls`, `Imports`, `Extends`,
  `Implements`. Each ref's `from` is the *enclosing definition* (`enclosing(&defs, pos)`), found by
  smallest byte-range def containing the ref position.
- **`local_edge`** (both endpoints known at parse time): `Contains` (file→def), `Imports`
  (file→import node).

`classify_capture(cap_name) -> CaptureRole` is the dispatch point; the match loop collects roles,
then emits nodes/refs/edges. Adding a new relationship class = a new `CaptureRole` + an arm in
`classify_capture` + emission in the loop, plus `.scm` patterns. The role handling is **generic**
(any language's `.scm` can use the new captures), so the language stays data.

## Java grammar ground truth (tree-sitter-java 0.23.5 node-types.json)

- `field_declaration`: children `modifiers` (holds `marker_annotation`/`annotation`); field `type`
  is `_unannotated_type` (for a plain `Foo` this is `type_identifier`); field `declarator`.
- `constructor_declaration`: children `modifiers`; field `parameters` → `formal_parameters` →
  `formal_parameter` with field `type` (`_unannotated_type`).
- `method_declaration`: children `annotation`/`marker_annotation`/`modifiers`; field `name`.
- `marker_annotation` (no args) field `name`; `annotation` (args) fields `name` + `arguments`
  (`annotation_argument_list` → bare `string_literal` or `element_value_pair`). `string_literal` →
  `string_fragment`.

## SymbolId scheme (verified empirically against the def loop, NOT the prompt's recon)

The def loop builds **2-descriptor** global symbols, it does NOT nest method under class:
`Symbol::global("ts-java", None, [Descriptor(module_path, Namespace), Descriptor(name, suffix)])`
where `module_path` = path without extension, `def_suffix("class")=Type`, `def_suffix("method")=Method`.
The prompt's recon claimed method = `[module, ClassName#, method()]` (3 descriptors) — that is WRONG
for this codebase; matching it would dangle every edge. I match the real 2-descriptor scheme.

## Predicates are honored

`cursor.matches(query, root, src)` (tree-sitter 0.24/0.25) auto-applies text predicates. Proof:
python `code_constant.name` uses `(#match? @cap "^[A-Z]...")` and the constant-vs-variable tests
pass (treesitter.rs ~line 3018). So `#eq?`/`#any-of?` can filter annotation names safely.

## Decision

- **DI wiring** (`@Autowired` field + constructor): `.scm` patterns nested under `class_declaration`
  binding `@di.source.name` (the class identifier → build class symbol) + `@di.target` (injected
  type identifier → `raw_name`), filtered by `(#eq? @_anno "Autowired")`. Emit
  `UnresolvedRef { from: class_symbol, raw_name: TypeName, kind: Other("di-wired") }`. Source is the
  class (NOT `enclosing()`, which would return the constructor for ctor injection) — so the source
  is captured explicitly. Resolved cross-file exactly like `extends`.
- **Route handler** (`@GetMapping`/`@PostMapping`/`@PutMapping`/`@DeleteMapping`/`@RequestMapping`):
  `.scm` binds `@route.path` (string_fragment) + `@route.handler.name` (method identifier). Emit a
  synthetic node `Symbol::synthetic("route", path)` (NodeKind::Synthetic) + a `local_edge`
  route → handler-method (`Other("route-handler")`), per the seam contract (source = route, target =
  handler). The handler method symbol is built with the identical 2-descriptor scheme.

Both use the `edge_tags::{DI_WIRED, ROUTE_HANDLER}` constants — no hardcoded strings.
