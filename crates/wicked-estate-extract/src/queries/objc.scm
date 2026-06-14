; wicked_estate Objective-C extraction queries — @code_* convention.
;
; Verified against arborium-objc-2.18.0 node-types.json.
; Named nodes used:
;   class_interface        — children include identifier (class name); fields: superclass, category
;   class_implementation   — children include identifier (class name); fields: superclass, category
;   method_definition      — children include method_identifier -> identifier (method name)
;   function_definition    — field declarator: _declarator (-> function_declarator -> identifier)
;   implementation_definition — children: method_definition

; ── @interface declarations ───────────────────────────────────────────────────
; class_interface: first named identifier child is the class name
(class_interface
  (identifier) @code_class.name
) @code_class.def

; ── @implementation blocks ────────────────────────────────────────────────────
(class_implementation
  (identifier) @code_class.name
) @code_class.def

; ── Method definitions ───────────────────────────────────────────────────────
; method_definition children: method_identifier -> identifier
(method_definition
  (method_identifier
    (identifier) @code_method.name)
) @code_method.def

; ── C function definitions (in .mm files) ────────────────────────────────────
(function_definition
  declarator: (function_declarator
    declarator: (_) @code_function.name)
) @code_function.def
