/*
 * The arborium-cobol language with the patched external scanner in `scanner.c` installed.
 *
 * `wicked_cobol_language(base)` copies the crate's TSLanguage (parse tables, lexer, symbol names:
 * all unchanged) once, swaps in the five external-scanner functions, and returns the copy. The
 * copy is static and never freed, like every compiled-in tree-sitter language.
 * `tree_sitter/parser.h` is byte-identical to arborium-cobol 2.18.0's, so the struct layout is the
 * one `parser.c` was compiled against; the ABI check below refuses anything else.
 */
#include "scanner.c"

static TSLanguage wicked_cobol;
static int wicked_cobol_ready = 0;

/* Returns NULL when `base` is not an ABI-15 language with an external scanner. The caller
 * (`lang_cobol` in treesitter.rs) runs once under a Rust `OnceLock`, so this is never raced. */
const TSLanguage *wicked_cobol_language(const TSLanguage *base) {
    if (wicked_cobol_ready) {
        return &wicked_cobol;
    }
    if (base == NULL || base->abi_version != 15 || base->external_scanner.scan == NULL
        || base->external_token_count != 7) {
        return NULL;
    }
    wicked_cobol = *base;
    wicked_cobol.external_scanner.create = wicked_cobol_scanner_create;
    wicked_cobol.external_scanner.destroy = wicked_cobol_scanner_destroy;
    wicked_cobol.external_scanner.scan = wicked_cobol_scanner_scan;
    wicked_cobol.external_scanner.serialize = wicked_cobol_scanner_serialize;
    wicked_cobol.external_scanner.deserialize = wicked_cobol_scanner_deserialize;
    wicked_cobol_ready = 1;
    return &wicked_cobol;
}
