; wicked_estate CMake extraction queries — @code_* convention.
;
; Verified against arborium-cmake-2.18.0 node-types.json.
; Named nodes used:
;   function_def — children: function_command, body, endfunction_command
;   macro_def    — children: macro_command, body, endmacro_command
;   normal_command — children: identifier, argument_list
;
; function_command / macro_command have an argument_list whose first argument
; is the function/macro name.  We use a wildcard to capture the first argument.

; ── Function definitions ──────────────────────────────────────────────────────
(function_def
  (function_command
    (argument_list
      .
      (_) @code_function.name))
) @code_function.def

; ── Macro definitions ────────────────────────────────────────────────────────
(macro_def
  (macro_command
    (argument_list
      .
      (_) @code_macro.name))
) @code_macro.def

; ── Command invocations (calls) ──────────────────────────────────────────────
(normal_command
  (identifier) @call.function
) @call
