; wicked_estate Zig extraction queries — @code_* convention.
;
; Verified against arborium-zig-2.18.0 node-types.json.
; Named nodes used:
;   FnProto — field function: IDENTIFIER (the function name)
;   VarDecl — field variable_type_function: IDENTIFIER (const/var name)

; ── Function prototypes / declarations (fn foo(...) ...) ──────────────────────
; FnProto.function field contains the IDENTIFIER (function name).
(FnProto
  function: (IDENTIFIER) @code_function.name
) @code_function.def

; ── Variable / constant declarations (const/var foo = ...) ───────────────────
(VarDecl
  variable_type_function: (IDENTIFIER) @code_variable.name
) @code_variable.def
