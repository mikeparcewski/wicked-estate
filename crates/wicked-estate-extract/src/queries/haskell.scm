; wicked_estate Haskell extraction queries — @code_* convention.
;
; Verified against tree-sitter-haskell-0.23.1 node-types.json.
; Named nodes used:
;   function        — field name: variable | prefix_id
;   data_type       — field name: name (types: name, prefix_id, …)
;   newtype         — field name: name (types: name, prefix_id, …)
;   type_synomym    — field name: name (note: typo "synomym" in grammar)
;   class           — field name: name
;   instance        — field name: name (the typeclass name, not the type)
;   import          — field module: module (the module name)
;   apply           — field function: expression (call site)

; ── Function / value definitions ─────────────────────────────────────────────
; Haskell function: (function name: (variable) @name)
(function
  name: (variable) @code_function.name
) @code_function.def

; Operator-style function head: name is prefix_id (e.g. `(+++) = …`)
(function
  name: (prefix_id) @code_function.name
) @code_function.def

; ── Type declarations ─────────────────────────────────────────────────────────
; data Foo = …
(data_type
  name: (name) @code_type.name
) @code_type.def

; newtype Foo = …
(newtype
  name: (name) @code_type.name
) @code_type.def

; type Foo = …  (note: typo "synomym" is in the grammar itself)
(type_synomym
  name: (name) @code_type.name
) @code_type.def

; ── Class / instance declarations ────────────────────────────────────────────
; class Functor f where …
(class
  name: (name) @code_class.name
) @code_class.def

; instance Functor Maybe where …
(instance
  name: (name) @code_class.name
) @code_class.def

; ── Imports ───────────────────────────────────────────────────────────────────
; import Data.Map  /  import qualified Data.Map as M  etc.
; (import field module: (module …))
(import
  module: (module) @import.source
) @import

; ── Function application (calls) ─────────────────────────────────────────────
; Haskell call: (apply function: (expression) …)
; The function position is an expression; when it reduces to a variable that is
; the callee name.  We capture the variable node text (the leaf identifier).
(apply
  function: (variable) @call.function
) @call
