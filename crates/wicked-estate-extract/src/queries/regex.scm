; wicked_estate Regex extraction queries — @code_* convention.
;
; Verified against arborium-regex-2.18.0 node-types.json.
; Regex files capture named capturing groups as definitions.
; Named nodes used:
;   named_capturing_group — children: group_name, pattern
;   group_name            — leaf (the capture name)

; ── Named capturing groups ────────────────────────────────────────────────────
; A named group (?P<name>...) or (?<name>...) defines a named capture.
(named_capturing_group
  (group_name) @code_variable.name
) @code_variable.def
