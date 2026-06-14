; wicked_estate Elm extraction queries — @code_* convention.
;
; Verified against arborium-elm-2.18.0 node-types.json.
; Named nodes used:
;   value_declaration          — field functionDeclarationLeft: function_declaration_left
;   function_declaration_left  — children: lower_case_identifier (the fn name)
;   type_declaration           — field name: upper_case_identifier
;   type_alias_declaration     — field name: upper_case_identifier
;   import_clause              — field moduleName: upper_case_qid

; ── Value / function declarations ────────────────────────────────────────────
(value_declaration
  functionDeclarationLeft: (function_declaration_left
    (lower_case_identifier) @code_function.name)
) @code_function.def

; ── Type declarations (union types) ──────────────────────────────────────────
(type_declaration
  name: (upper_case_identifier) @code_type.name
) @code_type.def

; ── Type alias declarations ───────────────────────────────────────────────────
(type_alias_declaration
  name: (upper_case_identifier) @code_type.name
) @code_type.def

; ── Import clauses ────────────────────────────────────────────────────────────
(import_clause
  moduleName: (_) @import.source
) @import
