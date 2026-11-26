/* wicked-estate#636 findings 1 and 2, in the vendored cfml scanner (common/scanner.h).
 *   leak:        cfset, cfreturn, cfelseif and cfelse start tags must not leak their names.
 *   deserialize: a buffer whose HTML tag stack nearly fills it must deserialize within its length.
 * Built by scripts/test_scanner_memory.py with -I on cfml/src. */
#include <stdlib.h>
#include <string.h>

/* memcpy's arguments must not be NULL even for zero bytes; glibc declares that, macOS does not.
 * Declare it here too, so the sanitizer checks it on every host. */
#undef memcpy
extern void *memcpy(void *, const void *, size_t) __attribute__((nonnull(1, 2)));

static long live = 0;
static void *count_malloc(size_t n) { live++; return malloc(n); }
static void *count_calloc(size_t c, size_t n) { live++; return calloc(c, n); }
static void *count_realloc(void *p, size_t n) { if (!p) live++; return realloc(p, n); }
static void count_free(void *p) { if (p) live--; free(p); }
#define ts_malloc count_malloc
#define ts_calloc count_calloc
#define ts_realloc count_realloc
#define ts_free count_free

#include "scanner.c"
#include "harness.h"

static int leak(void) {
    const char *tags[] = {"cfset x = 1>", "cfreturn x>", "cfelseif x>", "cfelse>"};
    Scanner *scanner = tree_sitter_cfml_external_scanner_create();
    for (unsigned i = 0; i < sizeof tags / sizeof *tags; i++) {
        StringLexer lexer = string_lexer(tags[i]);
        if (!scan_start_tag_name(scanner, &lexer.base, true, false)) {
            printf("scan of %s failed\n", tags[i]);
            return 2;
        }
    }
    tree_sitter_cfml_external_scanner_destroy(scanner);
    printf("live allocations after destroy: %ld\n", live);
    return live == 0 ? 0 : 1;
}

static int deserialize_bound(void) {
    Scanner *full = tree_sitter_cfml_external_scanner_create();
    for (int i = 0; i < 1017; i++) {
        Tag tag = tag_new();
        tag.type = DIV;
        array_push(&full->tags, tag);
    }
    char buffer[TREE_SITTER_SERIALIZATION_BUFFER_SIZE];
    unsigned length = tree_sitter_cfml_external_scanner_serialize(full, buffer);
    char *copy = guarded(length);
    memcpy(copy, buffer, length);
    Scanner *scanner = tree_sitter_cfml_external_scanner_create();
    tree_sitter_cfml_external_scanner_deserialize(scanner, copy, length);
    printf("serialized %u bytes; deserialized %u HTML tags, %u CF tags\n", length, scanner->tags.size,
           scanner->cf_tags.size);
    return scanner->tags.size == 1017 && scanner->cf_tags.size == 0 ? 0 : 1;
}

/* tree-sitter resets a scanner with deserialize(NULL, 0); that must not hand NULL to memcpy. */
static int reset(void) {
    Scanner *scanner = tree_sitter_cfml_external_scanner_create();
    tree_sitter_cfml_external_scanner_deserialize(scanner, NULL, 0);
    tree_sitter_cfml_external_scanner_destroy(scanner);
    printf("reset ok\n");
    return 0;
}

int main(int argc, char **argv) {
    if (argc == 2 && strcmp(argv[1], "reset") == 0) return reset();
    if (argc == 2 && strcmp(argv[1], "leak") == 0) return leak();
    if (argc == 2 && strcmp(argv[1], "deserialize") == 0) return deserialize_bound();
    fprintf(stderr, "usage: cfml_memory leak|deserialize|reset\n");
    return 2;
}
