; wicked_estate Make extraction queries — @code_* convention.
;
; Verified against arborium-make-2.18.0 node-types.json.
; Makefiles have rules (targets) and variable assignments as meaningful units.
;
; Named nodes used:
;   rule                — children: targets, recipe; fields: target, normal, order_only
;   variable_assignment — field name: word
;
; targets node has no name field; we use a wildcard child capture.

; ── Rules (targets) ───────────────────────────────────────────────────────────
; A rule's targets are in the `targets` child node.
; The targets node has no direct name field — capture first child as name.
(rule
  (targets
    (_) @code_struct.name)
) @code_struct.def

; ── Variable assignments ─────────────────────────────────────────────────────
(variable_assignment
  name: (word) @code_variable.name
) @code_variable.def
