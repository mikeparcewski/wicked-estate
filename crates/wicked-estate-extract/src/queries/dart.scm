; Dart (arborium-dart) — @code_* convention.
; Verified node types: class_definition.name=identifier; function_signature.name=identifier;
; enum_declaration.name=identifier. (Functions surface as `function_signature`, not
; `function_declaration`, in this grammar.)
(class_definition
  name: (identifier) @code_class.name) @code_class.def

(function_signature
  name: (identifier) @code_function.name) @code_function.def

(enum_declaration
  name: (identifier) @code_type.name) @code_type.def
