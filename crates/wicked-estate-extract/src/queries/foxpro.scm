; Visual FoxPro — wicked_estate @code_* convention.
; Node types are from the in-house tree-sitter-foxpro grammar (vendor/tree-sitter-foxpro).
; Validated by the corpus parse-gate + extraction-count test (tests/foxpro_grammar.rs).

; DEFINE CLASS name AS parent … ENDDEFINE
(define_class
  name: (identifier) @code_class.name) @code_class.def

; PROCEDURE name … [ENDPROC]
(procedure_definition
  name: (identifier) @code_function.name) @code_function.def

; FUNCTION name … [ENDFUNC]
(function_definition
  name: (identifier) @code_function.name) @code_function.def

; func(args) / obj.method(args)
(call_expression
  function: (identifier) @call.function) @call
