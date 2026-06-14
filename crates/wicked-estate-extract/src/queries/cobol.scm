; COBOL extraction — arborium-cobol grammar, @code_* convention.
; Node types + fields verified against arborium-cobol-2.18.0 node-types.json:
;   paragraph_header.name = WORD|integer ; section_header.name = WORD|integer ;
;   program_definition (node) ; call_statement.x (the called program) ;
;   perform_statement_call_proc.procedure (the PERFORMed paragraph).

; PROGRAM-ID NAME ... → a program is a module named by its PROGRAM-ID. Naming it lets cross-program
; refs resolve to it: a JCL `EXEC PGM=NAME`, an HLASM `CALL NAME`, or another COBOL `CALL 'NAME'`.
(program_definition
  (identification_division
    (program_name) @code_module.name)) @code_module.def

; Paragraphs are COBOL's procedures (the unit CALL/PERFORM target).
(paragraph_header
  name: (WORD) @code_function.name) @code_function.def

; Sections group paragraphs.
(section_header
  name: (WORD) @code_function.name) @code_function.def

; CALL "SUBPROG" USING ... → cross-program call
(call_statement
  x: (_) @call.function) @call

; PERFORM <paragraph> → intra-program call
(perform_statement_call_proc
  procedure: (_) @call.function) @call

; Data items (WORKING-STORAGE / LINKAGE / copybook fields). Each becomes a node — this is how
; "Advanced Data Formats" (COMP/COMP-3/Zoned/Signed usages) and "Complex Copybook Structures"
; (OCCURS arrays, REDEFINES overlays) enter the graph. The usage/picture live on the parsed
; subtree; surfacing them as node metadata is a depth refinement.
(data_description
  (entry_name) @code_field.name) @code_field.def

; REDEFINES <item> → reference from this field to the item it overlays.
; (Calls is the codebase's documented "references-by-name" proxy — no dedicated Refs edge kind.)
(data_description
  (redefines_clause (qualified_word) @call.function)) @call

; OCCURS n DEPENDING ON <counter> → reference to the counter field that sizes the array.
(data_description
  (occurs_clause depending: (qualified_word) @call.function)) @call
