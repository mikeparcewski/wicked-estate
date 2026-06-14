; wicked_estate CSS extraction queries — written fresh (no prior art equivalent).
; CSS has no functions/imports in the traditional sense. We capture:
;   - rule_set blocks (selectors) as type definitions
;   - @keyframes blocks as function definitions
;   - @import statements as import nodes
;
; Each definition pattern MUST pair a @code_<kind>.def anchor with a
; @code_<kind>.name capture so the extractor can emit a named node.

; Rule sets: `.foo { ... }`, `#bar { ... }`, `div { ... }`
; The selectors text becomes the node name.
(rule_set
  (selectors) @code_type.name
) @code_type.def

; @keyframes — name comes from keyframes_name child
(keyframes_statement
  (keyframes_name) @code_function.name
) @code_function.def

; @import — the string_value is the import path
(import_statement
  (string_value) @import.source
) @import
