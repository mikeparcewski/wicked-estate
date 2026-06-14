; wicked_estate Racket (.rkt) extraction queries — @code_* convention.
;
; Grammar: tree-sitter-racket 0.24.7 (6cdh/tree-sitter-racket). This is a PURE
; s-expression grammar — there are NO semantic nodes (no `define`/`function`/`call`
; node types). Everything is `(list ...)` of `(symbol)` / nested `(list)`.
; Verified against src/node-types.json (named: list, symbol, program, quote, ...);
; pattern mirrors the grammar's own queries/tags.scm and wicked_estate's clojure.scm.
;
; The `symbol` node is a leaf (no `name:` field) — capture (symbol) directly.

; (define (f args...) body)  → function; name is the FIRST symbol of the inner list.
(list
  .
  (symbol) @_kw
  (#match? @_kw "^(define|define/contract|define/public|define/private)$")
  .
  (list
    .
    (symbol) @code_function.name)
) @code_function.def

; (define x value)  → top-level constant/variable; name is the bare symbol.
(list
  .
  (symbol) @_def_kw
  (#eq? @_def_kw "define")
  .
  (symbol) @code_constant.name
) @code_constant.def

; (define-syntax name ...) / (define-syntax-rule (name ...) ...)  → macro (bare-symbol form)
(list
  .
  (symbol) @_macro_kw
  (#match? @_macro_kw "^(define-syntax|define-syntax-rule|define-simple-macro)$")
  .
  (symbol) @code_macro.name
) @code_macro.def

; (define-syntax (name ...) ...)  → macro (procedural form, name nested in inner list)
(list
  .
  (symbol) @_macro_kw2
  (#match? @_macro_kw2 "^(define-syntax|define-syntax-rule)$")
  .
  (list
    .
    (symbol) @code_macro.name)
) @code_macro.def

; (struct name ...) / (define-struct name ...)  → type/class
(list
  .
  (symbol) @_struct_kw
  (#match? @_struct_kw "^(struct|define-struct)$")
  .
  (symbol) @code_struct.name
) @code_struct.def

; (require mod ...)  → import; the first module spec is the source.
(list
  .
  (symbol) @_req_kw
  (#eq? @_req_kw "require")
  .
  (symbol) @import.source
) @import

; Function call: (some-fn args...) — first symbol is the callee.
(list
  .
  (symbol) @call.function
) @call
