; wicked_estate Vue extraction queries — @code_* convention.
;
; Verified against arborium-vue-2.18.0 node-types.json.
; Vue SFCs have script_element, template_element, style_element as top-level blocks.
; The grammar treats the document as HTML-like; no function-level parsing.
;
; Named nodes used:
;   script_element   — children: start_tag, raw_text, end_tag
;   template_element — children: (various)
;   element          — children: start_tag, end_tag, ...
;   start_tag        — children: tag_name, attribute, ...
;   tag_name         — leaf (named)

; ── Script element (the <script> block) ──────────────────────────────────────
(script_element
  (start_tag
    (tag_name) @code_module.name)
) @code_module.def

; ── Template element (the <template> block) ──────────────────────────────────
(template_element
  (start_tag
    (tag_name) @code_struct.name)
) @code_struct.def
