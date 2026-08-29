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

; Annotation type declarations (`public @interface Marker { … }`) — the generic
; interface role expresses them; no dedicated role needed (D04-7)
(annotation_type_declaration
  name: (identifier) @code_interface.name
  body: (annotation_type_body) @code_interface.body
) @code_interface.def

; Annotation type elements (`String value();`, `int priority() default 0;`) —
; they are the annotation's members, emitted as methods (D04-7)
(annotation_type_element_declaration
  name: (identifier) @code_method.name
) @code_method.def

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

; NOTE on predicate placement: tree-sitter only applies a `#eq?`/`#any-of?`/`#match?` predicate
; when it is nested INSIDE the pattern's outermost parentheses (the parser associates a predicate
; with the S-expression that lexically contains it). A predicate written *after* the pattern's
; final `)` is silently ignored — the pattern then fires on every structural match regardless of
; the annotation name. Every framework predicate below therefore sits before the closing `)`.

; DI: @Autowired / @Inject / @Resource field injection.  source = the class, target = the
; injected type.  Nested under class_declaration so the injecting class is bound unambiguously.
; @Inject (JSR-330) and @Resource (JSR-250) wire the same dependency as @Autowired.
(class_declaration
  name: (identifier) @di.source.name
  body: (class_body
    (field_declaration
      (modifiers
        (marker_annotation name: (identifier) @_di_field_anno))
      type: (type_identifier) @di.target))
  (#any-of? @_di_field_anno "Autowired" "Inject" "Resource"))

; DI: constructor injection.  Each @Autowired / @Inject constructor parameter type is an injected
; collaborator of the enclosing class.
(class_declaration
  name: (identifier) @di.source.name
  body: (class_body
    (constructor_declaration
      (modifiers
        (marker_annotation name: (identifier) @_di_ctor_anno))
      parameters: (formal_parameters
        (formal_parameter type: (type_identifier) @di.target))))
  (#any-of? @_di_ctor_anno "Autowired" "Inject" "Resource"))

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
      name: (identifier) @route.handler.name))
  (#any-of? @_route_anno
    "GetMapping" "PostMapping" "PutMapping" "DeleteMapping" "PatchMapping" "RequestMapping"))

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
      name: (identifier) @route.handler.name))
  (#any-of? @_route_anno_kv
    "GetMapping" "PostMapping" "PutMapping" "DeleteMapping" "PatchMapping" "RequestMapping"))

; ── Event pub/sub — emitted as Other("event-listens" / "event-emits") edges ───
; source = dependent (engine contract): the LISTENER for event-listens, the EMITTER for
; event-emits. Targets are a real event TYPE (resolved cross-file like di-wired) when the
; framework hands us a type, or a synthetic TOPIC node when it only hands us a string.

; @EventListener — Spring application-event listener.  The handler's first parameter type is
; the event.  source = listener method, target = the event type (resolved cross-file).
(method_declaration
  (modifiers
    (marker_annotation name: (identifier) @_evt_listener_anno))
  name: (identifier) @event.listener.name
  parameters: (formal_parameters
    (formal_parameter type: (type_identifier) @event.type))
  (#eq? @_evt_listener_anno "EventListener"))

; @KafkaListener(topics = "t") — message listener bound to a topic string.
; source = listener method, target = synthetic topic node.
(method_declaration
  (modifiers
    (annotation
      name: (identifier) @_evt_kafka_anno
      arguments: (annotation_argument_list
        (element_value_pair
          value: (string_literal (string_fragment) @event.topic)))))
  name: (identifier) @event.listener.name
  (#eq? @_evt_kafka_anno "KafkaListener"))

; publishEvent(new FooEvent()) — Spring ApplicationEventPublisher.  source = enclosing method,
; target = the published event type.
(method_invocation
  name: (identifier) @_emit_method
  arguments: (argument_list
    (object_creation_expression
      type: (type_identifier) @event.emit.type))
  (#eq? @_emit_method "publishEvent"))

; kafkaTemplate.send("topic", payload) — source = enclosing method, target = synthetic topic.
(method_invocation
  object: (identifier) @_emit_recv
  name: (identifier) @_emit_send
  arguments: (argument_list
    .
    (string_literal (string_fragment) @event.emit.topic))
  (#eq? @_emit_send "send")
  (#match? @_emit_recv "[Tt]emplate$"))
