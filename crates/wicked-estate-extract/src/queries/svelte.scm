; wicked_estate Svelte extraction queries — @code_* convention.
;
; Verified against arborium-svelte-2.18.0 node-types.json.
; Svelte files have no functions at tree-sitter level — the grammar tracks
; template structure: document > element | script_element | style_element.
; We capture the script_element (the <script> block) and HTML elements by tag name.
;
; Named nodes used:
;   script_element — children: start_tag, raw_text, end_tag
;   element        — children: start_tag, ...
;   start_tag      — children: tag_name, attribute, ...
;   tag_name       — leaf (named)

; ── Script element (the <script> block is the main code unit) ─────────────────
; Capture the script block as a "module" definition.
(script_element
  (start_tag
    (tag_name) @code_module.name)
) @code_module.def

; ── HTML elements by tag name ────────────────────────────────────────────────
; Top-level elements like <main>, <header>, <section> etc.
(element
  (start_tag
    (tag_name) @code_struct.name)
) @code_struct.def
