; wicked_estate JavaScript extraction queries — @code_* convention.

; ── Callable definitions ─────────────────────────────────────────────────────

; Function declarations
(function_declaration
  name: (identifier) @code_function.name
  parameters: (formal_parameters) @code_function.params
  body: (statement_block) @code_function.body
) @code_function.def

; Arrow functions assigned to variables
(variable_declarator
  name: (identifier) @code_function.name
  value: (arrow_function)
) @code_function.def

; Class declarations
(class_declaration
  name: (identifier) @code_class.name
  body: (class_body) @code_class.body
) @code_class.def

; Class expressions assigned to variables
(variable_declarator
  name: (identifier) @code_class.name
  value: (class
    body: (class_body) @code_class.body
  )
) @code_class.def

; Class extends clause (JS grammar: class_heritage has identifier directly, no extends_clause wrapper)
(class_declaration
  name: (identifier) @code_class.name
  (class_heritage (identifier) @code_extends.target)
) @code_extends.def

; Method definitions: covers regular, async, static, get, set, and constructor
; (all parse as method_definition — object-literal methods too)
(method_definition
  name: (property_identifier) @code_method.name
  parameters: (formal_parameters) @code_method.params
  body: (statement_block) @code_method.body
) @code_method.def

; Arrow-function class fields: handler = () => {}
; JS grammar uses field_definition with property: field (TS uses public_field_definition with name:)
(field_definition
  property: (property_identifier) @code_method.name
  value: (arrow_function)
) @code_method.def

; Object-valued class fields: `x = { ... }` / `#x = { ... }` become Term-suffixed
; Field defs (scm-anchors D6) — same intent as typescript.scm, spelled on the JS
; grammar's field_definition/property: shape (verified against tree-sitter-
; javascript 0.23.1 node-types.json). The literal's members truncate at the Term
; field (module-flat) instead of merging with the class's real methods.
; Computed-name fields (`[k] = { ... }`) stay uncaptured — documented residual.
(field_definition
  property: [(property_identifier) (private_property_identifier)] @code_field.name
  value: (object)
) @code_field.def

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

; ── Direct value-flow sites ─────────────────────────────────────────────────
;
; The JavaScript spelling of typescript.scm's value-lineage block (#213). Same classification
; grammar — `@flow.<semantics>.<evidence>.<construct>`, rule id `javascript/<evidence>/<construct>`
; (see `wicked_estate_core::flow` and docs/ENGINE-CONTRACT.md §3.2) — on the JS node shapes:
; formal parameters are bare `identifier` / `assignment_pattern` (no `required_parameter`
; wrapper) and class fields are `field_definition` (`property:`). The Angular `@Input()` and
; route-param conventions are TypeScript-decorator shaped and are not carried here.

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

; out = tainted — a reassignment is a value hop too (#217).
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

; const t = this.tenantId — a read of the class-owned field slot (#215).
(variable_declarator
  name: (identifier) @flow.consumer.local
  value: (member_expression
    object: (this)
    property: (property_identifier) @flow.producer.field)
) @flow.value.syntax.field_read

; Callable parameters and simple returns. JS formal parameters are bare: `id` or `id = dflt`.
; Each fills its own positional `value_params` slot; destructured / rest parameters stay `null`.
(function_declaration
  parameters: (formal_parameters
    [(identifier) @flow.parameter.local
     (assignment_pattern
      left: (identifier) @flow.parameter.local)]))

(method_definition
  parameters: (formal_parameters
    [(identifier) @flow.parameter.local
     (assignment_pattern
      left: (identifier) @flow.parameter.local)]))

(return_statement
  (identifier) @flow.return.local)

; Return barriers — see typescript.scm: a `return` inside a callback is the callback's value, not
; the enclosing method's; `.owned` marks a body that is its own definition's body.
(arrow_function body: (statement_block) @flow.barrier)
(function_expression body: (statement_block) @flow.barrier)
; Generators are callables too, and no definition pattern captures them: their returns are the
; generator's, never the enclosing definition's.
(generator_function body: (statement_block) @flow.barrier)
(generator_function_declaration body: (statement_block) @flow.barrier)

(variable_declarator
  value: (arrow_function body: (statement_block) @flow.barrier.owned))
(field_definition
  value: (arrow_function body: (statement_block) @flow.barrier.owned))

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
(field_definition value: (arrow_function) @flow.scope.owned)

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

; Generic call value-flow facts, carried as UnresolvedRef hints; edges only once the Calls
; resolver binds the exact site.
(call_expression
  arguments: (arguments
    (identifier) @call.arg.local) @call.arguments
) @call.value

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
;
; `@call.value` anchors (as typescript.scm): the Calls ref is keyed by the whole call_expression,
; the same span the call-argument facts above use, so the resolver can join them per site.

; Function calls — simple: foo()
(call_expression
  function: (identifier) @call.function
  arguments: (arguments) @call.args
) @call.value

; Method calls — member expression: a.b(), a.b.c(), a?.b()
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
