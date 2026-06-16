; wicked_estate Java extraction queries — @code_* convention.

; Class declarations
(class_declaration
  name: (identifier) @code_class.name
  body: (class_body) @code_class.body
) @code_class.def

; Enum declarations (treated as classes)
(enum_declaration
  name: (identifier) @code_class.name
  body: (enum_body) @code_class.body
) @code_class.def

; Class extends superclass
(class_declaration
  name: (identifier) @code_class.name
  (superclass (type_identifier) @code_extends.target)
) @code_extends.def

; Class implements interfaces (one capture per interface)
(class_declaration
  name: (identifier) @code_class.name
  (super_interfaces (type_list (type_identifier) @code_implements.target))
) @code_implements.def

; Interface declarations
(interface_declaration
  name: (identifier) @code_interface.name
  body: (interface_body) @code_interface.body
) @code_interface.def

; Method declarations
(method_declaration
  type: (_) @code_method.return_type
  name: (identifier) @code_method.name
  parameters: (formal_parameters) @code_method.params
  body: (block)? @code_method.body
) @code_method.def

; Constructor declarations
(constructor_declaration
  name: (identifier) @code_method.name
  parameters: (formal_parameters) @code_method.params
  body: (constructor_body) @code_method.body
) @code_method.def

; Field declarations (includes static final — Java grammar uses field_declaration for these)
(field_declaration
  declarator: (variable_declarator
    name: (identifier) @code_field.name
  )
) @code_field.def

; Import declarations
(import_declaration
  (scoped_identifier) @import.source
) @import

; Method invocations
(method_invocation
  name: (identifier) @call.method
  arguments: (argument_list) @call.args
) @call

; ── Framework relationships (Spring) — emitted as Other(<tag>) edges ──────────
; These use the generic @di.* / @route.* capture roles handled by the tree-sitter
; pipeline; the relationship is DATA here, not Rust per-language logic.

; DI: @Autowired field injection.  source = the class, target = the injected type.
; Nested under class_declaration so the injecting class is bound unambiguously.
(class_declaration
  name: (identifier) @di.source.name
  body: (class_body
    (field_declaration
      (modifiers
        (marker_annotation name: (identifier) @_di_field_anno))
      type: (type_identifier) @di.target)))
  (#eq? @_di_field_anno "Autowired")

; DI: constructor injection.  Each @Autowired-constructor parameter type is an injected
; collaborator of the enclosing class.
(class_declaration
  name: (identifier) @di.source.name
  body: (class_body
    (constructor_declaration
      (modifiers
        (marker_annotation name: (identifier) @_di_ctor_anno))
      parameters: (formal_parameters
        (formal_parameter type: (type_identifier) @di.target)))))
  (#eq? @_di_ctor_anno "Autowired")

; Route: @GetMapping("/x") / @PostMapping(...) / … with a bare string-literal path.
; source = the route/path node, target = the handler method.
(class_declaration
  body: (class_body
    (method_declaration
      (modifiers
        (annotation
          name: (identifier) @_route_anno
          arguments: (annotation_argument_list
            (string_literal (string_fragment) @route.path))))
      name: (identifier) @route.handler.name)))
  (#any-of? @_route_anno
    "GetMapping" "PostMapping" "PutMapping" "DeleteMapping" "PatchMapping" "RequestMapping")

; Route: @RequestMapping(value = "/x") / (path = "/x") — element_value_pair form.
(class_declaration
  body: (class_body
    (method_declaration
      (modifiers
        (annotation
          name: (identifier) @_route_anno_kv
          arguments: (annotation_argument_list
            (element_value_pair
              value: (string_literal (string_fragment) @route.path)))))
      name: (identifier) @route.handler.name)))
  (#any-of? @_route_anno_kv
    "GetMapping" "PostMapping" "PutMapping" "DeleteMapping" "PatchMapping" "RequestMapping")
