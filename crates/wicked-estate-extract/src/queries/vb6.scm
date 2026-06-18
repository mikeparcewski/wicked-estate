; wicked_estate VB6 extraction queries — @code_* convention.
; Verified against tree-sitter-vb6 0.0.2 (andersonm3ai) node-types.json.
;
; VB6 has no class-block node — the whole file is the module.
; Module kind (Module vs Class) is determined by file extension (.bas → Module;
; .cls/.frm/.ctl → Class) and is resolved at the Extractor layer.

; ── Sub definitions ───────────────────────────────────────────────────────────
(sub_definition
  name: (identifier) @code_function.name
) @code_function.def

; ── Function definitions ──────────────────────────────────────────────────────
(function_definition
  name: (identifier) @code_function.name
) @code_function.def

; ── Property definitions (Get / Let / Set) ────────────────────────────────────
(property_definition
  name: (identifier) @code_property.name
) @code_property.def

; ── Call sites ────────────────────────────────────────────────────────────────
; function_call: NAME(args) — standard parenthesised call.
(function_call
  name: (qualified_identifier) @call.function
) @call

; call_statement: NAME args — bare call form (no parens).
(call_statement
  name: (identifier) @call.function
) @call

; ── Heritage ──────────────────────────────────────────────────────────────────
; VB6 class modules declare Implements <Interface> in the header section.
(implements_statement
  (identifier) @code_implements.target
) @code_implements.def
