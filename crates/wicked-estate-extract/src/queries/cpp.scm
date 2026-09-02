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
; M4 RESOLVED (Option A — one logical symbol; ADR-002 third amendment,
; wicked-estate#152/#140): a foo.h in-class prototype (D6b) and the foo.cpp
; out-of-line definition minting ONE SymbolId across TWO files is the RECORDED
; CONVENTION, made safe store-side — per-(symbol, file) contribution rows, a
; definition-preferred derived primary, and remove_file survivor re-home
; replaced the last-write-wins flap / cross-file delete / digest-skip data
; loss (pinned: cpp_member_proto_def_cross_file_single_id_hazard +
; wicked_estate_core::conformance::multi_file_contribution_suite).
; Namespace direction of the qualifier ambiguity (R2-COR-1; pinned by
; cpp_namespace_qualified_free_fn_cross_kind_collision_known_defect): a
; NAMESPACE qualifier also parses as namespace_identifier, so
; `void ns::helper(int) {}` at file scope mints `<module>/ns#helper().` with
; kind Method — the SAME id an in-namespace `void helper() {}` definition mints
; with kind Function. No query-level fix exists (the grammar cannot separate
; class from namespace qualifiers); under the M4 convention the raw extraction
; stream keeps both kinds and the STORE derives one deterministic primary kind
; from the preferred contribution. The true fix — the overload disambiguator
; (parameter-type hash) — remains an OPEN residual, separately pinned
; (identity_disambiguator_is_none; a scheme change, ADR-002).
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
; `.decl` (not `.def`): the record is a DECLARATION contribution (M4 / Option A,
; wicked-estate#152) — same SymbolId as the out-of-line definition (identity is
; untouched); the store's multi-file contribution table prefers the DEFINITION
; record as the node's primary location/kind.
(field_declaration
  declarator: (function_declarator
    declarator: (field_identifier) @code_method.name)
) @code_method.decl

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

; FREE function prototypes (D6d, wicked-estate#140) — landed under the M4
; identity decision: Option A, one logical symbol (ADR-002 third amendment;
; scratch/proposals/ESTATE-M4-DECISION-BRIEF.md). A header prototype and its
; impl-file definition mint ONE SymbolId (module_path strips one extension:
; foo.h + foo.cpp share module `foo`); the prototype JOINS the definition's
; existing id — zero id churn. `.decl` marks the record as a DECLARATION
; contribution so the store's multi-file contribution table (#152) keeps the
; DEFINITION record as the node's primary location/kind; a header-only
; prototype with no definition mints the id alone as a declaration-primary node.
;
; PER-PARENT ANCHORED pattern set (docs/recon/extraction-gaps.md §D6(d)): the
; review's translation_unit-only anchor captures 0 prototypes in include-guarded
; headers (the guard wraps everything in preproc_ifdef), so one pattern per
; legal parent. The anchoring IS the false-positive guard (adversarial review of
; doc 04): body-local prototypes (`int localProto(int);` inside a function) and
; body-local most-vexing-parse object declarations (`Foo f(Foo());`) sit under
; compound_statement — matched by NO pattern here.
; ACCEPTED residuals (recorded in ADR-002 §Accepted residuals):
;   - a most-vexing-parse declaration AT TU/namespace scope CAN still match (it
;     IS a function declaration per [dcl.ambig.res]; the review recorded "accept
;     as documented"). Measured: tree-sitter resolves the ambiguity
;     context-dependently — a lone `Foo f(Foo());` parses as a function
;     declaration and emits; with sibling declarations it parses as an object
;     declaration and does not;
;   - a body-local prototype inside a preproc block inside a function body
;     (`void f() { #ifdef X\n int p(int); #endif }`) leaks through the
;     preproc_ifdef/preproc_if parents (negative ancestor predicates are not
;     expressible in tree-sitter queries);
;   - `extern "C" int f(int);` WITHOUT braces (parent = linkage_specification)
;     is not captured — the braced `extern "C" { ... }` form rides the
;     declaration_list parent;
;   - pointer-returning prototypes (`int* getPtr();`, pointer_declarator wraps
;     the function_declarator) are not captured — deliberately consistent with
;     the function_definition patterns above, which have the same shape gap.

; (1) translation-unit scope: `int freestanding();` in an unguarded file
(translation_unit
  (declaration
    declarator: (function_declarator
      declarator: (identifier) @code_function.name)
  ) @code_function.decl
)

; (2) include-guard / #ifdef blocks: the dominant header idiom
(preproc_ifdef
  (declaration
    declarator: (function_declarator
      declarator: (identifier) @code_function.name)
  ) @code_function.decl
)

; (3) #if blocks
(preproc_if
  (declaration
    declarator: (function_declarator
      declarator: (identifier) @code_function.name)
  ) @code_function.decl
)

; (4) namespace bodies and braced `extern "C" { ... }` blocks
(declaration_list
  (declaration
    declarator: (function_declarator
      declarator: (identifier) @code_function.name)
  ) @code_function.decl
)

; (5) template prototypes: `template <typename T> T gamma(T);`
(template_declaration
  (declaration
    declarator: (function_declarator
      declarator: (identifier) @code_function.name)
  ) @code_function.decl
)

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
