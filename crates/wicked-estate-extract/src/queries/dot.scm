; wicked_estate DOT/Graphviz extraction queries — @code_* convention.
;
; Verified against arborium-dot-2.18.0 node-types.json.
; Named nodes used:
;   source_file — field id: id (graph name), field block: block
;   subgraph    — field id: id (subgraph name), field block: block
;   node_stmt   — children: node_id, attr_list
;   node_id     — children: id, port
;   id          — children: identifier, string_literal, number_literal

; ── Graph (top-level named graph) ────────────────────────────────────────────
(source_file
  id: (id (identifier) @code_module.name)
) @code_module.def

; ── Subgraphs ─────────────────────────────────────────────────────────────────
(subgraph
  id: (id (identifier) @code_module.name)
) @code_module.def

; ── Node statements ───────────────────────────────────────────────────────────
(node_stmt
  (node_id (id (identifier) @code_variable.name))
) @code_variable.def
