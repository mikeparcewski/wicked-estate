; wicked_estate CUDA extraction — tree-sitter-cuda grammar (a C++ grammar fork with
; CUDA extensions), @code_* convention.
; Node types + fields verified against tree-sitter-cuda-0.21.1 node-types.json /
; grammar.js. The C++ surface mirrors cpp.scm exactly; the only CUDA-specific addition
; is the kernel-launch call form `f<<<grid, block>>>(args)` (kernel_call_syntax).

; ── Aggregates / types ───────────────────────────────────────────────────────
(class_specifier
  name: (type_identifier) @code_class.name) @code_class.def

(struct_specifier
  name: (type_identifier) @code_struct.name) @code_struct.def

(enum_specifier
  name: (type_identifier) @code_enum.name) @code_enum.def

(namespace_definition
  name: (namespace_identifier) @code_namespace.name) @code_namespace.def

; ── Functions ────────────────────────────────────────────────────────────────
; Top-level / __global__ / __device__ functions (identifier declarator).
(function_definition
  declarator: (function_declarator
    declarator: (identifier) @code_function.name)) @code_function.def

; Methods inside a class/struct (field_identifier declarator).
(function_definition
  declarator: (function_declarator
    declarator: (field_identifier) @code_method.name)) @code_method.def

; ── Macros ───────────────────────────────────────────────────────────────────
(preproc_def
  name: (identifier) @code_constant.name) @code_constant.def

(preproc_function_def
  name: (identifier) @code_function.name) @code_function.def

; ── Type aliases ─────────────────────────────────────────────────────────────
(type_definition
  declarator: (type_identifier) @code_type.name) @code_type.def

(alias_declaration
  name: (type_identifier) @code_type.name) @code_type.def

; ── Includes ─────────────────────────────────────────────────────────────────
(preproc_include
  path: (system_lib_string) @import.source) @import

(preproc_include
  path: (string_literal) @import.source) @import

; ── Calls ────────────────────────────────────────────────────────────────────
; Simple call: foo(args)
(call_expression
  function: (identifier) @call.function) @call

; Method call: obj.method(args) / ptr->method(args)
(call_expression
  function: (field_expression
    field: (field_identifier) @call.method)) @call.method

; CUDA kernel launch: my_kernel<<<grid, block>>>(args)
; In this grammar the launch parses as a normal call_expression whose `function:`
; is the bare kernel-name identifier, with kernel_call_syntax as a sibling holding
; the <<<grid,block>>> config — so the simple-call rule above already captures the
; kernel name. No dedicated rule is needed (verified against the real parse tree).
