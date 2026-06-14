; wicked_estate Java extraction queries — @code_* convention.

; Class declarations
(class_declaration
  name: (identifier) @code_class.name
  body: (class_body) @code_class.body
) @code_class.def

; Enum declarations (treated as classes)
(enum_declaration
  name: (identifier) @code_class.name
  body: (enum_body) @code_class.body
) @code_class.def

; Class extends superclass
(class_declaration
  name: (identifier) @code_class.name
  (superclass (type_identifier) @code_extends.target)
) @code_extends.def

; Class implements interfaces (one capture per interface)
(class_declaration
  name: (identifier) @code_class.name
  (super_interfaces (type_list (type_identifier) @code_implements.target))
) @code_implements.def

; Interface declarations
(interface_declaration
  name: (identifier) @code_interface.name
  body: (interface_body) @code_interface.body
) @code_interface.def

; Method declarations
(method_declaration
  type: (_) @code_method.return_type
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

; Field declarations (includes static final — Java grammar uses field_declaration for these)
(field_declaration
  declarator: (variable_declarator
    name: (identifier) @code_field.name
  )
) @code_field.def

; Import declarations
(import_declaration
  (scoped_identifier) @import.source
) @import

; Method invocations
(method_invocation
  name: (identifier) @call.method
  arguments: (argument_list) @call.args
) @call
