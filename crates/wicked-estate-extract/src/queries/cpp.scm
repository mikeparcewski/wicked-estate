; wicked_estate C++ extraction queries — @code_* convention.

; Class definitions — body: is REQUIRED (D04-6/D6a): without it, forward
; declarations (`class Widget;`) and elaborated uses (`struct Foo *p;`) mint
; phantom nodes with the same SymbolId as the real definition, which the store's
; last-write-wins upsert then re-kinds/relocates.
(class_specifier
  name: (type_identifier) @code_class.name
  body: (field_declaration_list) @code_class.body
) @code_class.def

; Struct definitions — body: required (see class note)
(struct_specifier
  name: (type_identifier) @code_struct.name
  body: (field_declaration_list) @code_struct.body
) @code_struct.def

; Enum definitions — body: required (see class note)
(enum_specifier
  name: (type_identifier) @code_enum.name
  body: (enumerator_list) @code_enum.body
) @code_enum.def

; Namespace definitions
(namespace_definition
  name: (namespace_identifier) @code_namespace.name
) @code_namespace.def

; Top-level function definitions (identifier declarator)
(function_definition
  declarator: (function_declarator
    declarator: (identifier) @code_function.name
  )
) @code_function.def

; Method definitions inside classes (field_identifier declarator)
(function_definition
  declarator: (function_declarator
    declarator: (field_identifier) @code_method.name
  )
) @code_method.def

; Out-of-line member definitions: `void Foo::reset() {}` (D6e) — the qualifier
; is the member's OWNER (scm-anchors D8, scheme 3). qualified_identifier.scope
; admits exactly {namespace_identifier, template_type, decltype, dependent_name}
; (tree-sitter-cpp 0.23.4 node-types.json); a class-name qualifier parses as
; namespace_identifier, so `void Foo::reset() {}` mints `<module>/Foo#reset().`
; and `void Foo<T>::reset() {}` anchors under Foo via the template_type branch.
; R-DEF-LOSS: the scope alternation is OPTIONAL (`?`) — decltype/dependent_name
; qualifiers degrade to OWNERLESS module-flat defs, never dropped defs. Single-
; level qualification only: `void Ns::Foo::bar()` at file scope nests
; qualified_identifiers and is not matched (write the definition inside its
; namespace to be captured).
; HAZARD (pinned by cpp_member_proto_def_cross_file_single_id_hazard): with the
; owner, a foo.h in-class prototype (D6b) and the foo.cpp out-of-line definition
; mint ONE SymbolId across TWO files (module_path strips one extension) —
; nodes.file flaps last-write-wins, remove_file deletes by file, the digest skip
; never re-extracts the survivor. Store-side fix filed via merge note M4 (the
; program's header/impl identity decision).
; HAZARD, namespace direction (R2-COR-1; pinned by
; cpp_namespace_qualified_free_fn_cross_kind_collision_known_defect): the
; qualifier ambiguity cuts BOTH ways — a NAMESPACE qualifier also parses as
; namespace_identifier, so `void ns::helper(int) {}` at file scope mints
; `<module>/ns#helper().` with kind Method: the SAME id an in-namespace
; `void helper() {}` definition mints with kind Function (containment nests it
; under the `ns#` namespace anchor). Cross-kind same-id → store re-kind flap.
; No query-level fix exists (the grammar cannot separate class from namespace
; qualifiers); folded into the M4 identity decision.
(function_definition
  declarator: (function_declarator
    declarator: (qualified_identifier
      scope: [
        (namespace_identifier) @code_method.owner
        (template_type name: (type_identifier) @code_method.owner)
      ]?
      name: (identifier) @code_method.name)
  )
) @code_method.def

; Member function prototypes: `int bar(int);` inside a class/struct body is a
; field_declaration wrapping a function_declarator (D6b). This also classifies
; pure virtuals (`virtual void pure() = 0;` parses as field_declaration).
; NOTE: FREE function prototypes (`int freestanding();` at namespace scope) are
; STILL DEFERRED (D6d; docs/recon/scm-anchors.md D8) — the recorded deferral
; terms require a program-level owner AND an identity DECISION for header/impl
; proto+def node identity, and only the owner is recorded: a header prototype
; and its .cpp definition share a SymbolId (module strips one extension), and
; remove_file + the digest skip make that a data-loss path whose fix is
; store-side (merge note M4). The ready per-parent pattern set lives in
; docs/recon/extraction-gaps.md §D6(d).
(field_declaration
  declarator: (function_declarator
    declarator: (field_identifier) @code_method.name)
) @code_method.def

; Member fields (D6c) — an explicit declarator ALTERNATION, not a wildcard: a
; wildcard would also capture function_declarator and emit prototypes as Field
; (kind conflict with the Method above).
(field_declaration
  declarator: [
    (field_identifier) @code_field.name
    (pointer_declarator declarator: (field_identifier) @code_field.name)
    (array_declarator declarator: (field_identifier) @code_field.name)
    (reference_declarator (field_identifier) @code_field.name)
  ]
) @code_field.def

; Object-like macros
(preproc_def
  name: (identifier) @code_constant.name
) @code_constant.def

; Function-like macros
(preproc_function_def
  name: (identifier) @code_function.name
) @code_function.def

; Type aliases (typedef) — two-pattern set (§11 / D04), same shape as c.scm.
; The old unconstrained pattern double-matched `typedef struct X X;` (TypeAlias with
; the same SymbolId as the real Struct → last-write-wins re-kind in the store).
; (i) tag-named typedefs emit only when the alias name differs from the tag name;
; (ii) anonymous-tag and non-tag typedefs always emit. The idiom is ubiquitous in C
; headers, which route to this grammar (.h → cpp).
(type_definition
  type: (struct_specifier name: (type_identifier) @_tag_name)
  declarator: (type_identifier) @code_type.name
  (#not-eq? @_tag_name @code_type.name)
) @code_type.def

(type_definition
  type: (enum_specifier name: (type_identifier) @_tag_name)
  declarator: (type_identifier) @code_type.name
  (#not-eq? @_tag_name @code_type.name)
) @code_type.def

(type_definition
  type: (union_specifier name: (type_identifier) @_tag_name)
  declarator: (type_identifier) @code_type.name
  (#not-eq? @_tag_name @code_type.name)
) @code_type.def

; (no macro_type_specifier here — that node exists only in the C grammar)
(type_definition
  type: [
    (struct_specifier !name)
    (enum_specifier !name)
    (union_specifier !name)
    (primitive_type)
    (sized_type_specifier)
    (type_identifier)
  ]
  declarator: (type_identifier) @code_type.name
) @code_type.def

; using alias: using X = Y
(alias_declaration
  name: (type_identifier) @code_type.name
) @code_type.def

; Include directives
(preproc_include
  path: (system_lib_string) @import.source
) @import

(preproc_include
  path: (string_literal) @import.source
) @import

; Function calls — simple
(call_expression
  function: (identifier) @call.function
) @call

; Method calls via field expression: obj.method()
(call_expression
  function: (field_expression
    field: (field_identifier) @call.method
  )
) @call.method
