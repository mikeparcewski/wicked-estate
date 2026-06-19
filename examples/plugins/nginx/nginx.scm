; nginx — wicked_estate @code_* extraction query (EXAMPLE plugin).
; Captures each nginx block (http / server / location / upstream / events / …) as a module-like
; symbol. A richer query could also surface directives or upstream/location arguments — this is
; intentionally minimal to demonstrate the plugin mechanism.

(block
  name: (identifier) @code_module.name) @code_module.def
