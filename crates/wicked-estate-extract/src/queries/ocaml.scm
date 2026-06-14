; wicked_estate OCaml extraction — @code_* convention.
; Field/type pairs verified against tree-sitter-ocaml 0.23.2 node-types.json:
;   let_binding.pattern accepts `value_name` (NOT value_pattern — that is not a _binding_pattern);
;   module_binding.name=module_name; module_type_definition.name=module_type_name;
;   type_binding.name=type_constructor; method_definition.name=method_name;
;   class_binding.name=class_name; application_expression.function=_simple_expression (wildcard).

; let f ... = ...   (value + function bindings)
(value_definition
  (let_binding
    pattern: (value_name) @code_function.name)) @code_function.def

; module M = ...
(module_definition
  (module_binding
    name: (module_name) @code_module.name)) @code_module.def

; module type S = ...  → interface
(module_type_definition
  name: (module_type_name) @code_interface.name) @code_interface.def

; type t = ...
(type_definition
  (type_binding
    name: (type_constructor) @code_type.name)) @code_type.def

; method m = ...
(method_definition
  name: (method_name) @code_method.name) @code_method.def

; class c = ...
(class_definition
  (class_binding
    name: (class_name) @code_class.name)) @code_class.def

; f x   (function application)
(application_expression
  function: (_) @call.function) @call

; obj#m   (method invocation)
(method_invocation
  (method_name) @call.function) @call

; open M / include M → import
(open_module) @import
