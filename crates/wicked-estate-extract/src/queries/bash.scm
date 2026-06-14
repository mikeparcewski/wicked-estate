; wicked_estate Bash extraction queries — @code_* convention.

; Function definitions
(function_definition
  name: (word) @code_function.name
) @code_function.def

; Variable assignments (top-level only — not inside functions for bash)
(variable_assignment
  name: (variable_name) @code_variable.name
) @code_variable.def

; Command invocations (function calls in bash)
(command
  name: (command_name (word) @call.function)
) @call
