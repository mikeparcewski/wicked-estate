; wicked_estate Idris 2 extraction queries — @code_* convention.
;
; Verified against arborium-idris-2.18.0 node-types.json.
; Named nodes used:
;   function      — children: lhs, rhs, where, with
;   data          — field name: data_name
;   interface     — child interface_head (field name: interface_name)
;   data_name     — leaf (name of a data type)

; ── Data type definitions ─────────────────────────────────────────────────────
(data
  name: (data_name) @code_struct.name
) @code_struct.def

; ── Interface definitions ─────────────────────────────────────────────────────
(interface
  (interface_head
    name: (_) @code_interface.name)
) @code_interface.def

; ── Function definitions (function node in declarations) ─────────────────────
; function → lhs (contains the function name as first child of lhs)
; lhs has no fields; capture the whole function and use wildcard name from lhs.
(function
  (lhs (_) @code_function.name)
) @code_function.def
