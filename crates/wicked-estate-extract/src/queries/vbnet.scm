; wicked_estate VB.NET extraction queries — @code_* convention.
; Verified against tree-sitter-vb-dotnet 0.1.0 (jamie8johnson) node-types.json.

; ── Type definitions ──────────────────────────────────────────────────────────
(class_block
  name: (identifier) @code_class.name
) @code_class.def

(module_block
  name: (identifier) @code_module.name
) @code_module.def

(interface_block
  name: (identifier) @code_interface.name
) @code_interface.def

(structure_block
  name: (identifier) @code_struct.name
) @code_struct.def

(enum_block
  name: (identifier) @code_enum.name
) @code_enum.def

; ── Namespace ─────────────────────────────────────────────────────────────────
(namespace_block
  name: (namespace_name) @code_namespace.name
) @code_namespace.def

; ── Method / Sub / Function definitions ──────────────────────────────────────
(method_declaration
  name: (identifier) @code_function.name
) @code_function.def

; ── Property definitions ──────────────────────────────────────────────────────
(property_declaration
  name: (identifier) @code_property.name
) @code_property.def

; ── Imports ───────────────────────────────────────────────────────────────────
(imports_statement
  namespace: (namespace_name) @import.source
) @import

; ── Heritage: Inherits / Implements ──────────────────────────────────────────
; Match inherits_clause and implements_clause wherever they appear in the tree.
; The VB.NET grammar puts them immediately after the class/interface name (before
; the first _terminator), but tree-sitter error-recovery also produces named nodes
; when they appear on separate lines — both cases are captured here.
(inherits_clause
  (type) @code_extends.target
) @code_extends.def

(implements_clause
  (type) @code_implements.target
) @code_implements.def

; ── Call sites ────────────────────────────────────────────────────────────────
; invocation: target(args) — method or function invocation.
(invocation
  target: (_) @call.function
) @call

; new_expression: New TypeName — object construction.
(new_expression
  type: (type) @call.function
) @call
