; wicked_estate Perl extraction queries — @code_* convention.
;
; Verified against arborium-perl-2.18.0 node-types.json.
; Named nodes used:
;   subroutine_declaration_statement — field name: bareword
;   package_statement                — field name: package
;   use_statement                    — field module: package

; ── Subroutine declarations (sub foo { ... }) ─────────────────────────────────
(subroutine_declaration_statement
  name: (_) @code_function.name
) @code_function.def

; ── Package declarations ──────────────────────────────────────────────────────
(package_statement
  name: (_) @code_module.name
) @code_module.def

; ── Use statements (imports) ─────────────────────────────────────────────────
(use_statement
  module: (_) @import.source
) @import
