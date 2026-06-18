; wicked_estate VBA extraction queries — @code_* convention.
; Verified against tmepple/tree-sitter-vba node-types.json.
;
; VBA has no explicit class-block node — the whole module is the class/module.
; The class_header node (VERSION 1.0 CLASS...) is opaque metadata; module
; identity is resolved at the file level outside the query.

; ── Sub definitions ───────────────────────────────────────────────────────────
(sub_declaration
  name: (identifier) @code_function.name
) @code_function.def

; ── Function definitions ──────────────────────────────────────────────────────
(function_declaration
  name: (identifier) @code_function.name
) @code_function.def

; ── Property definitions (Get / Let / Set) ────────────────────────────────────
(property_declaration
  name: (identifier) @code_property.name
) @code_property.def

; ── Call sites ────────────────────────────────────────────────────────────────
; call_statement.target can be: identifier (bare call) or index_expression (call
; with args — VBA grammar wraps Helper(42) as index_expression{object: identifier}).
; Decompose each form to capture just the callee name.

; bare call: Helper or Call Helper
(call_statement
  target: (identifier) @call.function
) @call

; call with args: Helper(42) → index_expression { object: identifier "Helper" }
(call_statement
  target: (index_expression
    object: (identifier) @call.function)
) @call

; method call without args: obj.Method
(call_statement
  target: (member_access_expression) @call.function
) @call

; method call with args: obj.Method(arg) → index_expression { object: member_access_expression }
(call_statement
  target: (index_expression
    object: (member_access_expression) @call.function)
) @call

; new_expression: New ClassName — object construction.
(new_expression
  (type_expression) @call.function
) @call
