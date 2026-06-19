; Crystal Reports formula language (Crystal Syntax) — wicked_estate @code_* convention.
; Node types are from the in-house tree-sitter-crystal-formula grammar.
; Validated by the corpus parse-gate + extraction-count test (tests/crystal_formula_grammar.rs).

; [Local|Global|Shared] <Type>Var NAME → a variable (Shared/Global are cross-formula state).
(variable_declaration
  name: (identifier) @code_variable.name) @code_variable.def

; {@FormulaName} → a reference to another formula (the formula-to-formula call edge).
(formula_ref
  name: (brace_name) @call.function) @call

; Name(args) → built-in or user function call.
(call_expression
  function: (identifier) @call.function) @call
