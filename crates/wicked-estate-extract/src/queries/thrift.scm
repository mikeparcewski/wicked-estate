; Thrift IDL extraction — arborium-thrift grammar (vendors duskmoon314/tree-sitter-thrift),
; @code_* convention. Node types + fields verified against the grammar's node-types.json:
;   struct_definition / service_definition / enum_definition / union_definition each carry the
;   name in a field literally named `type` (an `identifier`).
;   function_definition / exception_definition / const_definition carry the name as a direct
;   `identifier` child (no field); the other identifiers are nested under parameters/type/throws.
;   typedef_definition names via a `typedef_identifier` child.
;   include_statement → a `string` child (the included file path).
; Thrift has no call expressions (it is an IDL) — symbols + includes only.

; Structs → struct type units.
(struct_definition
  type: (identifier) @code_struct.name) @code_struct.def

; Services → interfaces (a service is a set of RPC method declarations).
(service_definition
  type: (identifier) @code_interface.name) @code_interface.def

; Enums.
(enum_definition
  type: (identifier) @code_enum.name) @code_enum.def

; Unions → model as structs (tagged record).
(union_definition
  type: (identifier) @code_struct.name) @code_struct.def

; Exceptions → model as structs (they are field-bearing records). Name is the direct identifier
; child; field/annotation children are distinct node types so this binds the exception name.
(exception_definition
  (identifier) @code_struct.name) @code_struct.def

; Service RPC methods → functions. The direct `identifier` child is the method name; parameters,
; return type and throws live under their own node types, so this captures only the name.
(function_definition
  (identifier) @code_function.name) @code_function.def

; typedefs → type aliases.
(typedef_definition
  (typedef_identifier) @code_type.name) @code_type.def

; Constants → constants. The direct identifier child is the const name (the value is a `literal`,
; the declared type is a `type` node).
(const_definition
  (identifier) @code_constant.name) @code_constant.def

; include "shared.thrift" → cross-file dependency. The string child is the included path;
; strip_literal_quotes canonicalises it.
(include_statement
  (string) @import.source) @import
