; wicked_estate Dockerfile extraction queries — @code_* convention.
;
; Verified against arborium-dockerfile-2.18.0 node-types.json.
; Dockerfiles have no functions/classes — the meaningful unit is an instruction.
; We capture FROM instructions (the base image) as struct definitions because
; they define the build stage.
;
; Named nodes used:
;   from_instruction — children: image_spec, param; field as: image_alias
;   image_spec       — field name: image_name
;   image_name       — the image name text

; ── FROM instructions (build stages) ─────────────────────────────────────────
; Capture the image name as the "definition" of the build stage.
(from_instruction
  (image_spec
    name: (image_name) @code_struct.name)
) @code_struct.def

; ── ARG instructions (build-time variables) ───────────────────────────────────
(arg_instruction
  name: (_) @code_variable.name
) @code_variable.def
