; wicked_estate GLSL extraction queries — @code_* convention.
;
; Verified against arborium-glsl-2.18.0 node-types.json.
; Named nodes used:
;   function_definition — field declarator: _declarator (contains function_declarator -> declarator)
;   struct_specifier    — field name: type_identifier
;   call_expression     — field function: expression

; ── Function definitions ──────────────────────────────────────────────────────
; function_definition.declarator -> function_declarator.declarator -> type_identifier
(function_definition
  declarator: (function_declarator
    declarator: (_) @code_function.name)
) @code_function.def

; ── Struct definitions ────────────────────────────────────────────────────────
(struct_specifier
  name: (type_identifier) @code_struct.name
) @code_struct.def

; ── Function calls ───────────────────────────────────────────────────────────
(call_expression
  function: (_) @call.function
) @call
