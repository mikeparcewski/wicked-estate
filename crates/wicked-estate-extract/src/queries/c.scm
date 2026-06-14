; wicked_estate C extraction queries — @code_* convention.

; Struct definitions
(struct_specifier
  name: (type_identifier) @code_struct.name
  body: (field_declaration_list) @code_struct.body
) @code_struct.def

; Enum definitions
(enum_specifier
  name: (type_identifier) @code_enum.name
  body: (enumerator_list) @code_enum.body
) @code_enum.def

; Function definitions
(function_definition
  declarator: (function_declarator
    declarator: (identifier) @code_function.name
    parameters: (parameter_list) @code_function.params
  )
  body: (compound_statement) @code_function.body
) @code_function.def

; Object-like macros (#define NAME value)
(preproc_def
  name: (identifier) @code_constant.name
) @code_constant.def

; Function-like macros (#define FOO(x) ...)
(preproc_function_def
  name: (identifier) @code_function.name
) @code_function.def

; Type aliases (typedef)
(type_definition
  declarator: (type_identifier) @code_type.name
) @code_type.def

; Include directives
(preproc_include
  path: (system_lib_string) @import.source
) @import

(preproc_include
  path: (string_literal) @import.source
) @import

; Function calls
(call_expression
  function: (identifier) @call.function
  arguments: (argument_list) @call.args
) @call

; Method/member calls via field expression
(call_expression
  function: (field_expression
    field: (field_identifier) @call.method
  )
) @call.method
