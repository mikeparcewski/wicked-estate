; wicked_estate INI extraction queries — @code_* convention.
;
; Verified against arborium-ini-2.18.0 node-types.json.
; Named nodes used:
;   section      — children: section_name, setting
;   section_name — leaf (the name between brackets)
;   setting      — children: setting_name, setting_value
;   setting_name — leaf

; ── Section definitions ───────────────────────────────────────────────────────
(section
  (section_name) @code_module.name
) @code_module.def

; ── Key/value settings ────────────────────────────────────────────────────────
(setting
  (setting_name) @code_variable.name
) @code_variable.def
