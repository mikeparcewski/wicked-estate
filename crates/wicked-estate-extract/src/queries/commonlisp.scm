; wicked_estate Common Lisp extraction queries — @code_* convention.
;
; Verified against arborium-commonlisp-2.18.0 node-types.json.
; Named nodes used:
;   defun        — has child defun_header which has field function_name
;   defun_header — field function_name: sym_lit/kwd_lit etc.
;
; The `defun` node is a top-level recognized form.
; defun_header.function_name points to the name.

; ── Function/method definitions (defun, defmethod, defgeneric) ───────────────
(defun
  (defun_header
    function_name: (_) @code_function.name)
) @code_function.def
