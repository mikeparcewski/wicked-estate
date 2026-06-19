; PowerBuilder PowerScript — wicked_estate @code_* convention.
; Node types are from the in-house tree-sitter-powerscript grammar (vendor/tree-sitter-powerscript).
; Validated by the corpus parse-gate + extraction-count test (tests/powerscript_grammar.rs).

; [global|shared] type NAME from ANCESTOR … end type → a PB object (window/userobject/structure).
; The ancestor (from-clause) is the inheritance target.
(type_definition
  name: (identifier) @code_class.name
  ancestor: (identifier) @code_extends.target) @code_class.def
(type_definition
  name: (identifier) @code_class.name
  !ancestor) @code_class.def

; [access] function <rettype> <name>(…) … end function
(function_body
  name: (identifier) @code_function.name) @code_function.def

; [access] subroutine <name>(…) … end subroutine
(subroutine_body
  name: (identifier) @code_function.name) @code_function.def

; event [type <rettype>] <name>(…) … end event
(event_body
  name: (identifier) @code_method.name) @code_method.def

; on <object>.create | <object>.destroy … end on
(on_body
  name: (identifier) @code_method.name) @code_method.def

; func(args) / obj.method(args)
(call_expression
  function: (identifier) @call.function) @call
