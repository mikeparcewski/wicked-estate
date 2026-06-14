; wicked_estate KDL extraction queries — @code_* convention.
;
; Verified against arborium-kdl-2.18.0 node-types.json.
; KDL is a document language; primary construct is a "node".
; Named nodes used:
;   node        — field children: node_children; children: identifier, prop, node_field
;   identifier  — leaf or string child (node name)

; ── Node definitions ─────────────────────────────────────────────────────────
; A KDL node with an identifier as its first (name) child.
(node
  (identifier) @code_variable.name
) @code_variable.def
