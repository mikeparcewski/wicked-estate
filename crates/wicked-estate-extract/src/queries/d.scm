; wicked_estate D extraction — arborium-d grammar, @code_* convention.
; Node types + structure verified against arborium-d-2.18.0 node-types.json / grammar.json.
; NOTE: this grammar exposes NO named fields — every definition names its identifier as a
; positional child, so anchors use direct-child `(identifier)` patterns, not `name:` fields.

; ── Aggregates ───────────────────────────────────────────────────────────────
; class C { ... }  — the only bare direct-child identifier is the class name
; (annotations/capabilities are their own nodes, not bare identifiers).
(class_declaration
  (identifier) @code_class.name) @code_class.def

; interface I { ... }
(interface_declaration
  (identifier) @code_interface.name) @code_interface.def

; struct S { ... }
(struct_declaration
  (identifier) @code_struct.name) @code_struct.def

; union U { ... } — modelled as a struct (no dedicated union NodeKind).
(union_declaration
  (identifier) @code_struct.name) @code_struct.def

; enum E { ... }
(enum_declaration
  (identifier) @code_enum.name) @code_enum.def

; enum members become fields of the enum.
(enum_member
  (identifier) @code_field.name) @code_field.def

; template T(args) { ... } — D templates; named like a type.
(template_declaration
  (identifier) @code_type.name) @code_type.def

; ── Functions ────────────────────────────────────────────────────────────────
; ReturnType foo(params) { ... } — the name is identifier inside func_declarator.
(func_declaration
  (func_declarator
    (identifier) @code_function.name)) @code_function.def

; auto foo() { ... } — auto return-type form; identifier is a direct child.
(auto_func_declaration
  (identifier) @code_function.name) @code_function.def

; ── Module + imports ─────────────────────────────────────────────────────────
; module pkg.sub;  → a module named by its fully-qualified path.
(module_declaration
  (module_fully_qualified_name) @code_module.name) @code_module.def
(module_declaration
  (module_name) @code_module.name) @code_module.def

; import a.b.c;  /  import a.b.c : sym;  — anchor on the statement node; the module
; path lives in module_fully_qualified_name (dotted) or module_name (single) inside
; the import node. Both the plain and the selective-binding (import_bindings) forms
; are handled. (Verified against the real parse tree — `import` is matchable here.)
(import_declaration
  (import_list
    (import (module_fully_qualified_name) @import.source))) @import
(import_declaration
  (import_list
    (import (module_name) @import.source))) @import
(import_declaration
  (import_list
    (import_bindings
      (import (module_fully_qualified_name) @import.source)))) @import
(import_declaration
  (import_list
    (import_bindings
      (import (module_name) @import.source)))) @import

; ── Calls ────────────────────────────────────────────────────────────────────
; foo(args) — direct, UNqualified call: postfix_expression whose callee is a
; qualified_identifier holding exactly one identifier (the `. (identifier) .`
; anchors make it the sole child), followed by an argument_list. Qualified /
; method calls (a.b(), pkg.f()) are intentionally NOT captured: the callee is a
; left-nested qualified_identifier whose trailing name cannot be selected at a
; bounded depth, so capturing it would risk emitting the wrong name. Verified
; against the real parse tree.
(postfix_expression
  (qualified_identifier . (identifier) @call.function .)
  (argument_list)) @call
