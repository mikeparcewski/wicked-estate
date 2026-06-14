; wicked_estate PowerShell extraction queries — @code_* convention.
;
; Verified against arborium-powershell-2.18.0 node-types.json.
; Named nodes used:
;   function_statement — children: function_name (named, no sub-children), script_block
;   class_statement    — children: simple_name (named, no sub-children), ...

; ── Function definitions ──────────────────────────────────────────────────────
; function_statement has a function_name child (named node, leaf).
(function_statement
  (function_name) @code_function.name
) @code_function.def

; ── Class definitions ─────────────────────────────────────────────────────────
; class_statement has a simple_name child (named node, leaf).
(class_statement
  (simple_name) @code_class.name
) @code_class.def
