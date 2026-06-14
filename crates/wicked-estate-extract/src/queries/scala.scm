; wicked_estate Scala extraction queries — @code_* convention.
; Capture names follow the @code_* convention.
; Node field names verified against tree-sitter-scala 0.23.4 node-types.json.
; Note: extends_clause uses `type:` field, not anonymous child;
;       class/object/trait use `extend:` field for the extends_clause.

; Object definitions (singletons — map to class)
(object_definition
  name: (identifier) @code_class.name
) @code_class.def

; Class definitions
(class_definition
  name: (identifier) @code_class.name
) @code_class.def

; Trait definitions (map to interface)
(trait_definition
  name: (identifier) @code_interface.name
) @code_interface.def

; Function / method definitions
(function_definition
  name: (identifier) @code_function.name
) @code_function.def

; Val definitions (immutable bindings → variable)
(val_definition
  pattern: (identifier) @code_variable.name
) @code_variable.def

; Package declarations (→ module)
(package_clause
  name: (_) @code_module.name
) @code_module.def

; Import declarations — capture whole node as @import (no .source needed;
; extractor uses node text as raw import name when .source is absent)
(import_declaration) @import

; Call expressions — function field is identifier
(call_expression
  function: (identifier) @call.function
) @call

; Heritage: class extends plain type identifier
(class_definition
  name: (identifier) @code_class.name
  extend: (extends_clause
    type: (type_identifier) @code_extends.target)
) @code_extends.def

; Heritage: class extends generic type, e.g. Container[B]
(class_definition
  name: (identifier) @code_class.name
  extend: (extends_clause
    type: (generic_type
      type: (type_identifier) @code_extends.target))
) @code_extends.def

; Heritage: trait extends plain type identifier
(trait_definition
  name: (identifier) @code_interface.name
  extend: (extends_clause
    type: (type_identifier) @code_extends.target)
) @code_extends.def

; Heritage: trait extends generic type
(trait_definition
  name: (identifier) @code_interface.name
  extend: (extends_clause
    type: (generic_type
      type: (type_identifier) @code_extends.target))
) @code_extends.def

; Heritage: object extends plain type
(object_definition
  name: (identifier) @code_class.name
  extend: (extends_clause
    type: (type_identifier) @code_extends.target)
) @code_extends.def

; Heritage: object extends generic type
(object_definition
  name: (identifier) @code_class.name
  extend: (extends_clause
    type: (generic_type
      type: (type_identifier) @code_extends.target))
) @code_extends.def
