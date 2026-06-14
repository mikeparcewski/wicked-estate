; wicked_estate Kotlin extraction queries — @code_* convention.
;
; Verified against tree-sitter-kotlin-ng-1.1.0 node-types.json.
; Named nodes used:
;   class_declaration    — field name: (identifier)
;   function_declaration — field name: (identifier)
;   object_declaration   — field name: (identifier) (singleton objects)
;   type_alias           — field type: (identifier) (note: this is the ALIAS name)
;   import               — children: identifier | qualified_identifier
;   call_expression      — children: expression + value_arguments
;   navigation_expression — children: expression + identifier (method name)
;   variable_declaration — children include identifier (the variable name)
;   delegation_specifiers / user_type / identifier (heritage)
;
; NOTE: tree-sitter-kotlin-ng uses "identifier" for all names (not simple_identifier).

; ── Class declarations ────────────────────────────────────────────────────────
(class_declaration
  name: (identifier) @code_class.name
) @code_class.def

; ── Object declarations (singletons) ─────────────────────────────────────────
(object_declaration
  name: (identifier) @code_class.name
) @code_class.def

; ── Function declarations ─────────────────────────────────────────────────────
; function_body is optional (expression body `= expr` also compiles to function_body
; in the grammar, but we omit the body capture to handle all declaration forms).
(function_declaration
  name: (identifier) @code_function.name
) @code_function.def

; ── Property declarations (variables) ────────────────────────────────────────
; property_declaration child: variable_declaration child: identifier
(property_declaration
  (variable_declaration
    (identifier) @code_variable.name)
) @code_variable.def

; ── Heritage: class Foo : Bar(), IFoo ────────────────────────────────────────
; delegation_specifier > user_type > identifier
(class_declaration
  name: (identifier) @code_class.name
  (delegation_specifiers
    (delegation_specifier
      (user_type
        (identifier) @code_extends.target)))
) @code_extends.def

; constructor_invocation form: class A : Base(…)
(class_declaration
  name: (identifier) @code_class.name
  (delegation_specifiers
    (delegation_specifier
      (constructor_invocation
        (user_type
          (identifier) @code_extends.target))))
) @code_extends.def

; ── Import declarations ───────────────────────────────────────────────────────
; (import (identifier)) or (import (qualified_identifier))
(import
  (identifier) @import.source
) @import

(import
  (qualified_identifier) @import.source
) @import

; ── Direct call expressions: foo(…) ──────────────────────────────────────────
; call_expression children: expression (the callee) + value_arguments
(call_expression
  (identifier) @call.function
  (value_arguments)
) @call

; ── Navigation/method calls: foo.bar(…) ──────────────────────────────────────
; navigation_expression children: expression + identifier (the method name)
(call_expression
  (navigation_expression
    (identifier) @call.method)
  (value_arguments)
) @call.method
