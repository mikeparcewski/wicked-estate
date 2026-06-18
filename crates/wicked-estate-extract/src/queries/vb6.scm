; wicked_estate VB6 extraction queries — @code_* convention.
; Verified against joannefan/tree-sitter-vb6 (2026-03).
;
; Node name changes vs andersonm3ai 0.0.2:
;   sub_definition       → sub_declaration
;   function_definition  → function_declaration
;   property_definition  → property_declaration
;   call_statement       → method_invocation  (target: name_access | member_access)
;   function_call        → function_invocation (target: name_access | member_access)
;   implements_statement → implements_declaration
;
; Key fixes in this grammar:
;   - Call keyword is a first-class branch of method_invocation (no longer misread as callee name)
;   - Sub/Function bodies use REPEAT (not REPEAT1) — empty bodies parse correctly
;   - attribute_line exposes VB_Name for module name extraction

; ── Module name (from Attribute VB_Name = "...") ─────────────────────────────
; .cls and .frm files declare the module's identity via this attribute.
(attribute_line
  (identifier) @_attr
  (string) @code_module.name
  (#eq? @_attr "VB_Name")) @code_module.def

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
; method_invocation covers all bare-call forms:
;   Fn              (no args, no parens)
;   Fn arg1, arg2   (args, no parens)
;   Call Fn         (explicit Call, no args)
;   Call Fn arg     (explicit Call with args)
;   Call Obj.Method(...) (member access)
; target field is aliased as name_access (identifier) or member_access.
(method_invocation
  target: (name_access) @call.function
) @call

(method_invocation
  target: (member_access) @call.function
) @call

; function_invocation covers parenthesised calls: Fn(args), Obj.Method(args)
(function_invocation
  target: (name_access) @call.function
) @call

(function_invocation
  target: (member_access) @call.function
) @call

; ── Heritage ──────────────────────────────────────────────────────────────────
; VB6 class modules declare Implements <Interface> in the header section.
(implements_declaration
  (identifier) @code_implements.target
) @code_implements.def
