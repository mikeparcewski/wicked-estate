; wicked_estate Devicetree (DTS/DTSI) extraction queries — @code_* convention.
;
; Verified against arborium-devicetree-2.18.0 node-types.json.
; Named nodes used:
;   node        — field name: identifier (or reference)
;   preproc_def — field name: identifier
;   property    — field name: identifier

; ── Device nodes (the primary DTS construct) ──────────────────────────────────
(node
  name: (identifier) @code_module.name
) @code_module.def

; ── Preprocessor macro definitions ───────────────────────────────────────────
(preproc_def
  name: (identifier) @code_constant.name
) @code_constant.def

; ── Node properties ───────────────────────────────────────────────────────────
(property
  name: (identifier) @code_variable.name
) @code_variable.def
