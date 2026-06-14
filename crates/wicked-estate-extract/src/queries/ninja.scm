; wicked_estate Ninja build extraction queries — @code_* convention.
;
; Verified against arborium-ninja-2.18.0 node-types.json.
; Named nodes used:
;   rule   — field name: identifier
;   pool   — field name: identifier
;   let    — field name: identifier (variable assignment)

; ── Rule definitions ──────────────────────────────────────────────────────────
(rule
  name: (identifier) @code_function.name
) @code_function.def

; ── Pool definitions ──────────────────────────────────────────────────────────
(pool
  name: (identifier) @code_variable.name
) @code_variable.def

; ── Top-level variable assignments ───────────────────────────────────────────
(let
  name: (identifier) @code_variable.name
) @code_variable.def
