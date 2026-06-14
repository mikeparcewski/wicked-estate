; wicked_estate C# extraction queries — @code_* convention.
; Note: uses tree-sitter-c-sharp 0.21.3 (ABI 14).

; Class declarations
(class_declaration
  name: (identifier) @code_class.name
  body: (declaration_list) @code_class.body
) @code_class.def

; Class extends a base class or implements an interface (plain identifier)
(class_declaration
  name: (identifier) @code_class.name
  (base_list (identifier) @code_extends.target)
) @code_extends.def

; Interface declarations
(interface_declaration
  name: (identifier) @code_interface.name
  body: (declaration_list) @code_interface.body
) @code_interface.def

; Struct declarations
(struct_declaration
  name: (identifier) @code_struct.name
  body: (declaration_list) @code_struct.body
) @code_struct.def

; Enum declarations
(enum_declaration
  name: (identifier) @code_enum.name
  body: (enum_member_declaration_list) @code_enum.body
) @code_enum.def

; Method declarations
(method_declaration
  name: (identifier) @code_method.name
  parameters: (parameter_list) @code_method.params
  body: (block)? @code_method.body
) @code_method.def

; Constructor declarations
(constructor_declaration
  name: (identifier) @code_method.name
  parameters: (parameter_list) @code_method.params
  body: (block) @code_method.body
) @code_method.def

; Field declarations (includes const, readonly, static fields)
(field_declaration
  (variable_declaration
    (variable_declarator name: (identifier) @code_field.name))
) @code_field.def

; Using directives (plain `using System;` does NOT have a name: field in 0.21 grammar —
; only the alias form `using X = Y` does. We capture both forms via the qualified_name or
; identifier child for the alias form.)
(using_directive
  (qualified_name) @import.source
) @import

(using_directive
  (identifier) @import.source
) @import

; Method invocations — member access
(invocation_expression
  function: (member_access_expression
    name: (identifier) @call.method
  )
  arguments: (argument_list) @call.args
) @call.method

; Simple invocations
(invocation_expression
  function: (identifier) @call.function
  arguments: (argument_list) @call.args
) @call
