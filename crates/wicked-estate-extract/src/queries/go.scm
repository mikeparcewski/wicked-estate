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

; Struct fields: every named field_declaration inside a struct body (multi-name
; declarations like `a, b int` match once per name; embedded fields have no name:
; and are skipped)
(struct_type
  (field_declaration_list
    (field_declaration
      name: (field_identifier) @code_field.name
    ) @code_field.def
  )
)

; Type aliases: type X = Y
(type_alias
  name: (type_identifier) @code_type.name
) @code_type.def

; Defined types: type X T where T is NOT a struct/interface literal (those emit as
; Struct/Interface above). The type: alternation is deliberately EXHAUSTIVE over the
; non-struct/non-interface members of _type — a catch-all `type: (_)` would ALSO
; match every struct/interface type_spec, minting a TypeAlias with the SAME SymbolId
; as the real Struct/Interface node, which the store's last-write-wins upsert then
; silently re-kinds (D04-2).
; APPROXIMATION (D04-10, documented in docs/DESIGN-NOTES.md): Go defined types
; (`type UserID string`) are distinct types, not aliases, but we emit them as the
; generic `type` role → NodeKind::TypeAlias — the id suffix is correct (`Name#`)
; and no per-language Rust arm is needed.
(type_declaration
  (type_spec
    name: (type_identifier) @code_type.name
    type: [
      (type_identifier)
      (qualified_type)
      (generic_type)
      (function_type)
      (slice_type)
      (array_type)
      (map_type)
      (pointer_type)
      (channel_type)
      (parenthesized_type)
    ]
  )
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
