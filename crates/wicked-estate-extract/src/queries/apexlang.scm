; wicked_estate APEXlang (Oracle APEX `.apx`) extraction queries.
;
; APEXlang is not extracted by the generic @code_* pass: identity is the path of (kind, key) pairs
; from the application root (ADR-002), which the APEXlang extractor (src/apexlang.rs) mints from
; these captures plus the data in apexlang.toml.

; Every component declaration: `<kind> [<id>] ( ... )`.
(component
  kind: (identifier) @apexlang.component.kind) @apexlang.component

; Every `@reference` written as a property value or array item. References are taken from these
; nodes only, never from an `@` inside text or a fenced block.
(reference) @apexlang.reference

; Every fenced block, with its language tag and body when present.
(fenced_block) @apexlang.fence
