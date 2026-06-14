; wicked_estate Pascal extraction queries.
; Node-types verified against tree-sitter-pascal 0.10.2.
;
; Grammar structure:
;   root        — top-level container (program | unit | library)
;   program     — has moduleName child
;   unit        — has moduleName child
;   defProc     — concrete procedure/function implementation: field header: (declProc)
;   declProc    — procedure/function declaration: field name: (identifier | genericDot | genericTpl | operatorName)
;   declType    — type declaration: field name: (identifier), field type: (_)
;   declClass / declIntf — class/interface declarations (no name field; name is in context)
;   exprCall    — function/method call: field entity: (_), field args: (_)
;
; Capture strategy:
;   - defProc wraps the header (declProc) which has the name field — capture at defProc level.
;   - Forward declarations (declProc without a defProc wrapper) are also captured.
;   - declType captures named type aliases, classes, records, enums.

; ── Concrete procedure/function implementations ───────────────────────────────
; Captures the defProc (full implementation node). The name comes from the
; header's declProc name field.
(defProc
  header: (declProc
    name: (_) @code_function.name)
) @code_function.def

; ── Forward/external declarations (declProc without a wrapping defProc) ───────
; These appear directly under program/unit/interface sections. The grammar
; aliases declProcFwd → declProc, so forward declarations also parse as declProc.
; Note: this pattern also fires on the inner declProc of a defProc, producing a
; second (smaller) node for the same logical symbol. That is acceptable — the
; two nodes have different byte spans and represent different syntactic roles
; (declaration vs implementation). The extractor treats them as distinct nodes.
(declProc
  name: (_) @code_function.name
) @code_function.def

; ── Type declarations (type aliases, records, classes, enums) ─────────────────
(declType
  name: (identifier) @code_type.name
) @code_type.def

; ── Program / unit name ───────────────────────────────────────────────────────
(program
  (moduleName) @code_module.name
) @code_module.def

(unit
  (moduleName) @code_module.name
) @code_module.def

; ── Function/procedure calls ──────────────────────────────────────────────────
(exprCall
  entity: (_) @call.function
) @call
