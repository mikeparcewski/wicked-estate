; wicked_estate MATLAB extraction queries — @code_* convention.
;
; Verified against arborium-matlab-2.18.0 node-types.json.
; Named nodes used:
;   function_definition — field name: identifier | property_name
;   class_definition    — field name: identifier

; ── Function definitions ──────────────────────────────────────────────────────
(function_definition
  name: (identifier) @code_function.name
) @code_function.def

; ── Class definitions ─────────────────────────────────────────────────────────
(class_definition
  name: (identifier) @code_class.name
) @code_class.def
