; wicked_estate Arduino (.ino) extraction queries — @code_* convention.
;
; Grammar: tree-sitter-arduino 0.24.0, which is `grammar(CPP, {...})` — it INHERITS
; the entire tree-sitter-cpp node set (verified against src/node-types.json: every
; node used here is present). This query mirrors cpp.scm; Arduino sketches are C++
; with setup()/loop() free functions, so the C++ patterns extract them directly.

; Class definitions
(class_specifier
  name: (type_identifier) @code_class.name
) @code_class.def

; Struct definitions
(struct_specifier
  name: (type_identifier) @code_struct.name
) @code_struct.def

; Enum definitions
(enum_specifier
  name: (type_identifier) @code_enum.name
) @code_enum.def

; Namespace definitions
(namespace_definition
  name: (namespace_identifier) @code_namespace.name
) @code_namespace.def

; Top-level function definitions (identifier declarator) — e.g. setup(), loop()
(function_definition
  declarator: (function_declarator
    declarator: (identifier) @code_function.name
  )
) @code_function.def

; Method definitions inside classes (field_identifier declarator)
(function_definition
  declarator: (function_declarator
    declarator: (field_identifier) @code_method.name
  )
) @code_method.def

; Object-like macros:  #define LED 13
(preproc_def
  name: (identifier) @code_constant.name
) @code_constant.def

; Function-like macros
(preproc_function_def
  name: (identifier) @code_function.name
) @code_function.def

; Type aliases (typedef)
(type_definition
  declarator: (type_identifier) @code_type.name
) @code_type.def

; using alias: using X = Y
(alias_declaration
  name: (type_identifier) @code_type.name
) @code_type.def

; Include directives:  #include <Arduino.h>  /  #include "mylib.h"
(preproc_include
  path: (system_lib_string) @import.source
) @import

(preproc_include
  path: (string_literal) @import.source
) @import

; Function calls — simple:  digitalWrite(...)
(call_expression
  function: (identifier) @call.function
) @call

; Method calls via field expression:  Serial.println(...)
(call_expression
  function: (field_expression
    field: (field_identifier) @call.method
  )
) @call.method
