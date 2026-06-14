; wicked_estate Protocol Buffers extraction queries — @code_* convention.
;
; Verified against arborium-proto-2.18.0 node-types.json.
; Named nodes used:
;   message      — children: message_name, message_body
;   service      — children: service_name, rpc, ...
;   rpc          — children: rpc_name, message_or_enum_type, ...
;   enum         — children: enum_name, enum_body
;   message_name — children: identifier
;   service_name — children: identifier
;   rpc_name     — children: identifier
;   enum_name    — children: identifier
;   import       — field path: string_lit

; ── Message definitions ───────────────────────────────────────────────────────
(message
  (message_name
    (identifier) @code_struct.name)
) @code_struct.def

; ── Service definitions ───────────────────────────────────────────────────────
(service
  (service_name
    (identifier) @code_interface.name)
) @code_interface.def

; ── RPC definitions ──────────────────────────────────────────────────────────
(rpc
  (rpc_name
    (identifier) @code_function.name)
) @code_function.def

; ── Enum definitions ─────────────────────────────────────────────────────────
(enum
  (enum_name
    (identifier) @code_enum.name)
) @code_enum.def

; ── Import statements ────────────────────────────────────────────────────────
(import
  path: (_) @import.source
) @import
