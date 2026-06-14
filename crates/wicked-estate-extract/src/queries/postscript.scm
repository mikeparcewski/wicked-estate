; wicked_estate PostScript extraction queries — @code_* convention.
;
; Verified against arborium-postscript-2.18.0 node-types.json.
; PostScript primary named constructs: procedure (code block), operator (name token).
; Named nodes used:
;   procedure — a code block { ... }
;   operator  — a PostScript name/operator leaf
;
; In PostScript, a named procedure is defined as:
;   /myProc { ... } def
; The operator child within a procedure is used as the name.

; ── Procedure blocks named by their operator child ───────────────────────────
(procedure
  (operator) @code_function.name
) @code_function.def
