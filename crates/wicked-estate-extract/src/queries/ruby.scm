; wicked_estate Ruby extraction queries — @code_* convention.

; Class definitions
(class
  name: (constant) @code_class.name
  body: (_) @code_class.body
) @code_class.def

; Singleton class definitions
(singleton_class
  value: (constant) @code_class.name
  body: (_) @code_class.body
) @code_class.def

; Module definitions
(module
  name: (constant) @code_module.name
  body: (_) @code_module.body
) @code_module.def

; Method definitions
(method
  name: (identifier) @code_method.name
  parameters: (method_parameters)? @code_method.params
  body: (_)? @code_method.body
) @code_method.def

; Singleton method definitions (class methods)
(singleton_method
  name: (identifier) @code_method.name
  parameters: (method_parameters)? @code_method.params
  body: (_)? @code_method.body
) @code_method.def

; Class-level constants (UPPER_CASE or CamelCase — Ruby constant node)
(assignment
  left: (constant) @code_constant.name
) @code_constant.def

; Heritage: class Foo < Bar → extends
(class
  name: (constant) @code_class.name
  superclass: (superclass
    (constant) @code_extends.target)
) @code_extends.def

; Heritage: include Mod inside a class → implements (Ruby mixin)
(class
  name: (constant) @code_class.name
  body: (body_statement
    (call
      method: (identifier) @_include_kw
      arguments: (argument_list
        (constant) @code_implements.target)))
  (#match? @_include_kw "^include$")
) @code_implements.def

; Require / require_relative imports
(call
  method: (identifier) @_req_kw
  arguments: (argument_list
    (string) @import.source
  )
  (#match? @_req_kw "^require(_relative)?$")
) @import

; Method calls
(call
  receiver: (_)? @call.object
  method: (identifier) @call.method
  arguments: (argument_list)? @call.args
) @call
