; VHDL extraction — arborium-vhdl 2.18.0 grammar (alemuller-derived fork),
; @code_* convention. Node types + fields verified against THIS crate's node-types.json
; (/arborium-vhdl-2.18.0/grammar/src/node-types.json), NOT upstream GitHub:
;   entity_declaration.name / architecture_body.name / package_declaration.name /
;   component_declaration.name  — field `name` : (identifier | extended_identifier).
;   function_body.designator / procedure_body.designator
;     — the subprogram-body name lives on field `designator`, NOT `name`
;       (types: identifier | extended_identifier | operator_symbol). function_body/procedure_body
;       have NO `name` field. The earlier `name: (_)` on function_body was a Structure error.
; `name`/`designator` accept several identifier node kinds, so the capture uses (_) to bind any.
; VHDL is hardware description, not call-graph code — symbols only (matches languages.toml caps).

; Entities → the design-unit interface; model as a module.
(entity_declaration
  name: (_) @code_module.name) @code_module.def

; Architectures → the implementation body of an entity; model as a module.
(architecture_body
  name: (_) @code_module.name) @code_module.def

; Packages → reusable declaration units; model as modules.
(package_declaration
  name: (_) @code_module.name) @code_module.def

; Components → reusable instantiable units; model as interfaces (a component is a port contract).
(component_declaration
  name: (_) @code_interface.name) @code_interface.def

; Functions (subprogram bodies) — name is on the `designator` field, not `name`.
(function_body
  designator: (_) @code_function.name) @code_function.def

; Procedures → model as functions (callable subprograms) — name is on `designator`.
(procedure_body
  designator: (_) @code_function.name) @code_function.def
