# tree-sitter-apexlang

**Oracle APEXlang** (`.apx`, Oracle APEX 26.1+) grammar for tree-sitter: the core syntax only.

Derived from the "Core layout and values" section of Oracle's
[`apexlang.ebnf`](https://github.com/oracle/skills/blob/014cdb29f12dfa6a689bbe8a80ee1c5f0bf15be1/apex/apexlang/26.1/apexlang-generation/assets/grammar/apexlang.ebnf)
(UPL-1.0) and checked against Oracle's generated applications. The component and property schema (the
rest of the EBNF, 1,464 property productions) is deliberately not encoded: SQLcl `apex validate` owns
schema validation, and a schema-sized parse table would only add states.

Where Oracle's applications differ from the EBNF's value rules, the grammar follows the applications:
a value is the rest of its line (`- Select -`, `#DEFAULT#`, a lone `"`), references are `@` plus
non-blank characters (`@slug`, `@/theme-template`, `@region/accent`), property keys may be quoted
inside object values, and a fenced block opens on the line after its `key:`.

- **Nodes:** `component` (`kind`, `id`), `group` (`name`), `property` (`key`, `value`),
  `object`, `array` (`reference` / `string` / `word` items), `reference`, `string`, `text`,
  `fenced_block` (`language`, `body`), `comment`.
- **Fenced blocks:** `src/scanner.c` lexes the body as one token whose byte range is exactly the text
  between the fences; an empty block has no body node.
- **Corpus gate:** zero ERROR and zero MISSING nodes over the pinned 3,160-file corpus
  (`oracle/apexlang/corpus.tsv`, fetched by `scripts/apexlang-corpus.py`), run in CI.
- **Generated with** tree-sitter-cli 0.25.10 (`tree-sitter generate` in this directory; CI checks the
  output is byte-identical). ABI 15, `STATE_COUNT` 55.

Compiled by `wicked-estate-extract`'s `build.rs` rather than as its own crate: crates.io Trusted
Publishing cannot create a new crate name, so a separate crate would hold every release on a manual
bootstrap. The layout is the standard one, so it can become a crate later without regenerating.

License: UPL-1.0 (`LICENSE`), as the grammar derives from Oracle's UPL-1.0 EBNF. Test inputs in
`test/corpus/` are written for this grammar; none are copied from docs.oracle.com.
