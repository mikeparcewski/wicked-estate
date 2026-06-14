; wicked_estate JavaScript extraction queries — @code_* convention.

; ── Callable definitions ─────────────────────────────────────────────────────

; Function declarations
(function_declaration
  name: (identifier) @code_function.name
  parameters: (formal_parameters) @code_function.params
  body: (statement_block) @code_function.body
) @code_function.def

; Arrow functions assigned to variables
(variable_declarator
  name: (identifier) @code_function.name
  value: (arrow_function)
) @code_function.def

; Class declarations
(class_declaration
  name: (identifier) @code_class.name
  body: (class_body) @code_class.body
) @code_class.def

; Class expressions assigned to variables
(variable_declarator
  name: (identifier) @code_class.name
  value: (class
    body: (class_body) @code_class.body
  )
) @code_class.def

; Class extends clause (JS grammar: class_heritage has identifier directly, no extends_clause wrapper)
(class_declaration
  name: (identifier) @code_class.name
  (class_heritage (identifier) @code_extends.target)
) @code_extends.def

; Method definitions: covers regular, async, static, get, set, and constructor
; (all parse as method_definition — object-literal methods too)
(method_definition
  name: (property_identifier) @code_method.name
  parameters: (formal_parameters) @code_method.params
  body: (statement_block) @code_method.body
) @code_method.def

; Arrow-function class fields: handler = () => {}
; JS grammar uses field_definition with property: field (TS uses public_field_definition with name:)
(field_definition
  property: (property_identifier) @code_method.name
  value: (arrow_function)
) @code_method.def

; ── Scoped constant/variable capture ────────────────────────────────────────
; Only MEANINGFUL declarations are captured:
;   (a) top-level — lexical_declaration/variable_declaration that are direct
;       children of (program …)
;   (b) exported — inside (export_statement …)
; Function-local bindings (inside statement_block) are intentionally excluded.

; Top-level const declarations
(program
  (lexical_declaration kind: "const"
    (variable_declarator
      name: (identifier) @code_constant.name)
  ) @code_constant.def)

; Exported const declarations
(export_statement
  (lexical_declaration kind: "const"
    (variable_declarator
      name: (identifier) @code_constant.name)
  ) @code_constant.def)

; Top-level let declarations
(program
  (lexical_declaration kind: "let"
    (variable_declarator name: (identifier) @code_variable.name)
  ) @code_variable.def)

; Exported let declarations
(export_statement
  (lexical_declaration kind: "let"
    (variable_declarator name: (identifier) @code_variable.name)
  ) @code_variable.def)

; Top-level var declarations (legacy)
(program
  (variable_declaration
    (variable_declarator name: (identifier) @code_variable.name)
  ) @code_variable.def)

; Exported var declarations (legacy)
(export_statement
  (variable_declaration
    (variable_declarator name: (identifier) @code_variable.name)
  ) @code_variable.def)

; ── Import statements ────────────────────────────────────────────────────────

(import_statement
  source: (string) @import.source
) @import

; ── Call sites ───────────────────────────────────────────────────────────────

; Function calls — simple: foo()
(call_expression
  function: (identifier) @call.function
  arguments: (arguments) @call.args
) @call

; Method calls — member expression: a.b(), a.b.c(), a?.b()
(call_expression
  function: (member_expression
    property: (property_identifier) @call.method
  )
) @call.method

; Constructor calls — new X()
(new_expression
  constructor: (identifier) @call.function
) @call

; Constructor calls — new a.B()
(new_expression
  constructor: (member_expression
    property: (property_identifier) @call.method
  )
) @call.method
