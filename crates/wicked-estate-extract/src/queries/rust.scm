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

; NOTE (§11): there is deliberately NO impl-body Method pattern here. The general
; function pattern above already matches impl methods; a second impl-scoped pattern
; emitted every method twice on the same SymbolId with a different kind
; (Method + Function), and the store's last-write-wins upsert silently re-kinded
; them. Restoring Method kind belongs with enclosing-type identity (method-identity
; lane), not with a duplicate pattern.

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
