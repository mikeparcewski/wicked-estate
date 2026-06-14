; wicked_estate Meson build extraction queries — @code_* convention.
;
; Verified against arborium-meson-2.18.0 node-types.json.
; Named nodes used:
;   normal_command  — field command: identifier (the function name)
;   expression_statement — field object: identifier (variable name)
;
; Meson "functions" like project(), executable(), custom_target() are
; normal_command nodes.  Variable assignments appear as expression_statement
; with object: identifier.

; ── Function call statements (project, executable, library, …) ───────────────
(normal_command
  command: (identifier) @code_function.name
) @code_function.def

; ── Variable assignments ──────────────────────────────────────────────────────
(expression_statement
  object: (identifier) @code_variable.name
) @code_variable.def
