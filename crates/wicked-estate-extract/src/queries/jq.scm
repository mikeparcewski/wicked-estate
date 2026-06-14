; wicked_estate jq extraction queries — @code_* convention.
;
; Verified against arborium-jq-2.18.0 node-types.json.
; Named nodes used:
;   funcdef     — children: identifier (name), funcdefargs, query
;   identifier  — leaf
;   import_     — children: string (path), identifier/variable (alias)

; ── Function definitions ──────────────────────────────────────────────────────
(funcdef
  (identifier) @code_function.name
) @code_function.def

; ── Import statements ─────────────────────────────────────────────────────────
(import_
  (string) @import.source
) @import
