; wicked_estate Fortran extraction queries.
; Node-types verified against tree-sitter-fortran 0.6.0.
;
; Grammar structure:
;   function        — contains function_statement (field name: (name))
;   subroutine      — contains subroutine_statement (field name: (name))
;   module          — contains module_statement (name is bare (name) child, no field)
;   program         — contains program_statement (name is bare (name) child, no field)
;   derived_type_definition — contains derived_type_statement
;                             which has _type_name aliased as (type_name), no named field
;   call_expression — field function: (_); field arguments: (_)
;   use_statement   — Fortran USE <module> → import
;
; Name nodes: function_statement/subroutine_statement have field name: (name).
; module_statement/program_statement have (name) as a positional child (no named field).
; Use (_) wildcard captures for positional name children.

; ── Function definitions ──────────────────────────────────────────────────────
(function
  (function_statement
    name: (name) @code_function.name)
) @code_function.def

; ── Subroutine definitions ────────────────────────────────────────────────────
(subroutine
  (subroutine_statement
    name: (name) @code_function.name)
) @code_function.def

; ── Module definitions ────────────────────────────────────────────────────────
(module
  (module_statement
    (name) @code_module.name)
) @code_module.def

; ── Program definitions ───────────────────────────────────────────────────────
(program
  (program_statement
    (name) @code_module.name)
) @code_module.def

; ── Derived type (struct) definitions ────────────────────────────────────────
(derived_type_definition
  (derived_type_statement
    (type_name) @code_type.name)
) @code_type.def

; ── Call expressions ──────────────────────────────────────────────────────────
(call_expression
  function: (_) @call.function
) @call

; ── Subroutine calls (CALL stmt) ──────────────────────────────────────────────
(subroutine_call
  subroutine: (_) @call.function
) @call

; ── USE statements (imports) ──────────────────────────────────────────────────
; use_statement children: module_name, use_alias, included_items
(use_statement
  (module_name) @import.source
) @import
