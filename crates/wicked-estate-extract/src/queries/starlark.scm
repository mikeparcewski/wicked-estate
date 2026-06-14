; wicked_estate Starlark (Bazel/Buck .bzl/.star) extraction queries — @code_* convention.
;
; Grammar: arborium-starlark 2.18.0 — node types verified against
; grammar/src/node-types.json AND the live parse tree. Starlark is Python-shaped
; (`function_definition`, `call`, `attribute`), with key differences:
;   - NO classes (Starlark has no class_definition).
;   - Imports use `load_statement` (children: `aliased_load` | `string`), NOT a
;     Python import_statement.  load("//pkg:defs.bzl", "sym", alias = "other")
;   - CALLEE WRAPPING: the `call` node's `function:` field is typed `primary_expression`
;     (not a bare `identifier`). A bare `(identifier)` there is a Structure error — the
;     callee must be reached THROUGH a `(primary_expression ...)` node. Same for the
;     `attribute` form. Verified from node-types.json (`call.function` -> primary_expression)
;     and the parse tree: `call function: (primary_expression (identifier))`.

; Function definitions (top-level and nested)
(function_definition
  name: (identifier) @code_function.name
  body: (block) @code_function.body
) @code_function.def

; Module-level UPPER_CASE constants:  FOO = ...
; `assignment` is nested under `expression_statement` (not a direct module child).
(module
  (expression_statement
    (assignment
      left: (identifier) @code_constant.name))
  (#match? @code_constant.name "^[A-Z][A-Z0-9_]*$")
) @code_constant.def

; load() statements — the .bzl source path is the FIRST string child.
;   load("//pkg:defs.bzl", "a", b = "c")
(load_statement
  .
  (string) @import.source
) @import

; Function calls — simple:  foo()
; The callee sits inside a `primary_expression` wrapper (grammar field type).
(call
  function: (primary_expression
    (identifier) @call.function)
) @call

; Method / attribute calls:  obj.foo()
; `attribute` is also wrapped in `primary_expression` under `function:`.
(call
  function: (primary_expression
    (attribute
      attribute: (identifier) @call.method))
) @call.method
