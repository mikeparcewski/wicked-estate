; Groovy (arborium-groovy) — @code_* convention.
; Verified node types: class_definition.name=identifier; function_definition.function=identifier;
; function_call.function=identifier. (No `method_definition` node in this grammar.)
(class_definition
  name: (identifier) @code_class.name) @code_class.def

(function_definition
  function: (identifier) @code_function.name) @code_function.def

(function_call
  function: (identifier) @call.function) @call
