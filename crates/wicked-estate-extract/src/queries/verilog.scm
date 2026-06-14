; Verilog / SystemVerilog extraction — arborium-verilog 2.18.0 grammar (full IEEE 1800),
; @code_* convention. Node types verified against THIS crate's node-types.json
; (/arborium-verilog-2.18.0/grammar/src/node-types.json), NOT upstream GitHub.
;
; Names are NOT on a `name` field — they are reached through identifier-wrapper children:
;   module_declaration → module_header → (simple_identifier)         ← the module name
;     (`module_identifier` does NOT exist in this fork — it is the hidden rule `_module_identifier`
;      which inlines to simple_identifier/escaped_identifier directly under module_header. The
;      ANSI/non-ANSI header nodes do NOT carry the name.)
;   function_declaration → function_body_declaration → function_identifier → (simple_identifier)
;   task_declaration     → task_body_declaration     → task_identifier     → (simple_identifier)
; tree-sitter binds cross-depth captures in one pattern. No reliable call/import surface (calls
; are deeply-nested function_subroutine_call chains) — symbols only, matching languages.toml caps.
;
; The earlier rule used `module_identifier`, a NodeType error (no such node). The module name is a
; simple_identifier child of module_header; both header forms route through it, so one rule covers
; ANSI and non-ANSI modules.

; Modules → the primary design unit (model as a module). The name is the simple_identifier child
; of module_header (covers both ANSI and non-ANSI module forms).
(module_declaration
  (module_header
    (simple_identifier) @code_module.name)) @code_module.def

; Functions.
(function_declaration
  (function_body_declaration
    (function_identifier
      (simple_identifier) @code_function.name))) @code_function.def

; Tasks → procedures; model as functions (callable units).
(task_declaration
  (task_body_declaration
    (task_identifier
      (simple_identifier) @code_function.name))) @code_function.def
