; wicked_estate GraphQL extraction queries — @code_* convention.
;
; Verified against arborium-graphql-2.18.0 node-types.json.
; GraphQL has no functions — the meaningful units are type/interface/enum/input definitions.
; name nodes are named children of the definition nodes.
;
; Named nodes used:
;   object_type_definition    — children: name
;   interface_type_definition — children: name
;   enum_type_definition      — children: name
;   input_object_type_definition — children: name
;   union_type_definition     — children: name
;   scalar_type_definition    — children: name

; ── Object type definitions ───────────────────────────────────────────────────
(object_type_definition
  (name) @code_struct.name
) @code_struct.def

; ── Interface type definitions ────────────────────────────────────────────────
(interface_type_definition
  (name) @code_interface.name
) @code_interface.def

; ── Enum type definitions ─────────────────────────────────────────────────────
(enum_type_definition
  (name) @code_enum.name
) @code_enum.def

; ── Input object type definitions ────────────────────────────────────────────
(input_object_type_definition
  (name) @code_struct.name
) @code_struct.def

; ── Union type definitions ────────────────────────────────────────────────────
(union_type_definition
  (name) @code_type.name
) @code_type.def
