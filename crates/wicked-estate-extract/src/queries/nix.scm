; wicked_estate Nix extraction queries — @code_* convention.
;
; Verified against tree-sitter-nix-0.3.0 node-types.json.
; Nix is a pure functional config language; "declarations" are attribute
; bindings and function expressions.  We treat them as code_variable.def /
; code_function.def.
;
; Named nodes used:
;   binding              — attrpath + expression (let/attrset binding)
;   function_expression  — universal (param name) + body
;   apply_expression     — function + argument (call site)
;   inherit              — attrs: inherited_attrs
;   inherit_from         — expression + attrs

; ── Attribute bindings (let / attrset) ───────────────────────────────────────
; let x = ...; in ...  or  { x = ...; }
; attrpath field is a chain of identifiers; we capture the first atom.
;
; Two disjoint patterns: function-valued bindings and everything else.
; We use a concrete type for the function case (function_expression) and
; a plain (binding) anchor for the general case — the general case also fires
; for function bindings, but having two kinds (function vs variable) is
; acceptable: a consuming agent sees the more-specific @code_function if
; available.

; Binding whose RHS is a function_expression → function definition
; Use anchor (.) to match only the first identifier in the attrpath, avoiding
; dotted paths like `a.b.c` producing 3 spurious matches.
(binding
  attrpath: (attrpath
    . (identifier) @code_function.name)
  expression: (function_expression) @code_function.body
) @code_function.def

; Binding for scalar / config values → variable definition
(binding
  attrpath: (attrpath
    . (identifier) @code_variable.name)
) @code_variable.def

; ── Imports (inherit / inherit_from) ─────────────────────────────────────────
; inherit (pkgs) lib stdenv;
(inherit_from
  expression: (_) @import.source
) @import

; ── Function application (calls) ─────────────────────────────────────────────
; Nix apply_expression: (apply_expression function: (variable_expression))
; variable_expression has field name: identifier
(apply_expression
  function: (variable_expression
    name: (identifier) @call.function)
) @call
