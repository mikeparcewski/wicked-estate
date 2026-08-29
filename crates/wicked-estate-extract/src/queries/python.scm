; wicked_estate Python extraction queries — @code_* convention.

; Class definitions
(class_definition
  name: (identifier) @code_class.name
  body: (block) @code_class.body
) @code_class.def

; Class extends — one capture per parent (multi-inheritance supported)
(class_definition
  name: (identifier) @code_class.name
  superclasses: (argument_list (identifier) @code_extends.target)
) @code_extends.def

; NOTE (§11): there is deliberately NO class-body Method pattern here. The general
; function pattern below already matches methods; a second class-scoped pattern
; emitted every method twice on the same SymbolId with a different kind
; (Method + Function), and the store's last-write-wins upsert silently re-kinded
; them. Restoring Method kind belongs with enclosing-type identity (method-identity
; lane), not with a duplicate pattern.

; Function definitions (top-level, nested, and inside classes)
(function_definition
  name: (identifier) @code_function.name
  body: (block) @code_function.body
) @code_function.def

; Module-level UPPER_CASE constants
(module
  (expression_statement
    (assignment
      left: (identifier) @code_constant.name))
  (#match? @code_constant.name "^[A-Z][A-Z0-9_]*$")
) @code_constant.def

; ── ORM / framework-aware extraction (W6.2) ──────────────────────────────────
;
; SQLAlchemy (1.x + 2.0 DeclarativeBase style)
; -----------------------------------------
; Class-level Column(...) assignment → NodeKind::Field
; Matches both:
;   id = Column(Integer, primary_key=True)           (unannotated)
;   id: int = mapped_column(Integer, primary_key=True)  (annotated, tree-sitter also uses `assignment`)
;
; Gate: the RHS call function name must be one of the recognised SQLAlchemy constructors.
; This avoids capturing every class-level assignment — only ORM-shaped RHS.

(class_definition
  body: (block
    (expression_statement
      (assignment
        left: (identifier) @code_field.name
        right: (call
          function: (identifier) @_sa_func)
      )
    )
  )
  (#any-of? @_sa_func "Column" "mapped_column" "relationship" "Mapped" "deferred" "synonym")
) @code_field.def

; ── Django ORM ────────────────────────────────────────────────────────────────
; Class-level models.XField(...) assignment → NodeKind::Field
; Gate: the RHS call must be an attribute access on an identifier (the `models` alias is common,
; but users also do `from django.db import models as m`).  We gate on the attribute name ending
; in "Field" or being one of the well-known non-suffixed constructors.
;
; Matches:
;   title    = models.CharField(max_length=200)
;   author   = models.ForeignKey("auth.User", on_delete=models.CASCADE)
;   pub_date = models.DateTimeField(auto_now_add=True)

(class_definition
  body: (block
    (expression_statement
      (assignment
        left: (identifier) @code_field.name
        right: (call
          function: (attribute
            attribute: (identifier) @_dj_field_type)
        )
      )
    )
  )
  (#match? @_dj_field_type "^(CharField|TextField|IntegerField|FloatField|DecimalField|BooleanField|NullBooleanField|DateField|DateTimeField|TimeField|DurationField|FileField|ImageField|URLField|EmailField|SlugField|UUIDField|GenericIPAddressField|IPAddressField|BinaryField|ForeignKey|OneToOneField|ManyToManyField|AutoField|BigAutoField|SmallAutoField|BigIntegerField|SmallIntegerField|PositiveIntegerField|PositiveSmallIntegerField|JSONField)$")
) @code_field.def

; Import statements
(import_statement
  name: (dotted_name) @import.source
) @import

(import_from_statement
  module_name: (dotted_name) @import.source
) @import

; Function calls — simple
(call
  function: (identifier) @call.function
) @call

; Method calls — attribute calls
(call
  function: (attribute
    attribute: (identifier) @call.method
  )
) @call.method
