; Informix 4GL — wicked_estate @code_* convention.
; Node types are from the in-house tree-sitter-informix4gl grammar (vendor/tree-sitter-informix4gl).
; Validated by the corpus parse-gate + extraction-count test (tests/informix4gl_grammar.rs).

; MAIN … END MAIN (program entry point) — the aliased MAIN keyword carries the name.
(main_definition
  name: (main_keyword) @code_function.name) @code_function.def

; FUNCTION name(params) … END FUNCTION
(function_definition
  name: (identifier) @code_function.name) @code_function.def

; REPORT name(params) … END REPORT
(report_definition
  name: (identifier) @code_function.name) @code_function.def

; CALL func(args) [RETURNING …]
(call_statement
  function: (identifier) @call.function) @call

; RUN "cmd" / RUN cmd_var
(run_statement
  command: (identifier) @call.function) @call

; func(args) — function call
(call_expression
  function: (identifier) @call.function) @call
