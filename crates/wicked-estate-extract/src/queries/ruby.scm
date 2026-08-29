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

; `class << self` — a NON-EMITTING containment anchor named "self" (scm-anchors
; D5, scheme 3): its members nest as `C#self#m().`, converging with `def self.m`
; (owner splice below) on ONE id shape, distinct from the instance `C#m().`.
; No node is minted — no principled node exists for the singleton class itself.
(singleton_class
  value: (self) @code_class.name
  body: (_) @code_class.body
) @code_class.anchor

; Module definitions
(module
  name: (constant) @code_module.name
  body: (_) @code_module.body
) @code_module.def

; Method definitions — name alternation covers plain methods, setters
; (`def name=`), and operators (`def []`, `def <=>`, `def ==`, `def <<`) (D04-4)
(method
  name: [(identifier) (setter) (operator)] @code_method.name
  parameters: (method_parameters)? @code_method.params
  body: (_)? @code_method.body
) @code_method.def

; Singleton method definitions (class methods). `def self.m` splices "self" as
; the owner, minting `C#self#m().` — the SAME shape `class << self` members get
; via the anchor above, so both spellings of a Ruby class-method converge on one
; id, distinct from the instance `C#m().` (scm-anchors D5). R-DEF-LOSS: the
; object constraint is OPTIONAL (`?`) — singleton_method.object admits the OPEN
; _arg expression set (tree-sitter-ruby 0.23.1), so a non-self alternation can
; never be exhaustive; `def Foo.m` / `def obj.m` keep their defs OWNERLESS
; (nested under the enclosing class by containment, still merging with the
; instance method — the fixture-pinned residual with its own flip instruction).
(singleton_method
  object: (self)? @code_method.owner
  name: [(identifier) (setter) (operator)] @code_method.name
  parameters: (method_parameters)? @code_method.params
  body: (_)? @code_method.body
) @code_method.def

; `alias new_name old_name` — the NEW name (first operand, field name:) becomes a
; Method definition (D04-4)
(alias
  name: (_method_name) @code_method.name
) @code_method.def

; `alias_method :new_name, :old_name` — capture ONLY the first symbol (the new
; name), anchored with `.`. Capturing every symbol would also emit a def for the
; OLD name with the same SymbolId AND same kind as the real method, and the store
; upsert would flap the real method's location (invisible to the kind-conflict
; guard). The `.name.symbol` suffix opts in to the leading-`:` strip
; (strip_leading_symbol_colon) — the plain `.name` channel keeps colons verbatim
; because CSS/YAML def names legitimately start with `:` (EG-COR-1).
(call
  method: (identifier) @_alias_kw
  arguments: (argument_list
    .
    (simple_symbol) @code_method.name.symbol)
  (#eq? @_alias_kw "alias_method")
) @code_method.def

; attr_reader / attr_writer / attr_accessor — every symbol argument defines a
; method (`attr_accessor :a, :b` emits one Method per symbol) (D04-4)
(call
  method: (identifier) @_attr_kw
  arguments: (argument_list
    (simple_symbol) @code_method.name.symbol)
  (#any-of? @_attr_kw "attr_reader" "attr_writer" "attr_accessor")
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
