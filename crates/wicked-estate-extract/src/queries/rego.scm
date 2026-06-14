; wicked_estate Rego (OPA) extraction queries — @code_* convention.
;
; Verified against arborium-rego-2.18.0 node-types.json.
; Named nodes used:
;   module   — root; children: package, import, policy
;   policy   — children: rule
;   rule     — children: rule_head, rule_body
;   rule_head — children: var, rule_args, rule_head_comp, term
;   var      — leaf (a variable/rule name)
;   import   — children (none with fields; raw text used)

; ── Rule definitions (rule_head → var = the rule name) ───────────────────────
(rule
  (rule_head
    (var) @code_function.name)
) @code_function.def

; ── Import statements ────────────────────────────────────────────────────────
(import) @import
