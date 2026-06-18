; Progress OpenEdge ABL — wicked_estate @code_* convention.
; Node types are from the in-house tree-sitter-abl grammar (vendor/tree-sitter-abl), each carrying
; a `name:` field. Validated by the corpus parse-gate + extraction-count test (tests/abl_grammar.rs).

; CLASS Foo.Bar.Baz : ... END CLASS.
(class_definition
  name: (identifier) @code_class.name) @code_class.def

; INTERFACE IFoo : ... END INTERFACE.
(interface_definition
  name: (identifier) @code_interface.name) @code_interface.def

; METHOD [access] returntype name(params) : ... END METHOD.
(method_definition
  name: (identifier) @code_method.name) @code_method.def

; CONSTRUCTOR [access] name(params) : ... END CONSTRUCTOR.
(constructor_definition
  name: (identifier) @code_constructor.name) @code_constructor.def

; DESTRUCTOR [access] name(params) : ... END DESTRUCTOR.
(destructor_definition
  name: (identifier) @code_method.name) @code_method.def

; FUNCTION name RETURNS type ... : ... END FUNCTION.
(function_definition
  name: (identifier) @code_function.name) @code_function.def

; PROCEDURE name ... : ... END PROCEDURE.
(procedure_definition
  name: (identifier) @code_function.name) @code_function.def

; name(args) → function/method call
(call_expression
  function: (identifier) @call.function) @call

; RUN target → procedure call (ABL's primary inter-procedure call)
(run_statement
  procedure: (identifier) @call.function) @call
