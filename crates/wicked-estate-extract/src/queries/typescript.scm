; wicked_estate TypeScript extraction queries — @code_* convention.

; ── Callable definitions ─────────────────────────────────────────────────────

; Function declarations
(function_declaration
  name: (identifier) @code_function.name
  parameters: (formal_parameters) @code_function.params
  body: (statement_block) @code_function.body
) @code_function.def

; Arrow functions assigned to a const/let/var binding (module-scope and class-field)
(variable_declarator
  name: (identifier) @code_function.name
  value: (arrow_function)
) @code_function.def

; Method definitions: covers regular, async, static, get, set, and constructor
; (they all parse as method_definition with property_identifier name)
(method_definition
  name: (property_identifier) @code_method.name
  parameters: (formal_parameters) @code_method.params
  body: (statement_block) @code_method.body
) @code_method.def

; Arrow-function class fields: handler = () => {}
; public_field_definition with an arrow_function value — these are methods too
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
  value: (_) @code_type.value
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

; ── ORM / framework-aware extraction (W6.2) ──────────────────────────────────
;
; TypeORM @Entity decorated class → NodeKind::Class
; Gate: the class_declaration must carry a @Entity() (or @Entity("table_name")) decorator.
; Uses the `decorator:` named field on class_declaration (tree-sitter-typescript 0.23+).

(class_declaration
  decorator: (decorator
    (call_expression
      function: (identifier) @_entity_dec
      (#eq? @_entity_dec "Entity")))
  name: (type_identifier) @code_class.name
) @code_class.def

; TypeORM column-like decorated property → NodeKind::Field
; Gate: property must carry one of the recognised TypeORM column decorators.
; public_field_definition carries `decorator:` as a named field.

(public_field_definition
  decorator: (decorator
    (call_expression
      function: (identifier) @_col_dec
      (#any-of? @_col_dec
        "Column"
        "PrimaryColumn"
        "PrimaryGeneratedColumn"
        "CreateDateColumn"
        "UpdateDateColumn"
        "DeleteDateColumn"
        "VersionColumn"
        "ViewColumn"
        "ObjectIdColumn"
        "JoinColumn"
        "JoinTable"
        "OneToOne"
        "OneToMany"
        "ManyToOne"
        "ManyToMany")))
  name: (property_identifier) @code_field.name
) @code_field.def

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
; The optional_chain variant also uses member_expression with property_identifier
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
