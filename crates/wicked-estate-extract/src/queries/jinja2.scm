; wicked_estate Jinja2 extraction queries — W13.2 IaC.
;
; Verified against tree-sitter-jinja2-0.0.14 grammar.js / node-types.json.
;
; The grammar is very flat: every template construct is a `statement` node
; containing a `keyword` leaf and optional identifiers/strings. There are no
; sub-structured AST nodes for individual statement types. We use
; `#eq?` predicates on the keyword to differentiate statement kinds.
;
; Named nodes used:
;   source_file  — root node
;   statement    — {% keyword ... %} — children: statement_begin, keyword, ..., statement_end
;   expression   — {{ ... }}       — children: expression_begin, ..., expression_end
;   keyword      — literal keyword (include / import / extends / macro / block / …)
;   identifier   — bare name (field name: identifier on statement / expression)
;   string       — 'quoted' or "quoted" value
;
; Strategy: we match `statement` with keyword predicates to detect include/import/extends/macro,
; then capture the first string child as import.source or the first identifier as the def name.

; ── Include / import / extends → import edges ────────────────────────────────
; {% include "file.j2" %}  — keyword is "include"
; The string is a direct child of the statement node.
(statement
  (keyword) @_kw (#eq? @_kw "include")
  (string) @import.source
) @import

; {% extends "base.j2" %}  — keyword is "extends"
(statement
  (keyword) @_kw (#eq? @_kw "extends")
  (string) @import.source
) @import

; {% import "macros.j2" as m %}  — keyword is "import"
(statement
  (keyword) @_kw (#eq? @_kw "import")
  (string) @import.source
) @import

; ── Macro definitions → function nodes ───────────────────────────────────────
; {% macro render_user(user) %}  — keyword is "macro", next is identifier (macro name)
(statement
  (keyword) @_kw (#eq? @_kw "macro")
  (identifier) @code_function.name
) @code_function.def

; ── Block definitions → struct nodes ─────────────────────────────────────────
; {% block content %} — keyword is "block", next is identifier (block name)
(statement
  (keyword) @_kw (#eq? @_kw "block")
  (identifier) @code_struct.name
) @code_struct.def

; ── Variable references → variable nodes ─────────────────────────────────────
; {{ my_var }} — top-level identifier in an expression
; The identifier field is named on the expression node.
(expression
  identifier: (identifier) @code_variable.name
) @code_variable.def
