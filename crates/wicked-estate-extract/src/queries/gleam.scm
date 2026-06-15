; Gleam (arborium-gleam) — @code_* convention.
; Verified node types: function.name=identifier; external_function.name=identifier;
; function_call.function=identifier; import.module=module.
(function
  name: (identifier) @code_function.name) @code_function.def

(external_function
  name: (identifier) @code_function.name) @code_function.def

(function_call
  function: (identifier) @call.function) @call

; Import statements — import gleam/float, import gleam/list as lst
(import
  module: (module) @import.source
) @import
