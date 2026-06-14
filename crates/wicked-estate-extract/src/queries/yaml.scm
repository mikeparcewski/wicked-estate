; wicked_estate YAML extraction queries — @code_* convention.
; YAML top-level block mapping keys become Struct nodes.

(stream
  (document
    (block_node
      (block_mapping
        (block_mapping_pair
          key: (flow_node (plain_scalar) @code_struct.name)
        ) @code_struct.def
      )
    )
  )
)
