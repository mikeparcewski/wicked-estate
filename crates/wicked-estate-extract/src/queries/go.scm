; wicked_estate Go extraction queries — @code_* convention.

; Function declarations
(function_declaration
  name: (identifier) @code_function.name
  parameters: (parameter_list) @code_function.params
  result: (_)? @code_function.return_type
  body: (block) @code_function.body
) @code_function.def

; Method declarations
(method_declaration
  name: (field_identifier) @code_method.name
  parameters: (parameter_list) @code_method.params
  result: (_)? @code_method.return_type
  body: (block) @code_method.body
) @code_method.def

; Struct type declarations: type X struct { ... }
(type_declaration
  (type_spec
    name: (type_identifier) @code_struct.name
    type: (struct_type) @code_struct.body
  )
) @code_struct.def

; Interface type declarations: type X interface { ... }
(type_declaration
  (type_spec
    name: (type_identifier) @code_interface.name
    type: (interface_type) @code_interface.body
  )
) @code_interface.def

; Type aliases: type X = Y
(type_alias
  name: (type_identifier) @code_type.name
) @code_type.def

; Constants: const X = ...
(const_spec name: (identifier) @code_constant.name) @code_constant.def

; Variables: var X ...
(var_spec name: (identifier) @code_variable.name) @code_variable.def

; Import spec path (individual string paths inside import blocks)
; Capturing import_spec.source is sufficient — the import_declaration wrapper
; would produce a duplicate @import per file; we use only the fine-grained path.
(import_spec
  path: (interpreted_string_literal) @import.source
) @import

; Function calls — simple
(call_expression
  function: (identifier) @call.function
  arguments: (argument_list) @call.args
) @call

; Method calls — selector expression
(call_expression
  function: (selector_expression
    field: (field_identifier) @call.method
  )
) @call.method
