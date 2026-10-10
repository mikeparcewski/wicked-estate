; wicked_estate TSX extraction queries — @code_* convention.
; TSX shares the TypeScript grammar (LANGUAGE_TSX) so node types are identical.

; ── Callable definitions ─────────────────────────────────────────────────────

; Function declarations
(function_declaration
  name: (identifier) @code_function.name
  parameters: (formal_parameters) @code_function.params
  body: (statement_block) @code_function.body
) @code_function.def

; Arrow functions assigned to variable bindings
(variable_declarator
  name: (identifier) @code_function.name
  value: (arrow_function)
) @code_function.def

; Method definitions: covers regular, async, static, get, set, and constructor
(method_definition
  name: (property_identifier) @code_method.name
  parameters: (formal_parameters) @code_method.params
  body: (statement_block) @code_method.body
) @code_method.def

; Arrow-function class fields: handler = () => {}
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

; `abstract class` declarations are classes too (TS-S3): without this an abstract base — the
; usual home of an inherited Angular `@Input()` — has no node, and its fields have no type owner.
(abstract_class_declaration
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
; Mirrors typescript.scm (#213): the TSX grammar is a superset of the TypeScript grammar, so the
; node shapes are identical and the value-lineage patterns carry over unchanged. Keep the two in
; step when either changes.
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
;                `tsx/<evidence>/<construct>` carried on every emitted edge.
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

; out = tainted — a REASSIGNMENT is a value hop too (#217): without it `let out = trusted;
; out = tainted; return out;` stored only the `trusted` hop and the partial lineage looked whole.
(expression_statement
  (assignment_expression
    left: (identifier) @flow.consumer.local
    right: (identifier) @flow.producer.local)
) @flow.value.syntax.reassignment

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

; const t = this.tenantId — a read of the CLASS's field (#215). `@flow.producer.field` mints the
; same class-owned `:field:<name>` slot an `@Input()` / `this.f = v` writes, so the read joins that
; slot instead of minting a per-method `:property:` node, and `@Input() tenantId` reaches `t`.
(variable_declarator
  name: (identifier) @flow.consumer.local
  value: (member_expression
    object: (this)
    property: (property_identifier) @flow.producer.field)
) @flow.value.syntax.field_read

; TS-S3: every property and accessor of a DECORATED class (`@Component`, `@Directive`, however
; the decorator is spelled or imported: `@Cmp`, `@ng.Component`) gets its class-owned `:field:`
; slot, so compiler-resolved template bindings (semantic evidence, docs/ENGINE-CONTRACT.md §3.5)
; have a canonical endpoint. A slot only: no flow, no edge. Undecorated classes gain nothing.
; `@Component(...) export class X` puts the decorator on the export statement.
(class_declaration
  decorator: (decorator)
  body: (class_body
    (public_field_definition
      name: (property_identifier) @flow.slot.field)))

(export_statement
  decorator: (decorator)
  declaration: (class_declaration
    body: (class_body
      (public_field_definition
        name: (property_identifier) @flow.slot.field))))

(class_declaration
  decorator: (decorator)
  body: (class_body
    (method_definition
      ["get" "set"]
      name: (property_identifier) @flow.slot.field)))

(export_statement
  decorator: (decorator)
  declaration: (class_declaration
    body: (class_body
      (method_definition
        ["get" "set"]
        name: (property_identifier) @flow.slot.field))))

(abstract_class_declaration
  decorator: (decorator)
  body: (class_body
    (public_field_definition
      name: (property_identifier) @flow.slot.field)))

(export_statement
  decorator: (decorator)
  declaration: (abstract_class_declaration
    body: (class_body
      (public_field_definition
        name: (property_identifier) @flow.slot.field))))

(abstract_class_declaration
  decorator: (decorator)
  body: (class_body
    (method_definition
      ["get" "set"]
      name: (property_identifier) @flow.slot.field)))

(export_statement
  decorator: (decorator)
  declaration: (abstract_class_declaration
    body: (class_body
      (method_definition
        ["get" "set"]
        name: (property_identifier) @flow.slot.field))))

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
; `optional_parameter` (`a?: string`) fills its own positional slot like a required one (#213):
; without it `value_params` held `null` there and the call-site argument hop was lost.
(function_declaration
  parameters: (formal_parameters
    [(required_parameter
      pattern: (identifier) @flow.parameter.local)
     (optional_parameter
      pattern: (identifier) @flow.parameter.local)]))

(method_definition
  parameters: (formal_parameters
    [(required_parameter
      pattern: (identifier) @flow.parameter.local)
     (optional_parameter
      pattern: (identifier) @flow.parameter.local)]))

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
; Generators are callables too, and no definition pattern captures them: their returns are the
; generator's, never the enclosing definition's.
(generator_function body: (statement_block) @flow.barrier)
(generator_function_declaration body: (statement_block) @flow.barrier)

(variable_declarator
  value: (arrow_function body: (statement_block) @flow.barrier.owned))
(public_field_definition
  value: (arrow_function body: (statement_block) @flow.barrier.owned))

; A named callable's body is an OWNED barrier even when the callable is declared inside a
; callback: `items.map(() => { function inner() { return v; } })` returns `v` from `inner`, and
; `inner` is a definition record, so the barrier must stop at it rather than at the arrow.
(function_declaration body: (statement_block) @flow.barrier.owned)
; Only a method a definition pattern captures (a `property_identifier` name) owns its returns: a
; computed (`[key]() {}`) or private method has no definition record, so its body is a plain
; barrier and its `return` is dropped rather than attributed to the enclosing definition. The
; barrier map ORs `owned`, so a named method's body is owned by the second pattern.
(method_definition body: (statement_block) @flow.barrier)
(method_definition
  name: (property_identifier)
  body: (statement_block) @flow.barrier.owned)

; ── Value scopes (#216) ─────────────────────────────────────────────────────
;
; Value identity is owner-scoped (`{owner}:local:{name}`), so two distinct bindings with one name in
; one callable used to become ONE node and a false flow ran between them: a callback parameter
; shadowing the method's parameter, or a block-scoped `const` shadowing it. These captures give the
; extractor the binding structure. A reference resolves to the innermost declaration of its name
; whose scope contains it; a declaration in a nested scope is keyed `{owner}:local:{name}@{n}`,
; where `n` numbers the owner's nested scopes that bind THAT name, in source order (structural:
; line shifts and unrelated blocks keep it — ADR-002).
; A declaration in the owner's own body keeps the plain `{owner}:local:{name}`, and a read of the
; owner's parameter joins its `{owner}:param:{name}` slot.
;
;   @flow.scope            a block scope (let/const/class bindings live here)
;   @flow.scope.callable   a callback boundary (its parameters and its `var`s live here)
;   @flow.scope.owned      a callable that IS its definition's body (an arrow bound to a const or a
;                          class field): transparent, its parameters belong to the owner itself
;   @flow.declare.block    a let/const/catch/callback-parameter binding, bound in the innermost
;                          scope. The capture is a binding PATTERN: the extractor walks it for every
;                          name it binds (`{a, b: c, ...d}`, `[e = f]`), never into default values.
;   @flow.declare.var      the same for a `var` binding, bound in the innermost callable scope
(statement_block) @flow.scope
(for_statement) @flow.scope
(for_in_statement) @flow.scope
(catch_clause) @flow.scope
(switch_body) @flow.scope

(arrow_function) @flow.scope.callable
(function_expression) @flow.scope.callable
(generator_function) @flow.scope.callable

(variable_declarator value: (arrow_function) @flow.scope.owned)
(public_field_definition value: (arrow_function) @flow.scope.owned)

(lexical_declaration (variable_declarator name: (_) @flow.declare.block))
(variable_declaration (variable_declarator name: (_) @flow.declare.var))
(for_in_statement kind: "const" left: (_) @flow.declare.block)
(for_in_statement kind: "let" left: (_) @flow.declare.block)
(for_in_statement kind: "var" left: (_) @flow.declare.var)
(catch_clause parameter: (_) @flow.declare.block)

(arrow_function parameter: (identifier) @flow.declare.block)
(arrow_function parameters: (formal_parameters (_) @flow.declare.block))
(function_expression parameters: (formal_parameters (_) @flow.declare.block))
(generator_function parameters: (formal_parameters (_) @flow.declare.block))

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
