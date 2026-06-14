; wicked_estate TOML extraction queries — @code_* convention.
;
; Verified against tree-sitter-toml-ng-0.7.0 node-types.json.
; TOML has no functions or classes.  We treat tables and top-level key-value
; pairs as "module" and "variable" definitions respectively.
;
; Named nodes used:
;   table              — children: bare_key | quoted_key | dotted_key | pair
;   table_array_element — same child types as table (the [[array]] form)
;   pair               — children include bare_key | quoted_key | dotted_key (the key)
;   bare_key           — anonymous terminal (the key name text)
;   quoted_key         — quoted string key

; ── Table headers: [section] and [[array]] ────────────────────────────────────
; The bare_key or quoted_key immediately under table is the table name.
(table
  (bare_key) @code_module.name
) @code_module.def

(table
  (quoted_key) @code_module.name
) @code_module.def

(table_array_element
  (bare_key) @code_module.name
) @code_module.def

(table_array_element
  (quoted_key) @code_module.name
) @code_module.def

; ── Key = value pairs (inside tables or at top level) ───────────────────────
; pair children: bare_key | dotted_key | quoted_key (key) + value types.
; bare_key and quoted_key are only produced for keys, not values.
(pair
  (bare_key) @code_variable.name
) @code_variable.def

(pair
  (quoted_key) @code_variable.name
) @code_variable.def
