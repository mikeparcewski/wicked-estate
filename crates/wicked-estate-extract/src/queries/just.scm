; wicked_estate Justfile extraction queries — @code_* convention.
;
; Verified against arborium-just-2.18.0 node-types.json.
; Named nodes used:
;   recipe          — children: attribute, recipe_header, recipe_body
;   recipe_header   — field name: identifier
;   assignment      — field left: identifier, field right: expression
;   alias           — field left: identifier

; ── Recipe definitions ────────────────────────────────────────────────────────
(recipe
  (recipe_header
    name: (identifier) @code_function.name)
) @code_function.def

; ── Variable assignments ──────────────────────────────────────────────────────
(assignment
  left: (identifier) @code_variable.name
) @code_variable.def
