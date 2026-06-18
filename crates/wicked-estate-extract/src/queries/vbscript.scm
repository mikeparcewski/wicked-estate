; wicked_estate VBScript extraction queries — @code_* convention.
; Verified against JJK96/tree-sitter-vbscript node-types.json.
;
; VBScript has no class block — it's a scripting language. The `type_definition`
; node is an `As <type>` clause, not a class definition.

; ── Sub definitions ───────────────────────────────────────────────────────────
; The subroutine node's name is a new_identifier direct child (not a named field).
(subroutine
  (new_identifier) @code_function.name
) @code_function.def

; ── Function definitions ──────────────────────────────────────────────────────
(function
  (new_identifier) @code_function.name
) @code_function.def

; ── Declare statements (external DLL functions) ───────────────────────────────
(ptrsafe_function_declaration
  (new_identifier) @code_function.name
) @code_function.def

; ── Call sites: function_call (parenthesised) ────────────────────────────────
; function_call children include: identifier (the name) + argument_list.
(function_call
  (identifier) @call.function
) @call

; ── Call sites: invocation_statement (bare call form) ────────────────────────
; Simple bare calls: SomeSub arg1, arg2 → identifier is the call target.
(invocation_statement
  (identifier) @call.function
) @call

; Method calls: obj.Method arg → member_expression is the call target.
(invocation_statement
  (member_expression) @call.function
) @call

; ── Object construction ───────────────────────────────────────────────────────
; new_expression: Set x = New ClassName
(new_expression
  (identifier) @call.function
) @call
