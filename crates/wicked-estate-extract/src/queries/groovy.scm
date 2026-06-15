; Groovy (arborium-groovy) — @code_* convention.
; Verified node types: class_definition.name=identifier; function_definition.function=identifier;
; function_call.function=identifier; groovy_import.import=(_).
(class_definition
  name: (identifier) @code_class.name) @code_class.def

(function_definition
  function: (identifier) @code_function.name) @code_function.def

(function_call
  function: (identifier) @call.function) @call

; Import statements — import groovy.transform.CompileStatic / import groovy.*
(groovy_import
  import: (_) @import.source
) @import

(wildcard_import) @import
