; wicked_estate ReScript extraction queries — @code_* convention.
;
; Verified against arborium-rescript-2.18.0 node-types.json.
; Named nodes used:
;   let_declaration  — children: let_binding
;   let_binding      — field pattern: value_identifier, field body: expression
;   module_declaration — children: module_binding (field name: module_identifier / type_identifier)
;   type_declaration   — children: type_binding (field name: type_identifier)
;   call_expression    — field function: expression

; ── Let bindings (functions and values) ──────────────────────────────────────
(let_declaration
  (let_binding
    pattern: (value_identifier) @code_function.name)
) @code_function.def

; ── Module declarations ───────────────────────────────────────────────────────
(module_declaration
  (module_binding
    name: (_) @code_module.name)
) @code_module.def

; ── Type declarations ─────────────────────────────────────────────────────────
(type_declaration
  (type_binding
    name: (_) @code_struct.name)
) @code_struct.def

; ── Function calls ────────────────────────────────────────────────────────────
(call_expression
  function: (_) @call.function
) @call
