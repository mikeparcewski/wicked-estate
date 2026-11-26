/* #322 / wicked-estate#636: a string lexer and a guard page for the scanner memory tests.
 * Included after a scanner's own headers, so TSLexer is already defined. */
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>

typedef struct {
    TSLexer base;
    const char *text;
    size_t len, pos;
    uint32_t column;
} StringLexer;

static void sl_advance(TSLexer *l, bool skip_it) {
    (void)skip_it;
    StringLexer *s = (StringLexer *)l;
    if (s->pos < s->len) {
        s->column = s->text[s->pos] == '\n' ? 0 : s->column + 1;
        s->pos++;
    }
    l->lookahead = s->pos < s->len ? (unsigned char)s->text[s->pos] : 0;
}
static void sl_mark_end(TSLexer *l) { (void)l; }
static uint32_t sl_get_column(TSLexer *l) { return ((StringLexer *)l)->column; }
static bool sl_at_range_start(const TSLexer *l) { (void)l; return false; }
static bool sl_eof(const TSLexer *l) { const StringLexer *s = (const StringLexer *)l; return s->pos >= s->len; }
static void sl_log(const TSLexer *l, const char *f, ...) { (void)l; (void)f; }

static StringLexer string_lexer(const char *text) {
    StringLexer s;
    memset(&s, 0, sizeof s);
    s.base.advance = sl_advance;
    s.base.mark_end = sl_mark_end;
    s.base.get_column = sl_get_column;
    s.base.is_at_included_range_start = sl_at_range_start;
    s.base.eof = sl_eof;
    s.base.log = sl_log;
    s.text = text;
    s.len = strlen(text);
    s.base.lookahead = s.len ? (unsigned char)text[0] : 0;
    return s;
}

/* n readable bytes that end exactly at an unmapped page: reading p[n] faults. */
static char *guarded(size_t n) {
    size_t page = (size_t)sysconf(_SC_PAGESIZE);
    size_t span = (n + page - 1) / page * page;
    char *base = mmap(NULL, span + page, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANON, -1, 0);
    if (base == MAP_FAILED || mprotect(base + span, page, PROT_NONE) != 0) {
        perror("guard page");
        _exit(2);
    }
    return base + span - n;
}
