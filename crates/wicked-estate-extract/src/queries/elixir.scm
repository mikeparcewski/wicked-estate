; wicked_estate Elixir extraction queries — @code_* convention.
; Capture names follow the @code_* convention.
; Elixir uses call-based syntax for all definitions (defmodule, def, defp, etc.).

; Module and protocol definitions
(call
  target: (identifier) @_def_kw
  (arguments (alias) @code_module.name)
  (#match? @_def_kw "^(defmodule|defprotocol)$")
) @code_module.def

; Function definitions — zero-arity: def foo
(call
  target: (identifier) @_fn_kw
  (arguments
    (identifier) @code_function.name)
  (#match? @_fn_kw "^(def|defp|defdelegate|defguard|defguardp|defmacro|defmacrop)$")
) @code_function.def

; Function definitions — regular clause: def foo(args)
(call
  target: (identifier) @_fn_kw
  (arguments
    (call target: (identifier) @code_function.name))
  (#match? @_fn_kw "^(def|defp|defdelegate|defguard|defguardp|defmacro|defmacrop)$")
) @code_function.def

; Function definitions — clause with guard: def foo(args) when guard
(call
  target: (identifier) @_fn_kw
  (arguments
    (binary_operator
      left: (call target: (identifier) @code_function.name)
      operator: "when"))
  (#match? @_fn_kw "^(def|defp|defdelegate|defguard|defguardp|defmacro|defmacrop)$")
) @code_function.def

; Import-like directives: use / import / alias — produce Import nodes
; These are syntactic calls in Elixir but semantically they are imports/aliases.
(call
  target: (identifier) @_kw
  (#match? @_kw "^(use|import|alias)$")
  (arguments . (alias) @import.source)
) @import

(call
  target: (identifier) @_kw
  (#match? @_kw "^(use|import|alias)$")
  (arguments . (identifier) @import.source)
) @import

; Function calls — local identifier call
(call
  target: (identifier) @call.function
) @call

; Function calls — remote (Mod.fun) — dot.right is the method name
(call
  target: (dot
    right: (identifier) @call.method)
) @call.method
