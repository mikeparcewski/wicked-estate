; wicked_estate CFML (tag-based) extraction queries — @code_* convention.
; Node types verified against cfmleditor/tree-sitter-cfml (cfml grammar), whose own
; queries/tags.scm is the reference these mirror. The tag grammar also parses embedded
; <cfscript> blocks, so script function/method definitions are captured here too.

; ── Script function/method definitions (embedded <cfscript>) ──────────────────
(function_declaration
  name: (identifier) @code_function.name) @code_function.def

(function_expression
  name: (identifier) @code_function.name) @code_function.def

(method_definition
  name: (property_identifier) @code_function.name) @code_function.def

; ── <cffunction name="..."> tag ───────────────────────────────────────────────
(cf_function_tag
  (cf_attribute
    (cf_attribute_name) @_name
    (quoted_cf_attribute_value
      (attribute_value) @code_function.name))
  (#eq? @_name "name")) @code_function.def

; ── <cfcomponent name="..."> tag → a class/component ──────────────────────────
(cf_component_open_tag
  (cf_tag_attributes
    (cf_attribute
      (cf_attribute_name) @_name
      (quoted_cf_attribute_value
        (attribute_value) @code_class.name)))
  (#eq? @_name "name")) @code_class.def

; ── Function calls (script expressions) ───────────────────────────────────────
(call_expression
  function: (identifier) @call.function) @call

(call_expression
  function: (member_expression
    property: (property_identifier) @call.function)) @call
