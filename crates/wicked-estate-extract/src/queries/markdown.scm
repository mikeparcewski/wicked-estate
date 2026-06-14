; wicked_estate Markdown extraction queries — @code_* convention.
;
; Verified against tree-sitter-md-0.3.2 (block grammar) node-types.json.
; Markdown has no functions or classes.  We treat headings as module-level
; definitions (the heading text becomes the symbol name).
;
; Named nodes used (block grammar):
;   atx_heading    — field heading_content: (inline)
;   setext_heading — field heading_content: (paragraph)
;   fenced_code_block — child: (info_string) (language of code block)
;
; We capture ATX headings (# Foo) and setext headings (Foo / ===).
; The inline/paragraph child text becomes the symbol name.

; ── ATX headings: # Foo, ## Bar ──────────────────────────────────────────────
(atx_heading
  heading_content: (inline) @code_module.name
) @code_module.def

; ── Setext headings: Foo / === or Foo / --- ──────────────────────────────────
(setext_heading
  heading_content: (paragraph) @code_module.name
) @code_module.def
