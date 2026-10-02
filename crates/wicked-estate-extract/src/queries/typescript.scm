; wicked_estate TypeScript extraction queries — @code_* convention.

; ── Callable definitions ─────────────────────────────────────────────────────

; Function declarations
(function_declaration
  name: (identifier) @code_function.name
  parameters: (formal_parameters) @code_function.params
  body: (statement_block) @code_function.body
) @code_function.def

; Arrow functions assigned to a const/let/var binding (module-scope and class-field)
(variable_declarator
  name: (identifier) @code_function.name
  value: (arrow_function)
) @code_function.def

; Method definitions: covers regular, async, static, get, set, and constructor
; (they all parse as method_definition with property_identifier name)
(method_definition
  name: (property_identifier) @code_method.name
  parameters: (formal_parameters) @code_method.params
  body: (statement_block) @code_method.body
) @code_method.def

; Arrow-function class fields: handler = () => {}
; public_field_definition with an arrow_function value — these are methods too
(public_field_definition
  name: (property_identifier) @code_method.name
  value: (arrow_function)
) @code_method.def

; Object-valued class fields: `x = { ... }` / `#x = { ... }` become Term-suffixed
; Field defs (scm-anchors D6): the field is then a container in the
; enclosing_chain walk, so the literal's members TRUNCATE at it (module-flat)
; instead of nesting under the class and merging with the class's real methods.
; Computed-name fields (`[k] = { ... }`) stay uncaptured — documented residual
; (their literal members still merge with same-named class methods).
(public_field_definition
  name: [(property_identifier) (private_property_identifier)] @code_field.name
  value: (object)
) @code_field.def

; Interface method signatures: interface Foo { bar(x: T): R; }
(interface_body
  (method_signature
    name: (property_identifier) @code_method.name
    parameters: (formal_parameters) @code_method.params
  ) @code_method.def
)

; Class declarations
(class_declaration
  name: (type_identifier) @code_class.name
  body: (class_body) @code_class.body
) @code_class.def

; Class extends clause
(class_declaration
  name: (type_identifier) @code_class.name
  (class_heritage
    (extends_clause value: (identifier) @code_extends.target))
) @code_extends.def

; Class implements clause
(class_declaration
  name: (type_identifier) @code_class.name
  (class_heritage
    (implements_clause (type_identifier) @code_implements.target))
) @code_implements.def

; Interface declarations
(interface_declaration
  name: (type_identifier) @code_interface.name
  body: (interface_body) @code_interface.body
) @code_interface.def

; Interface extends interface
(interface_declaration
  name: (type_identifier) @code_interface.name
  (extends_type_clause (type_identifier) @code_extends.target)
) @code_extends.def

; Type alias declarations
(type_alias_declaration
  name: (type_identifier) @code_type.name
  value: (_) @code_type.value
) @code_type.def

; Enum declarations
(enum_declaration
  name: (identifier) @code_enum.name
) @code_enum.def

; ── Scoped constant/variable capture ────────────────────────────────────────
; Only MEANINGFUL declarations are captured:
;   (a) top-level — lexical_declaration/variable_declaration that are direct
;       children of (program …)
;   (b) exported — inside (export_statement …)
; Function-local bindings (inside statement_block) are intentionally excluded.

; Top-level const declarations
(program
  (lexical_declaration kind: "const"
    (variable_declarator
      name: (identifier) @code_constant.name)
  ) @code_constant.def)

; Exported const declarations
(export_statement
  (lexical_declaration kind: "const"
    (variable_declarator
      name: (identifier) @code_constant.name)
  ) @code_constant.def)

; Top-level let declarations
(program
  (lexical_declaration kind: "let"
    (variable_declarator name: (identifier) @code_variable.name)
  ) @code_variable.def)

; Exported let declarations
(export_statement
  (lexical_declaration kind: "let"
    (variable_declarator name: (identifier) @code_variable.name)
  ) @code_variable.def)

; Top-level var declarations (legacy)
(program
  (variable_declaration
    (variable_declarator name: (identifier) @code_variable.name)
  ) @code_variable.def)

