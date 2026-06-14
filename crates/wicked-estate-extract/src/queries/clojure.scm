; wicked_estate Clojure extraction queries — @code_* convention.
;
; Verified against arborium-clojure-2.18.0 node-types.json.
; Clojure is homoiconic — definitions are (defn name ...) list literals.
; sym_lit has a `name` field -> sym_name (named: true).
; We match list_lit whose first element is a sym_lit with name defn/defmacro/def/deftype.
; Using a predicate-free structural match: first child sym_lit then second sym_lit = name.
;
; Named nodes used:
;   list_lit  — children include sym_lit
;   sym_lit   — field name: sym_name (named)
;   sym_name  — the bare name text (named: true)

; (defn name ...)
(list_lit
  .
  (sym_lit
    name: (sym_name) @_kw
    (#any-of? @_kw "defn" "defn-" "defmacro" "defmulti"))
  .
  (sym_lit
    name: (sym_name) @code_function.name)
) @code_function.def

; (def NAME ...) — top-level var
(list_lit
  .
  (sym_lit
    name: (sym_name) @_def_kw
    (#eq? @_def_kw "def"))
  .
  (sym_lit
    name: (sym_name) @code_variable.name)
) @code_variable.def

; (deftype name ...) / (defrecord name ...)
(list_lit
  .
  (sym_lit
    name: (sym_name) @_type_kw
    (#any-of? @_type_kw "deftype" "defrecord" "defprotocol" "definterface"))
  .
  (sym_lit
    name: (sym_name) @code_class.name)
) @code_class.def

; Namespace declaration (ns name ...)
(list_lit
  .
  (sym_lit
    name: (sym_name) @_ns_kw
    (#eq? @_ns_kw "ns"))
  .
  (sym_lit
    name: (sym_name) @code_module.name)
) @code_module.def

; Function call: (some-fn args...) — first sym_lit is the callee
(list_lit
  .
  (sym_lit
    name: (sym_name) @call.function)
) @call
