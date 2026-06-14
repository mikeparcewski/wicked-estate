; wicked_estate R extraction queries — @code_* convention.
;
; Verified against tree-sitter-r-1.2.0 node-types.json.
; R uses assignment operators (<-, =) to bind names.  When the RHS is a
; function_definition we emit a function node; otherwise a variable node.
;
; Named nodes used:
;   binary_operator  — lhs (identifier), operator ("<-" | "="), rhs
;   function_definition — body + parameters
;   call             — function (identifier | namespace_operator), arguments
;   namespace_operator — lhs: identifier, rhs: identifier (pkg::fn)

; ── Function definitions via <- ───────────────────────────────────────────────
; foo <- function(x) { ... }
(binary_operator
  lhs: (identifier) @code_function.name
  operator: "<-"
  rhs: (function_definition) @code_function.body
) @code_function.def

; foo = function(x) { ... }
(binary_operator
  lhs: (identifier) @code_function.name
  operator: "="
  rhs: (function_definition) @code_function.body
) @code_function.def

; ── Direct function calls: foo(…) ────────────────────────────────────────────
(call
  function: (identifier) @call.function
) @call

; ── Qualified calls: pkg::foo(…) ─────────────────────────────────────────────
(call
  function: (namespace_operator
    rhs: (identifier) @call.function)
) @call
