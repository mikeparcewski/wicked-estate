# Hand-written C review (#322, PROVENANCE rule 7)

Every C or assembly source or header under `vendor/` that the pinned CLI does not generate is
listed here against the git blob that was reviewed. `scripts/test_scanner_review.py` fails CI when
such a file has no row, when its bytes differ from the reviewed blob, or when a row names a file
that is gone. The only files exempt are the ones `scripts/regen-vendored-parsers.py` writes next to
each `src/grammar.json` (`parser.c` and the `src/tree_sitter/` headers); CI checks those by
regenerating them (rule 5).

## How a file is reviewed

Read every line. For each loop and each scan function, check:

- **Termination at the end of the input.** At EOF `advance` does nothing, `lookahead` stays 0,
  `eof()` is true and `get_column` stops moving. A loop must stop on one of those, or on a bound
  that every iteration moves.
- **Work per scan.** A scan that fails after reading to the end of the file, and that the parser
  retries at each later position, is quadratic in the file.
- **Memory.** Fixed buffers are indexed within their size; `serialize` writes stay within
  `TREE_SITTER_SERIALIZATION_BUFFER_SIZE`; `deserialize` reads stay within `length`.
- **Ownership.** Every allocation is freed once, on every return path.

Then record the file's blob, the outcome and every finding with what was done about it. A
finding is fixed (upstream, or as a marked `WICKED PATCH` per rule 7), filed as an issue, or
accepted with the reason.

## Reviews

The 2026-11-25 review. Two model reviewers read every line of each file against the classes above:
one review, then a second as round 1 of the PR that added this record, which found findings 4-6.
The lane checked each finding against the code, and reproduced each hang with the pinned
tree-sitter CLI (0.25.10) before it was fixed.

| File | Reviewed blob | Lines | Outcome |
|---|---|---|---|
| `tree-sitter-apexlang/src/scanner.c` | `f15f26bd4b8ea297dedc163929619086a9cbf07f` | 75 | Sound. Stateless. Every loop stops at `eof()`, and each pass of the main loop consumes a line or returns. |
| `tree-sitter-cfml/cfml/src/scanner.c` | `74cd1565e362b3d39a10792fdfc20e0689b0587e` | 34 | Sound. A wrapper over `common/scanner.h`; `destroy` frees each tag and both arrays once. Finding A. |
| `tree-sitter-cfml/cfml/src/tag.c` | `36788ffa85f346445c8d79fb3eedd0cab5202233` | 1 | Sound. One line, `#include "../../common/tag.h"` (reviewed in that row), and not compiled: the build script compiles only `parser.c` and `scanner.c`. |
| `tree-sitter-cfml/cfscript/src/scanner.c` | `978a8c8ba978f44d557a0cfab62ea20b6aad3784` | 404 | Finding 1, fixed here as `WICKED PATCH 1`. The rest is sound: stateless, and every other loop stops at 0, at `eof()` or after a fixed count. |
| `tree-sitter-cfml/common/scanner.h` | `38519e8ad837977ada33aba91c8189d59a085860` | 1628 | Finding 2 (#631). Findings 4 and 5, fixed as `WICKED PATCH 2` and `3` (#636). Findings A and B. Serialization writes are guarded against the buffer size. Fixed buffers (`end_delimiter[256]`, `word[16]`) are bounded by length checks. Every content scan advances on each pass and stops at 0. |
| `tree-sitter-cfml/common/tag.h` | `d441dc4fc5f21b35db1d279be38e2e19ec8b6ab7` | 416 | Sound. Fixed tables (the 126-entry tag map holds 126 entries; the void-tag list ends in NULL). `tag_free` frees the name of exactly the tag types that own one. |
| `tree-sitter-cobol-scanner/cobol_language.c` | `aef9c1c228aaa02de50ea812fbfe044c720b6ebb` | 33 | Sound. It copies the crate's language once and refuses anything but ABI 15 with 7 external tokens. |
| `tree-sitter-cobol-scanner/scanner.c` | `9de18c6c38300c16ed9599ec73a234f2aad39245` | 426 | Finding 3, fixed as `WICKED PATCH 5` (#634). Finding 6, fixed as `WICKED PATCH 6` (#636). Finding A. Patches 1-4 keep error recovery linear. Apart from the loop in finding 3, every loop stops at 0, or at a column bound together with a character test that fails at 0. |
| `tree-sitter-cobol-scanner/tree_sitter/parser.h` | `858107deb9e4a1ffcd2ed5e6ffe98a5fcab3e1f6` | 286 | Sound. Type and macro definitions only, byte-identical to arborium-cobol 2.18.0's (the ABI the crate's `parser.c` was compiled against). |

## Findings

1. **cfscript `scan_query_text` never returned at the end of the input** (hang). The loop waited
   for a closing `"` with no end-of-input check, so a `.cfs` file ending inside a
   `queryExecute("` string never finished parsing. Fixed by `WICKED PATCH 1`, with a test in
   `legacy_family_realworld.rs`. The fuzz smoke now cuts every token off inside a string it
   opens.
2. **`scan_comment` is quadratic on unterminated nested `<!--`.** Each scan reads to the end of
   the file and fails, and the parser retries it at the next `<!`. Filed as #631. The fuzz smoke
   lists the shape as known slow until then.
3. **COBOL's sequence-area loop never returned at the end of the input** (hang). It advanced
   until column 6 with no end-of-input check. Fixed by `WICKED PATCH 5` (#634). The extractor's
   `cobol_scanner_fuzz_smoke` test now cuts files off in every column of the sequence area.
4. **cfml `deserialize` can read past its buffer** (`DESERIALIZE_TAGS`). When the HTML tag stack
   nearly fills the 1,024-byte buffer, `serialize` skips the CF stack's header, but `deserialize`
   reads that header and the records its stale counts name, without checking `length`. Fixed by
   `WICKED PATCH 2`: it reads from a zero-padded copy, so a missing header is an empty stack (#636).
5. **cfml leaks a start tag's name** for `cfset`, `cfreturn`, `cfelseif` and `cfelse`:
   `scan_start_tag_name` returns without `tag_free`. Fixed by `WICKED PATCH 3` (#636).
6. **COBOL `start_with_word` reads past ruled-out keywords.** It advances and dereferences every
   keyword's pointer, including keywords that already failed, so a short one (`author`) is read
   past its terminating NUL while a long one is still matching. Fixed by `WICKED PATCH 6` (#636).
   Findings 4-6 each have a test in `scripts/test_scanner_memory.py`: a C harness drives the
   function directly, with a guard page that faults on an over-read and allocators that count.

Accepted, with the reason:

- **A. An unchecked allocation** (`ts_calloc` / `calloc` in `create`) crashes only when memory
  runs out. It is the upstream idiom, and tree-sitter's own allocator aborts there too.
- **B. `serialize` caps the tag count at 65,535 and each name at 255 bytes**, and the depth
  counters are 16-bit. Deeper or longer input round-trips lossily; that is a parse-accuracy
  limit, not a memory fault.
