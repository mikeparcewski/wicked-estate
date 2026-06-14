; wicked_estate RON (Rusty Object Notation) extraction queries — @code_* convention.
;
; Verified against arborium-ron-2.18.0 node-types.json.
; Named nodes used:
;   struct      — children: struct_name, tuple, unit_struct; field body: struct_entry
;   struct_name — children: identifier
;   enum_variant — children: identifier

; ── Named struct definitions ─────────────────────────────────────────────────
(struct
  (struct_name
    (identifier) @code_struct.name)
) @code_struct.def

; ── Enum variant definitions ─────────────────────────────────────────────────
(enum_variant
  (identifier) @code_enum_member.name
) @code_enum_member.def
