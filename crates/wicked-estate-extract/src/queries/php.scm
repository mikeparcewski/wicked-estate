; wicked_estate PHP extraction queries — @code_* convention.
; Capture names follow the @code_* convention.

; Class declarations
(class_declaration
  name: (name) @code_class.name
  body: (declaration_list) @code_class.body
) @code_class.def

; Interface declarations
(interface_declaration
  name: (name) @code_interface.name
  body: (declaration_list) @code_interface.body
) @code_interface.def

; Trait declarations (map to interface — no separate trait NodeKind)
(trait_declaration
  name: (name) @code_interface.name
  body: (declaration_list) @code_interface.body
) @code_interface.def

; Method declarations
(method_declaration
  name: (name) @code_method.name
  body: (compound_statement)? @code_method.body
) @code_method.def

; Function definitions
(function_definition
  name: (name) @code_function.name
  body: (compound_statement) @code_function.body
) @code_function.def

; Namespace declarations
(namespace_definition
  name: (namespace_name) @code_namespace.name
  body: (compound_statement)? @code_namespace.body
) @code_namespace.def

; Use declarations (namespace imports)
(namespace_use_declaration
  (namespace_use_clause
    (qualified_name) @import.source
  )
) @import

; require / include statements
(include_expression
  (string) @import.source
) @import

(include_once_expression
  (string) @import.source
) @import

(require_expression
  (string) @import.source
) @import

(require_once_expression
  (string) @import.source
) @import

; Function calls
(function_call_expression
  function: (name) @call.function
) @call

(function_call_expression
  function: (qualified_name
    (name) @call.function
  )
) @call

; Method calls
(member_call_expression
  name: (name) @call.method
) @call.method

; Static method calls
(scoped_call_expression
  name: (name) @call.method
) @call.method
