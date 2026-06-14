; wicked_estate Pony extraction — tree-sitter-pony grammar, @code_* convention.
; Node types + fields verified against tree-sitter-pony-1.0.0 node-types.json / grammar.js.
; Type definitions name their identifier as a direct child (after optional annotation/
; capability, which are their own nodes); methods/behaviors/constructors likewise.

; ── Type definitions ─────────────────────────────────────────────────────────
; actor A is Trait ... — Pony's concurrent active object; modelled as a class.
(actor_definition
  (identifier) @code_class.name) @code_class.def

; class C ...
(class_definition
  (identifier) @code_class.name) @code_class.def

; struct S ...
(struct_definition
  (identifier) @code_struct.name) @code_struct.def

; primitive P ... — a stateless type; modelled as a class.
(primitive_definition
  (identifier) @code_class.name) @code_class.def

; interface I ... (structural)
(interface_definition
  (identifier) @code_interface.name) @code_interface.def

; trait T ... (nominal)
(trait_definition
  (identifier) @code_trait.name) @code_trait.def

; type Alias is Concrete
(type_alias
  (identifier) @code_type.name) @code_type.def

; ── Members ──────────────────────────────────────────────────────────────────
; fun name(params): T => ...  — method (the identifier is a direct child).
(method
  (identifier) @code_method.name) @code_method.def

; be name(params) => ...  — behaviour (async message handler); a method.
(behavior
  (identifier) @code_method.name) @code_method.def

; new name(params) => ...  — constructor.
(constructor
  (identifier) @code_constructor.name) @code_constructor.def

; let/var/embed name: T  — a field (has a real `name` field).
(field
  name: (identifier) @code_field.name) @code_field.def

; ── Imports ──────────────────────────────────────────────────────────────────
; use "collections"  /  use coll = "collections"  — source is the string literal
; (quotes are stripped downstream by strip_literal_quotes).
(use_statement
  (string) @import.source) @import

; ── Calls ────────────────────────────────────────────────────────────────────
; foo(args) — direct call. Method-style calls (a.b()) go through member_expression,
; which has no name field (it is `expression "." expression`); capturing them would
; emit the whole receiver.member text as the name, so they are omitted to avoid a
; wrong call name.
(call_expression
  callee: (identifier) @call.function) @call
