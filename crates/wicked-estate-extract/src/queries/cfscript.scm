; wicked_estate CFScript extraction queries — @code_* convention.
; Node types verified against cfmleditor/tree-sitter-cfml (cfscript grammar). CFScript is the
; modern, JS-shaped CFML dialect: `component { function foo() {} }`. Mirrors the grammar's own
; queries/tags.scm. (Script components are typically named by file, so no component-name capture.)

; ── Function / method definitions ─────────────────────────────────────────────
(function_declaration
  name: (identifier) @code_function.name) @code_function.def

(function_expression
  name: (identifier) @code_function.name) @code_function.def

(method_definition
  name: (property_identifier) @code_function.name) @code_function.def

; ── Function calls ────────────────────────────────────────────────────────────
(call_expression
  function: (identifier) @call.function) @call

(call_expression
  function: (member_expression
    property: (property_identifier) @call.function)) @call
