; Free-format RPG IV — wicked_estate @code_* convention.
; Node types are from the in-house tree-sitter-rpg grammar (vendor/tree-sitter-rpg), each carrying
; a `name:` field. Validated by the corpus parse-gate + extraction-count test (tests/rpg_grammar.rs).

; dcl-proc NAME ... end-proc  → a procedure (RPG's function; the CALL/CALLP target)
(proc_decl
  name: (identifier) @code_function.name) @code_function.def

; dcl-s NAME ...  → standalone variable
(var_decl
  name: (identifier) @code_variable.name) @code_variable.def

; dcl-c NAME ...  → named constant
(const_decl
  name: (identifier) @code_constant.name) @code_constant.def

; dcl-ds NAME ... end-ds  → data structure (a type)
(ds_decl
  name: (identifier) @code_type.name) @code_type.def

; dcl-f NAME ...  → file declaration
(file_decl
  name: (identifier) @code_variable.name) @code_variable.def

; NAME(args)  → procedure call (proc→proc graph; resolves to dcl-proc nodes)
(call_expression
  function: (identifier) @call.function) @call
