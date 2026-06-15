; wicked_estate Bicep extraction queries — W9.3 IaC.
; Node-types verified against tree-sitter-bicep 1.1.0.
;
; resource_declaration / module_declaration: 'resource'/'module' <identifier> ...
;   The identifier is the first named child (no named field for it).
; parameter_declaration: 'param' <identifier> <type> ...
; output_declaration: 'output' <identifier> <type> ...
; variable_declaration: 'var' <identifier> ...
; user_defined_function: has field name: (identifier).
; call_expression: field function: (_).
; import_statement: first child is a string (no named identifier for source).

; ── Resource declarations (IaC resources — W9.3 core) ─────────────────────────
(resource_declaration
  (identifier) @code_resource.name
) @code_resource.def

; ── Module declarations (Bicep module references) ─────────────────────────────
(module_declaration
  (identifier) @code_module.name
) @code_module.def

; ── Parameter declarations ─────────────────────────────────────────────────────
(parameter_declaration
  (identifier) @code_parameter.name
) @code_parameter.def

; ── Output declarations ───────────────────────────────────────────────────────
(output_declaration
  (identifier) @code_output.name
) @code_output.def

; ── Variable declarations ─────────────────────────────────────────────────────
(variable_declaration
  (identifier) @code_variable.name
) @code_variable.def

; ── User-defined functions (has explicit name field) ─────────────────────────
(user_defined_function
  name: (identifier) @code_function.name
) @code_function.def

; ── Call expressions ──────────────────────────────────────────────────────────
(call_expression
  function: (identifier) @call.function
) @call

; ── Import statements ─────────────────────────────────────────────────────────
; import 'path'
(import_statement
  (string) @import.source
) @import

; import * as alias from 'path'  /  import { name } from 'path'
(import_with_statement
  (string) @import.source
) @import

; import functionality from 'path'
(import_functionality
  (string) @import.source
) @import
