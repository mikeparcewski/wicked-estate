; wicked_estate Swift extraction queries — @code_* convention.
;
; Verified against tree-sitter-swift-0.7.1 node-types.json.
; Named nodes used:
;   class_declaration    — declaration_kind field ("class"|"struct"|"enum"|"actor"|"extension"),
;                          name: (type_identifier)
;   protocol_declaration — name: (type_identifier)
;   function_declaration — name: (simple_identifier) [also operator names — we gate to simple_identifier]
;   import_declaration   — child: (identifier) > (simple_identifier)+
;   call_expression      — child: (simple_identifier) for direct call
;                          OR (navigation_expression suffix: (navigation_suffix suffix: (simple_identifier)))
;   typealias_declaration — name field uses type nodes, but we use a wildcard (_)

; ── Class declarations ────────────────────────────────────────────────────────
(class_declaration
  "class"
  name: (type_identifier) @code_class.name
  body: (class_body) @code_class.body
) @code_class.def

; ── Struct declarations ───────────────────────────────────────────────────────
(class_declaration
  "struct"
  name: (type_identifier) @code_struct.name
  body: (class_body) @code_struct.body
) @code_struct.def

; ── Enum declarations ─────────────────────────────────────────────────────────
(class_declaration
  "enum"
  name: (type_identifier) @code_enum.name
) @code_enum.def

; ── Protocol declarations (interfaces) ───────────────────────────────────────
(protocol_declaration
  name: (type_identifier) @code_interface.name
  body: (protocol_body) @code_interface.body
) @code_interface.def

; ── Function declarations (top-level and inside bodies) ──────────────────────
; Gate on simple_identifier: skips operator function declarations
(function_declaration
  name: (simple_identifier) @code_function.name
  body: (function_body) @code_function.body
) @code_function.def

; ── Import declarations ───────────────────────────────────────────────────────
; (import_declaration (identifier) (simple_identifier)) — identifier wraps simple_identifiers
(import_declaration
  (identifier) @import.source
) @import

; ── Direct call expressions: foo(…) ──────────────────────────────────────────
; call_expression child sequence: (simple_identifier) (call_suffix)
(call_expression
  (simple_identifier) @call.function
) @call

; ── Method call expressions: foo.bar(…) ──────────────────────────────────────
; navigation_expression suffix field → navigation_suffix suffix field → simple_identifier
(call_expression
  (navigation_expression
    suffix: (navigation_suffix
      suffix: (simple_identifier) @call.method)
  )
) @call.method
