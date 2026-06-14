; wicked_estate Apex (Salesforce) extraction queries — @code_* convention.
;
; Grammar: tree-sitter-sfapex 3.0.0 (module `apex`), descends from tree-sitter-java —
; node types verified against apex/src/node-types.json.
; Apex has NO import statements (namespaces are implicit), so there are no @import captures.
; Apex-specific: `trigger_declaration` (a top-level named handler bound to an SObject).

; Class declarations
(class_declaration
  name: (identifier) @code_class.name
  body: (class_body) @code_class.body
) @code_class.def

; Class extends superclass:  class X extends Y
(class_declaration
  name: (identifier) @code_class.name
  superclass: (superclass (type_identifier) @code_extends.target)
) @code_extends.def

; Class implements interfaces (one capture per interface)
(class_declaration
  name: (identifier) @code_class.name
  interfaces: (interfaces (type_list (type_identifier) @code_implements.target))
) @code_implements.def

; Interface declarations
(interface_declaration
  name: (identifier) @code_interface.name
  body: (interface_body) @code_interface.body
) @code_interface.def

; Enum declarations (treated as classes — same as java.scm)
(enum_declaration
  name: (identifier) @code_class.name
  body: (enum_body) @code_class.body
) @code_class.def

; Trigger declarations:  trigger T on SObject (events) { ... }
; A top-level named construct — surface it as a class-kind symbol.
(trigger_declaration
  name: (identifier) @code_class.name
  body: (trigger_body) @code_class.body
) @code_class.def

; Method declarations
(method_declaration
  name: (identifier) @code_method.name
  parameters: (formal_parameters) @code_method.params
  body: (block)? @code_method.body
) @code_method.def

; Constructor declarations
(constructor_declaration
  name: (identifier) @code_method.name
  parameters: (formal_parameters) @code_method.params
  body: (constructor_body) @code_method.body
) @code_method.def

; Field declarations (includes static/final — Apex uses field_declaration)
(field_declaration
  declarator: (variable_declarator
    name: (identifier) @code_field.name
  )
) @code_field.def

; Method invocations:  foo.bar()  /  bar()
(method_invocation
  name: (identifier) @call.method
  arguments: (argument_list) @call.args
) @call

; Object construction:  new Foo(...)  → constructor call
(object_creation_expression
  type: (type_identifier) @call.function
) @call
