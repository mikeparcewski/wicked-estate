; wicked_estate Lua extraction queries — written fresh (no prior art equivalent).
; Node types confirmed from tree-sitter-lua 0.2.0 node-types.json.
; function_declaration has named `name:` field (identifier or method_index_expression).
; function_call has named `name:` field (variable supertype: identifier | dot_index | bracket_index).
; Note: `variable` is a supertype — concrete subtype `identifier` is used in the name: field.

; Named function declarations: function foo() ... end
(function_declaration
  name: (identifier) @code_function.name
) @code_function.def

; Method declarations: function Obj:method() ... end
(function_declaration
  name: (method_index_expression
    method: (identifier) @code_method.name)
) @code_method.def

; Function calls — simple identifier call: foo(...)
(function_call
  name: (identifier) @call.function
) @call

; Method calls — colon syntax: obj:method(...)
(function_call
  name: (method_index_expression
    method: (identifier) @call.method)
) @call.method
