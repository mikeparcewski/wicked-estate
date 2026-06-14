; wicked_estate SQL extraction queries — @code_* convention.
;
; Verified against arborium-sql-2.18.0 node-types.json.
; SQL has no functions in the traditional sense — meaningful units are
; CREATE TABLE/VIEW/FUNCTION/PROCEDURE/TYPE statements.
;
; Named nodes used:
;   create_table     — children: keyword_create, object_reference (table name), column_definitions
;   create_view      — children: keyword_create, identifier (view name)
;   create_function  — field custom_type: object_reference (function name)
;   create_type      — field name: identifier (type name)
;   object_reference — field name: identifier

; ── CREATE TABLE ──────────────────────────────────────────────────────────────
(create_table
  (object_reference
    name: (identifier) @code_struct.name)
) @code_struct.def

; ── CREATE VIEW ───────────────────────────────────────────────────────────────
(create_view
  (identifier) @code_struct.name
) @code_struct.def

; ── CREATE FUNCTION ───────────────────────────────────────────────────────────
(create_function
  custom_type: (object_reference
    name: (identifier) @code_function.name)
) @code_function.def

; ── CREATE TYPE ───────────────────────────────────────────────────────────────
(create_type
  name: (identifier) @code_type.name
) @code_type.def
