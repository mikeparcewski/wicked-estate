; wicked_estate Ada extraction queries — @code_* convention.
;
; Verified against arborium-ada-2.18.0 node-types.json.
; Named nodes used:
;   subprogram_body     — procedure/function body; name field -> identifier
;   package_declaration — package spec; name field -> identifier

; ── Subprogram (procedure/function) bodies ───────────────────────────────────
; subprogram_body children include procedure_specification or function_specification
; which carry a `name` field.  We capture the whole body as def anchor and
; the first identifier child of the specification as the name.
(subprogram_body
  [
    (procedure_specification
      name: (_) @code_function.name)
    (function_specification
      name: (_) @code_function.name)
  ]
) @code_function.def

; ── Package declarations ──────────────────────────────────────────────────────
(package_declaration
  name: (_) @code_module.name
) @code_module.def

; ── Package bodies ────────────────────────────────────────────────────────────
(package_body
  name: (_) @code_module.name
) @code_module.def
