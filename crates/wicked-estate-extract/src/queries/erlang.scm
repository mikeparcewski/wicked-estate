; wicked_estate Erlang extraction queries — @code_* convention.
;
; Verified against arborium-erlang-2.18.0 node-types.json.
; Named nodes used:
;   function_clause — field name: _name (includes atom)
;   call            — field expr: _expr

; ── Function clauses (each clause is named by the function name atom) ─────────
; Erlang functions consist of one or more clauses; capture each clause head.
(function_clause
  name: (_) @code_function.name
) @code_function.def

; ── Function calls ───────────────────────────────────────────────────────────
(call
  expr: (_) @call.function
) @call
