; wicked_estate Rust extraction queries — @code_* convention.

; Function definitions
(function_item
  name: (identifier) @code_function.name
  body: (block) @code_function.body
) @code_function.def

; Struct definitions
(struct_item
  name: (type_identifier) @code_struct.name
) @code_struct.def

; Enum definitions
(enum_item
  name: (type_identifier) @code_enum.name
) @code_enum.def

; Trait definitions
(trait_item
  name: (type_identifier) @code_trait.name
) @code_trait.def

; impl blocks — methods inside impl
(impl_item
  body: (declaration_list
    (function_item
      name: (identifier) @code_method.name
      body: (block) @code_method.body
    ) @code_method.def
  )
)

; Constants
(const_item name: (identifier) @code_constant.name) @code_constant.def

; Statics (treated as constants)
(static_item name: (identifier) @code_constant.name) @code_constant.def

; Type aliases
(type_item name: (type_identifier) @code_type.name) @code_type.def

; Use declarations — capture the path argument as the import source.
; Handles: use std::fmt; (scoped_identifier), use foo; (identifier), use foo::{a, b} (use_list)
(use_declaration
  argument: (scoped_identifier) @import.source
) @import

(use_declaration
  argument: (identifier) @import.source
) @import

; Function calls — simple
(call_expression
  function: (identifier) @call.function
) @call

; Method calls — field expression (obj.method())
(call_expression
  function: (field_expression
    field: (field_identifier) @call.method
  )
) @call.method

; Method calls — scoped (Type::method())
(call_expression
  function: (scoped_identifier name: (identifier) @call.function)
) @call
