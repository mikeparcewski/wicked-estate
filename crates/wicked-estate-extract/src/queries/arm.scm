; wicked_estate ARM Template extraction queries — W13.4 IaC.
;
; ARM templates are JSON files. No dedicated grammar — reuses tree-sitter-json.
; Node-types verified against tree-sitter-json-0.24.8.
;
; ARM resources have the shape:
;   { "type": "Microsoft.Storage/storageAccounts", "name": "...", "dependsOn": [...] }
;
; Strategy:
;   - Capture "type" key-value pairs whose value matches the Azure provider pattern
;     (contains a "/" — heuristic for "Microsoft.Foo/bars") → struct nodes.
;   - Capture "name" key-value pairs to pair with the nearest resource type.
;   - Capture string values inside "dependsOn" arrays → import edges.
;   - The resolver can post-process ARM function calls (e.g. "[resourceId(...)]") later.
;
; Named nodes used (tree-sitter-json):
;   document     — root
;   object       — { key: value, ... }
;   pair         — key: string, value: _value
;   string       — "content"
;   string_content — inner text of a string (no quotes)
;   array        — [ value, ... ]

; ── Resource "type" fields → struct nodes ────────────────────────────────────
; Capture the resource type string as the struct name.
; All Azure resource types contain "/" (e.g. "Microsoft.Storage/storageAccounts").
; We capture every "type" pair; the resolver can filter by naming pattern.
(pair
  key: (string (string_content) @_key (#eq? @_key "type"))
  value: (string (string_content) @code_struct.name)
) @code_struct.def

; ── "dependsOn" array items → import edges ───────────────────────────────────
; dependsOn values are strings (resource IDs or ARM function calls).
(pair
  key: (string (string_content) @_key (#eq? @_key "dependsOn"))
  value: (array
    (string (string_content) @import.source))
) @import

; ── Top-level "resources" array → capture resource name fields ───────────────
; ARM "name" fields inside resource objects become variable defs.
; This is a heuristic — any "name" pair is captured; the extractor surfaces them.
(pair
  key: (string (string_content) @_key (#eq? @_key "name"))
  value: (string (string_content) @code_variable.name)
) @code_variable.def
