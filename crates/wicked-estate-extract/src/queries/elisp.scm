; wicked_estate Emacs Lisp extraction queries — @code_* convention.
;
; Verified against arborium-elisp-2.18.0 node-types.json.
; Named nodes used:
;   function_definition  — field name: symbol, field parameters: list, field docstring: string
;   macro_definition     — field name: symbol, field parameters: list

; ── Function definitions (defun) ─────────────────────────────────────────────
(function_definition
  name: (symbol) @code_function.name
) @code_function.def

; ── Macro definitions (defmacro) ─────────────────────────────────────────────
(macro_definition
  name: (symbol) @code_macro.name
) @code_macro.def
