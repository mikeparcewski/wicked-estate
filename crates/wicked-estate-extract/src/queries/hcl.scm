; wicked_estate HCL/Terraform extraction queries — @code_* convention.
;
; Verified against tree-sitter-hcl-1.1.0 node-types.json.
; HCL blocks (resource/data/module/variable/output/locals/provider/…) become
; struct definitions.  Function calls (built-in HCL functions like toset(),
; lookup(), …) are captured as call refs.
;
; Named nodes used:
;   block           — children: identifier+ (labels) + body
;   attribute       — children: identifier (key) + expression (value)
;   function_call   — children: identifier (function name) + function_arguments
;   body            — children: attribute | block
;   config_file     — root node

; ── Block definitions (resource, variable, module, data, output, …) ──────────
; A block has one or more identifier children before the body.
; The FIRST identifier is the block type (e.g. "resource"); subsequent
; identifiers and string_lits are the labels (e.g. "aws_instance" "main").
; We capture the entire block as a struct definition, naming it by its first
; identifier child (the block type keyword).
(block
  (identifier) @code_struct.name
  (body) @code_struct.body
) @code_struct.def

; ── Attribute definitions (key = value) ───────────────────────────────────────
; Capture top-level attributes as variable definitions.
(attribute
  (identifier) @code_variable.name
) @code_variable.def

; ── Function calls (HCL built-ins: toset, lookup, merge, …) ─────────────────
(function_call
  (identifier) @call.function
) @call
