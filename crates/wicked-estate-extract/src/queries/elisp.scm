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

; ── Function calls — list/special_form whose first child is a symbol ──────────
; In arborium-elisp there is no dedicated function_call node; every application
; is a list (or special_form) with a symbol as its first element.
(list . (symbol) @call.function) @call

(special_form . (symbol) @call.function) @call
