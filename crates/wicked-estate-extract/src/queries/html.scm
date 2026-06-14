; wicked_estate HTML extraction queries — @code_* convention.
; Capture names follow the @code_* convention.
; HTML has no functions or imports; we capture element tag names as type definitions
; and script/style blocks as function-shaped nodes with the tag name as identifier.

; Element definitions (open tags) — tag_name is child of start_tag which is child of element
(element
  (start_tag
    (tag_name) @code_type.name)
) @code_type.def

; Self-closing elements
(element
  (self_closing_tag
    (tag_name) @code_type.name)
) @code_type.def

; Script elements — tag name "script" captured as the function name
(script_element
  (start_tag
    (tag_name) @code_function.name)
) @code_function.def

; Style elements — tag name "style" captured as the function name
(style_element
  (start_tag
    (tag_name) @code_function.name)
) @code_function.def
