; wicked_estate Nginx config extraction queries — @code_* convention.
;
; Verified against arborium-nginx-2.18.0 node-types.json.
; Named nodes used:
;   block_directive  — field name: directive
;   simple_directive — field name: directive
;   directive        — leaf node (named: true)

; ── Block directives (server, location, http, upstream, …) ───────────────────
(block_directive
  name: (directive) @code_module.name
) @code_module.def

; ── Simple directives (listen, root, proxy_pass, …) ─────────────────────────
(simple_directive
  name: (directive) @code_variable.name
) @code_variable.def
