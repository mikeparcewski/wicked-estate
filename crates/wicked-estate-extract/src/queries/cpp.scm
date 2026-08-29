; wicked_estate C++ extraction queries — @code_* convention.

; Class definitions
(class_specifier
  name: (type_identifier) @code_class.name
) @code_class.def

; Struct definitions
(struct_specifier
  name: (type_identifier) @code_struct.name
) @code_struct.def

; Enum definitions
(enum_specifier
  name: (type_identifier) @code_enum.name
) @code_enum.def

; Namespace definitions
(namespace_definition
  name: (namespace_identifier) @code_namespace.name
) @code_namespace.def

; Top-level function definitions (identifier declarator)
(function_definition
  declarator: (function_declarator
    declarator: (identifier) @code_function.name
  )
) @code_function.def

; Method definitions inside classes (field_identifier declarator)
(function_definition
  declarator: (function_declarator
    declarator: (field_identifier) @code_method.name
  )
) @code_method.def

; Object-like macros
(preproc_def
  name: (identifier) @code_constant.name
) @code_constant.def

; Function-like macros
(preproc_function_def
  name: (identifier) @code_function.name
) @code_function.def

; Type aliases (typedef) — two-pattern set (§11 / D04), same shape as c.scm.
; The old unconstrained pattern double-matched `typedef struct X X;` (TypeAlias with
; the same SymbolId as the real Struct → last-write-wins re-kind in the store).
; (i) tag-named typedefs emit only when the alias name differs from the tag name;
; (ii) anonymous-tag and non-tag typedefs always emit. The idiom is ubiquitous in C
; headers, which route to this grammar (.h → cpp).
(type_definition
  type: (struct_specifier name: (type_identifier) @_tag_name)
  declarator: (type_identifier) @code_type.name
  (#not-eq? @_tag_name @code_type.name)
) @code_type.def

(type_definition
  type: (enum_specifier name: (type_identifier) @_tag_name)
  declarator: (type_identifier) @code_type.name
  (#not-eq? @_tag_name @code_type.name)
) @code_type.def

(type_definition
  type: (union_specifier name: (type_identifier) @_tag_name)
  declarator: (type_identifier) @code_type.name
  (#not-eq? @_tag_name @code_type.name)
) @code_type.def

; (no macro_type_specifier here — that node exists only in the C grammar)
(type_definition
  type: [
    (struct_specifier !name)
    (enum_specifier !name)
    (union_specifier !name)
    (primitive_type)
    (sized_type_specifier)
    (type_identifier)
  ]
  declarator: (type_identifier) @code_type.name
) @code_type.def

; using alias: using X = Y
(alias_declaration
  name: (type_identifier) @code_type.name
) @code_type.def

; Include directives
(preproc_include
  path: (system_lib_string) @import.source
) @import

(preproc_include
  path: (string_literal) @import.source
) @import

; Function calls — simple
(call_expression
  function: (identifier) @call.function
) @call

; Method calls via field expression: obj.method()
(call_expression
  function: (field_expression
    field: (field_identifier) @call.method
  )
) @call.method
