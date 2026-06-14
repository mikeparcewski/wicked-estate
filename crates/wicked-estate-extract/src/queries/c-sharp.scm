; wicked_estate C# extraction queries (tags convention: @definition.* / @reference.* + @name).
; Symbols + calls + imports.

(method_declaration name: (identifier) @name) @definition.method
(class_declaration name: (identifier) @name) @definition.class
(interface_declaration name: (identifier) @name) @definition.interface
(enum_declaration name: (identifier) @name) @definition.enum

(invocation_expression
  function: (member_access_expression name: (identifier) @name)) @reference.call
(invocation_expression
  function: (identifier) @name) @reference.call

(using_directive name: (_) @name) @reference.import
