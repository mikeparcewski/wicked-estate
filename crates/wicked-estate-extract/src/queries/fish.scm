; wicked_estate Fish Shell extraction queries — @code_* convention.
;
; Verified against arborium-fish-2.18.0 node-types.json.
; Named nodes used:
;   function_definition — field name: (concatenation/double_quote_string/...)
;   command             — field name: (...)

; ── Function definitions ──────────────────────────────────────────────────────
(function_definition
  name: (_) @code_function.name
) @code_function.def

; ── Command invocations ───────────────────────────────────────────────────────
(command
  name: (_) @call.function
) @call
