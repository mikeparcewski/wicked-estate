; LotusScript (IBM Domino) — wicked_estate @code_* convention.
; Node types are from the in-house tree-sitter-lotusscript grammar (vendor/tree-sitter-lotusscript).
; Validated by the corpus parse-gate + extraction-count test (tests/lotusscript_grammar.rs).

; Class … End Class
(class_definition
  name: (identifier) @code_class.name) @code_class.def

; Sub name(args) … End Sub
(sub_definition
  name: (identifier) @code_function.name) @code_function.def

; Function name(args) [As type] … End Function
(function_definition
  name: (identifier) @code_function.name) @code_function.def

; Property Get/Set name … End Property
(property_definition
  name: (identifier) @code_property.name) @code_property.def

; Type name … End Type
(type_definition
  name: (identifier) @code_type.name) @code_type.def

; Call target(args)
(call_statement
  function: (identifier) @call.function) @call

; name(args) — function/method call
(call_expression
  function: (identifier) @call.function) @call
