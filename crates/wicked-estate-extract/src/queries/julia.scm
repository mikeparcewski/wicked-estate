; wicked_estate Julia extraction queries — @code_* convention.
; Written fresh: node types confirmed from tree-sitter-julia 0.23.1 node-types.json.
; julia's AST uses no named fields on function_definition / struct_definition —
; names live in child nodes (signature > identifier, type_head > identifier).

; Module definitions — module_definition has a named `name:` field
(module_definition
  name: (identifier) @code_module.name
) @code_module.def

; Function definitions — name is inside a signature child (identifier form)
(function_definition
  (signature
    (identifier) @code_function.name)
) @code_function.def

; Function definitions — name inside signature > call_expression > identifier
(function_definition
  (signature
    (call_expression
      (identifier) @code_function.name))
) @code_function.def

; Struct definitions — name is inside type_head > identifier
(struct_definition
  (type_head
    (identifier) @code_struct.name)
) @code_struct.def

; Abstract type definitions — name inside type_head > identifier
(abstract_definition
  (type_head
    (identifier) @code_type.name)
) @code_type.def

; Import statements
(import_statement
  (identifier) @import.source
) @import

; Using statements
(using_statement
  (identifier) @import.source
) @import

; Function call expressions — first child identifier is the function name
(call_expression
  (identifier) @call.function
) @call
