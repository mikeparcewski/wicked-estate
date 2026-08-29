; wicked_estate TSX extraction queries — @code_* convention.
; TSX shares the TypeScript grammar (LANGUAGE_TSX) so node types are identical.

; ── Callable definitions ─────────────────────────────────────────────────────

; Function declarations
(function_declaration
  name: (identifier) @code_function.name
  parameters: (formal_parameters) @code_function.params
  body: (statement_block) @code_function.body
) @code_function.def

; Arrow functions assigned to variable bindings
(variable_declarator
  name: (identifier) @code_function.name
  value: (arrow_function)
) @code_function.def

; Method definitions: covers regular, async, static, get, set, and constructor
(method_definition
  name: (property_identifier) @code_method.name
  parameters: (formal_parameters) @code_method.params
  body: (statement_block) @code_method.body
) @code_method.def

; Arrow-function class fields: handler = () => {}
(public_field_definition
  name: (property_identifier) @code_method.name
  value: (arrow_function)
) @code_method.def

; Interface method signatures: interface Foo { bar(x: T): R; }
(interface_body
  (method_signature
    name: (property_identifier) @code_method.name
    parameters: (formal_parameters) @code_method.params
  ) @code_method.def
)

; Class declarations
(class_declaration
  name: (type_identifier) @code_class.name
  body: (class_body) @code_class.body
) @code_class.def

; Class extends clause
(class_declaration
  name: (type_identifier) @code_class.name
  (class_heritage
    (extends_clause value: (identifier) @code_extends.target))
) @code_extends.def

; Class implements clause
(class_declaration
  name: (type_identifier) @code_class.name
  (class_heritage
    (implements_clause (type_identifier) @code_implements.target))
) @code_implements.def

; Interface declarations
(interface_declaration
  name: (type_identifier) @code_interface.name
  body: (interface_body) @code_interface.body
) @code_interface.def

; Interface extends interface
(interface_declaration
  name: (type_identifier) @code_interface.name
  (extends_type_clause (type_identifier) @code_extends.target)
) @code_extends.def

; Type alias declarations
(type_alias_declaration
  name: (type_identifier) @code_type.name
) @code_type.def

; Enum declarations
(enum_declaration
  name: (identifier) @code_enum.name
) @code_enum.def

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

; Re-exports with a source are imports too: `export * from './y'`, `export { a } from './y'`.
(export_statement
  source: (string) @import.source
) @import

; Dynamic import: `import('./dyn')` — the grammar exposes `import` as a callable keyword.
(call_expression
  function: (import)
  arguments: (arguments . (string) @import.source)
) @import

; CommonJS require: `require('./z')` — gated on the callee text so ordinary calls never match.
(call_expression
  function: (identifier) @_req
  arguments: (arguments . (string) @import.source)
  (#eq? @_req "require")
) @import

; TS import-equals: `import r = require('./req')`.
(import_statement
  (import_require_clause
    source: (string) @import.source)
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
