; wicked_estate AWK extraction queries — @code_* convention.
;
; Verified against arborium-awk-2.18.0 node-types.json.
; Named nodes used:
;   func_def  — fields: name (identifier/ns_qualified_name)
;   func_call — fields: name

; ── Function definitions ──────────────────────────────────────────────────────
(func_def
  name: (_) @code_function.name
) @code_function.def

; ── Function calls ───────────────────────────────────────────────────────────
(func_call
  name: (_) @call.function
) @call
