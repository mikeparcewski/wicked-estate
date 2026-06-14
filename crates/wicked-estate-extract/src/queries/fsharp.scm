; wicked_estate F# extraction queries — @code_* convention.
;
; Verified against arborium-fsharp-2.18.0 node-types.json.
; Named nodes used:
;   function_or_value_defn — children: function_declaration_left | value_declaration_left
;   function_declaration_left — children: identifier (the fn name)
;   module_defn — children: identifier; field block: module body
;   named_module — field name: ?

; ── Function / value bindings (let f x = ...) ────────────────────────────────
; function_or_value_defn contains function_declaration_left which has an identifier.
(function_or_value_defn
  (function_declaration_left
    (identifier) @code_function.name)
) @code_function.def

; ── Module definitions ────────────────────────────────────────────────────────
(module_defn
  (identifier) @code_module.name
) @code_module.def

; ── Named module (module Foo = ...) ──────────────────────────────────────────
(named_module
  name: (_) @code_module.name
) @code_module.def
