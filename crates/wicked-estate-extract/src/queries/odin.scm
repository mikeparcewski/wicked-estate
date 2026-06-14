; wicked_estate Odin extraction queries — @code_* convention.
;
; Verified against arborium-odin-2.18.0 node-types.json.
; In Odin, procedures and constants are defined via assignment_statement or
; variable_declaration containing a procedure child.  Fields on these nodes
; have no typed name field; we capture the whole node and use a wildcard for
; the name child (the first expression child = identifier).
;
; Named nodes used:
;   variable_declaration  — children: attributes, expression, procedure
;   assignment_statement  — children: attributes, expression, procedure
;   call_expression       — field function: expression
;
; We capture variable_declaration (the main way to declare procs/consts in Odin)
; and extract the first expression child as the name.

; ── Variable / procedure declarations ────────────────────────────────────────
(variable_declaration
  (expression) @code_variable.name
) @code_variable.def

; ── Assignment statements (also used for proc bindings at package level) ──────
(assignment_statement
  (expression) @code_variable.name
) @code_variable.def

; ── Function calls ────────────────────────────────────────────────────────────
(call_expression
  function: (_) @call.function
) @call
