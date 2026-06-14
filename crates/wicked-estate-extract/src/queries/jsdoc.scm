; wicked_estate JSDoc extraction queries — @code_* convention.
;
; Verified against arborium-jsdoc-2.18.0 node-types.json.
; JSDoc is documentation, not source code; its "primary construct" is a tag.
; Named nodes used:
;   tag      — children: tag_name, description, expression, optional_identifier, type
;   tag_name — leaf (e.g. "@param", "@returns")
;   document — root; children: description, tag

; ── Tag definitions ───────────────────────────────────────────────────────────
; Capture each JSDoc tag as a "variable" definition keyed by the tag_name.
(tag
  (tag_name) @code_variable.name
) @code_variable.def
