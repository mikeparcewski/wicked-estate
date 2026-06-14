; wicked_estate Lean 4 extraction queries — @code_* convention.
;
; Verified against arborium-lean-2.18.0 node-types.json.
; Named nodes used:
;   def             — field name: identifier
;   theorem         — field name: identifier
;   abbrev          — field name: identifier
;   axiom           — field name: identifier
;   class_inductive — field name: identifier

; ── Definitions (def / abbrev) ────────────────────────────────────────────────
(def
  name: (identifier) @code_function.name
) @code_function.def

(abbrev
  name: (identifier) @code_function.name
) @code_function.def

; ── Theorems / axioms ────────────────────────────────────────────────────────
(theorem
  name: (identifier) @code_function.name
) @code_function.def

(axiom
  name: (identifier) @code_constant.name
) @code_constant.def

; ── Class / structure definitions ────────────────────────────────────────────
(class_inductive
  name: (identifier) @code_class.name
) @code_class.def