; Exported var declarations (legacy)
(export_statement
  (variable_declaration
    (variable_declarator name: (identifier) @code_variable.name)
  ) @code_variable.def)

; ── ORM / framework-aware extraction (W6.2) ──────────────────────────────────
;
; TypeORM @Entity decorated class → NodeKind::Class
; Gate: the class_declaration must carry a @Entity() (or @Entity("table_name")) decorator.
; Uses the `decorator:` named field on class_declaration (tree-sitter-typescript 0.23+).

(class_declaration
  decorator: (decorator
    (call_expression
      function: (identifier) @_entity_dec
      (#eq? @_entity_dec "Entity")))
  name: (type_identifier) @code_class.name
) @code_class.def

; TypeORM column-like decorated property → NodeKind::Field
; Gate: property must carry one of the recognised TypeORM column decorators.
; public_field_definition carries `decorator:` as a named field.

(public_field_definition
  decorator: (decorator
    (call_expression
      function: (identifier) @_col_dec
      (#any-of? @_col_dec
        "Column"
        "PrimaryColumn"
        "PrimaryGeneratedColumn"
        "CreateDateColumn"
        "UpdateDateColumn"
        "DeleteDateColumn"
        "VersionColumn"
        "ViewColumn"
        "ObjectIdColumn"
        "JoinColumn"
        "JoinTable"
        "OneToOne"
        "OneToMany"
        "ManyToOne"
        "ManyToMany")))
  name: (property_identifier) @code_field.name
) @code_field.def

; ── Import statements ────────────────────────────────────────────────────────

(import_statement
  source: (string) @import.source
) @import

; Re-exports with a source are imports too: `export * from './y'`, `export { a } from './y'`.
(export_statement
  source: (string) @import.source
) @import

; Dynamic import: `import('./dyn')` — the grammar exposes `import` as a callable keyword.
(call_expression
  function: (import)
  arguments: (arguments . (string) @import.source)
) @import

; CommonJS require: `require('./z')` — gated on the callee text so ordinary calls never match.
(call_expression
  function: (identifier) @_req
  arguments: (arguments . (string) @import.source)
  (#eq? @_req "require")
) @import

; TS import-equals: `import r = require('./req')`.
(import_statement
  (import_require_clause
    source: (string) @import.source)
) @import

; ── Direct value-flow sites ─────────────────────────────────────────────────
;
; The whole-pattern anchor capture names the fact's CLASSIFICATION, so the classification is data
; in this file rather than a `match language` arm in Rust:
;
;     @flow.<semantics>.<evidence>.<construct>
;
;   <semantics>  value     — the producer's value becomes the consumer's value, whole
;                influence — the producer contributes to it (NOT a claim the value is preserved)
;   <evidence>   syntax     — the AST proves this fact at this site
;                convention — a framework naming/shape match the parser CANNOT prove. An
;                             identifier named `Input` is not necessarily `@angular/core`'s
;                             `Input`; a receiver named `route` is not necessarily an
;                             `ActivatedRoute`. These edges are emitted at the Heuristic tier.
;   <construct>  free-form; becomes part of the stable rule id
;                `typescript/<evidence>/<construct>` carried on every emitted edge.
;
; See `wicked_estate_core::flow` and docs/ENGINE-CONTRACT.md §3.2.

; const a = b / let a = b / var a = b
(variable_declarator
  name: (identifier) @flow.consumer.local
  value: (identifier) @flow.producer.local
) @flow.value.syntax.assignment

; const c = a + b
(variable_declarator
  name: (identifier) @flow.consumer.local
  value: (binary_expression
    left: (identifier) @flow.producer.local
    right: (identifier) @flow.producer.local)
) @flow.influence.syntax.expression

; this.field = value
(expression_statement
  (assignment_expression
    left: (member_expression
      object: (this)
      property: (property_identifier) @flow.consumer.field)
    right: (identifier) @flow.producer.local)
) @flow.value.syntax.assignment

; const id = customer.id
(variable_declarator
  name: (identifier) @flow.consumer.local
  value: (member_expression
    object: (identifier)
    property: (property_identifier)) @flow.producer.property
) @flow.value.syntax.property_read

; @Input() tenantId
(public_field_definition
  decorator: (decorator
    (call_expression
      function: (identifier) @_input_dec
      (#eq? @_input_dec "Input")))
  name: (property_identifier) @flow.consumer.field @flow.producer.angular_input
) @flow.value.convention.angular_input

; const routeId = route.snapshot.paramMap.get('id')
(variable_declarator
  name: (identifier) @flow.consumer.local
  value: (call_expression
    function: (member_expression
      object: (member_expression
        object: (member_expression
          object: (identifier) @_route_obj
          property: (property_identifier) @_snapshot)
        property: (property_identifier) @_param_map)
      property: (property_identifier) @_get)
    arguments: (arguments (string) @flow.producer.route_param)
    (#eq? @_route_obj "route")
    (#eq? @_snapshot "snapshot")
    (#eq? @_param_map "paramMap")
    (#eq? @_get "get"))
) @flow.value.convention.route_param

; Callable parameters and simple returns become stable value nodes/edges. The call resolver
; later joins exact call-site argument facts to these callable-owned values.
(function_declaration
  parameters: (formal_parameters
    (required_parameter
      pattern: (identifier) @flow.parameter.local)))

(method_definition
  parameters: (formal_parameters
    (required_parameter
      pattern: (identifier) @flow.parameter.local)))

(return_statement
  (identifier) @flow.return.local)

; Return barriers. A `return x` is only the OWNER callable's return value when no other callable
; body lies between them: in the canonical RxJS shape
; `svc.get(id).subscribe((customer) => { return customer; })` the returned value belongs to the
; callback, not to the enclosing method, and anonymous callables are not definition records — so
; without this the method's return value is asserted to be the callback's, at confidence 1.00
; (wicked-estate#207 review, C5a). `.owned` marks a body that IS its own definition's body (an
; arrow bound to a const or a class field is captured as a def above), whose returns are kept.
(arrow_function body: (statement_block) @flow.barrier)
(function_expression body: (statement_block) @flow.barrier)

(variable_declarator
  value: (arrow_function body: (statement_block) @flow.barrier.owned))
(public_field_definition
  value: (arrow_function body: (statement_block) @flow.barrier.owned))

; A named callable's body is an OWNED barrier even when the callable is declared inside a
; callback: `items.map(() => { function inner() { return v; } })` returns `v` from `inner`, and
; `inner` is a definition record, so the barrier must stop at it rather than at the arrow.
(function_declaration body: (statement_block) @flow.barrier.owned)
(method_definition body: (statement_block) @flow.barrier.owned)

; Generic call value-flow facts. These are carried as UnresolvedRef hints and only become
; edges when the existing Calls resolver binds the exact site.
(call_expression
  arguments: (arguments
    (identifier) @call.arg.local) @call.arguments
) @call.value

; this.field passed as an argument, e.g. this.loadCustomer(service, this.customerId)
(call_expression
  arguments: (arguments
    (member_expression
      object: (this)
      property: (property_identifier) @call.arg.field)) @call.arguments
) @call.value

(variable_declarator
  name: (identifier) @call.result.local
  value: (call_expression) @call.value)

; ── Call sites ───────────────────────────────────────────────────────────────

; Function calls — simple: foo()
(call_expression
  function: (identifier) @call.function
  arguments: (arguments) @call.args
) @call.value

; Method calls — member expression: a.b(), a.b.c(), a?.b()
; The optional_chain variant also uses member_expression with property_identifier
(call_expression
  function: (member_expression
    property: (property_identifier) @call.method
  )
) @call.value

; Constructor calls — new X()
(new_expression
  constructor: (identifier) @call.function
) @call.value

; Constructor calls — new a.B()
(new_expression
  constructor: (member_expression
    property: (property_identifier) @call.method
  )
) @call.value
