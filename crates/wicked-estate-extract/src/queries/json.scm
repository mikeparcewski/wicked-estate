; wicked_estate JSON extraction queries — @code_* convention.
; JSON has no functions or calls. Top-level object keys become Struct nodes.

(document
  (object
    (pair
      key: (string (string_content) @code_struct.name)
    ) @code_struct.def
  )
)
