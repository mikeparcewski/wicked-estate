; Solidity extraction — arborium-solidity 2.18.0 grammar (vendors a JoranHonig-derived fork),
; @code_* convention. Node types + fields verified against THIS crate's node-types.json
; (/arborium-solidity-2.18.0/grammar/src/node-types.json), NOT upstream GitHub:
;   contract_declaration.name / interface_declaration.name / library_declaration.name → (identifier)
;   struct_declaration.name / enum_declaration.name → (identifier)
;   function_definition.{name:(identifier), body, return_type}
;   modifier_definition.{name:(identifier), body}
;   event_definition.name / state_variable_declaration.name → (identifier)
;   call_expression.function : (expression)   ← a VISIBLE wrapper rule, not (identifier).
;     A bare callee is (expression (identifier)); a member call is
;     (expression (member_expression property: (identifier))). member_expression.property:(identifier).
;   import_directive.source : (string).
; The earlier `function: (identifier)` was a Structure error because the `function` field holds an
; `expression` node (this fork makes `expression` a visible single-child wrapper), never a bare
; `identifier`. constructor_definition has no `name` field, so it is not surfaced — the contract
; node already anchors the type.

; Contracts → the primary type unit.
(contract_declaration
  name: (identifier) @code_class.name) @code_class.def

; Interfaces.
(interface_declaration
  name: (identifier) @code_interface.name) @code_interface.def

; Libraries (reusable code units) — model as a module.
(library_declaration
  name: (identifier) @code_module.name) @code_module.def

; Structs.
(struct_declaration
  name: (identifier) @code_struct.name) @code_struct.def

; Enums.
(enum_declaration
  name: (identifier) @code_enum.name) @code_enum.def

; Functions (free functions and contract methods both use function_definition).
(function_definition
  name: (identifier) @code_function.name) @code_function.def

; Modifiers — Solidity-specific guards; model as functions (they are callable units).
(modifier_definition
  name: (identifier) @code_function.name) @code_function.def

; Events — surfaced as fields (named declarations on the contract).
(event_definition
  name: (identifier) @code_field.name) @code_field.def

; State variables → fields.
(state_variable_declaration
  name: (identifier) @code_field.name) @code_field.def

; Direct calls — foo(...). The callee is wrapped: function: (expression (identifier)).
(call_expression
  function: (expression (identifier) @call.function)) @call

; Member calls — obj.method(...) : the callee expression wraps a member_expression whose
; property (an identifier) is the called name.
(call_expression
  function: (expression
    (member_expression
      property: (identifier) @call.method))) @call

; Imports — `import "./Foo.sol";` / `import {A} from "./Foo.sol";`. The source is a string
; literal; strip_literal_quotes canonicalises the path.
(import_directive
  source: (string) @import.source) @import
