/*
 * COBOL external scanner: a patched copy of arborium-cobol 2.18.0 `grammar/scanner.c`
 * (upstream zharinov/tree-sitter-cobol @ befb226951b65585617f42ecbe526d00c124100c, MIT).
 * Installed over the crate's own scanner by `cobol_language.c`; the parse tables are the crate's.
 *
 * Patches (wicked-estate, marked `WICKED PATCH`):
 *   1. Error recovery. While tree-sitter recovers from a syntax error it offers EVERY external
 *      token. The upstream scanner then returned COMMENT_ENTRY (an IDENTIFICATION DIVISION
 *      free-text line) for each remaining line, so one unparsed statement swallowed the rest of
 *      the file into a single ERROR node. Here a scan that is offered every token never returns
 *      COMMENT_ENTRY or a multi-line string, so the parser resyncs on the next real token.
 *   2. EXEC after an open statement. Where a quoted literal could come next (after
 *      `PERFORM X THRU Y` with no period), the upstream scanner returned false as soon as the
 *      next character was not `"`, before it tried EXEC_BLOCK. `EXEC CICS ...` there became an
 *      ERROR. The literal branch now runs only on `"`, and the EXEC branch is still tried.
 *   3. An EXEC block with no END-EXEC. The upstream scan looked for END-EXEC up to the end of the
 *      file. With patches 1 and 2 the parser retries at every later EXEC, so a run of unterminated
 *      blocks cost a scan to EOF each (quadratic). A block never holds another block opener
 *      (`EXEC` then SQL / CICS / DLI) outside its literals and comments, so the scan now fails at
 *      the next one: each failing scan stops at the next opener. A terminated block is matched
 *      exactly as before.
 *   4. Recovery that takes over the file (wicked-estate#583). On text the grammar mostly rejects
 *      (a non-COBOL file saved as `.cbl`, a dialect it cannot read), patch 1 keeps the parser
 *      recovering token by token, and tree-sitter's recovery then walks ever longer runs of its
 *      stack: the parse grows faster than the file. The scanner counts, per parse, the scans
 *      made while recovering and the others. Once recovering scans pass RECOVERY_FLOOR and are
 *      more than one in RECOVERY_RATIO of the others, patch 1 is off for the rest of that parse:
 *      upstream's recovery, one ERROR to the end, in linear time. Real COBOL recovers from a
 *      statement in a few scans and never gets there. The count lives in the scanner's payload,
 *      so it is per parser, and the extractor makes one parser per file.
 *   5. A file that ends inside the sequence area (wicked-estate#634). The upstream loop over
 *      columns 1-6 advanced until column 6 with no end-of-input check; at the end of the file
 *      the column stops moving, so the scan never returned. It now stops at end of input too.
 *   6. The keyword matcher (wicked-estate#636). Upstream advanced and read every keyword's pointer,
 *      including keywords already ruled out, so a short one was read past its terminating NUL
 *      while a long one still matched. Only the keywords still matching are read now.
 * Every function and table is `static`, so nothing clashes with the crate's own scanner symbols,
 * except `wicked_cobol_take_recovery_took_over` (patch 4's report to the extractor).
 */
#include "tree_sitter/parser.h"
#include <stdlib.h>
#include <wctype.h>

enum TokenType {
    WHITE_SPACES,
    LINE_PREFIX_COMMENT,
    LINE_SUFFIX_COMMENT,
    LINE_COMMENT,
    COMMENT_ENTRY,
    multiline_string,
    EXEC_BLOCK,
};

// WICKED PATCH 4: the scans one parse made while recovering, and the others.
typedef struct {
    unsigned recovering;
    unsigned other;
    bool upstream_recovery;
} WickedCobolScanner;

// Measured on AWS CardDemo's 106 COBOL files (wicked-estate#583): at most 1,295 recovering scans
// (a procedure copybook) and, in programs, at most 1 per 89 others. Generated non-COBOL text:
// 0.7 to 2.3 recovering scans per other one.
#define RECOVERY_FLOOR 8192u
#define RECOVERY_RATIO 4u

// Set on the parsing thread when patch 4 turns patch 1 off, so the extractor can report the file
// (never a silent truncation). The parse runs the scanner on the caller's thread.
#if defined(_MSC_VER)
#define WICKED_THREAD_LOCAL __declspec(thread)
#else
#define WICKED_THREAD_LOCAL __thread
#endif
static WICKED_THREAD_LOCAL int wicked_cobol_recovery_took_over = 0;

/* Whether patch 4 turned patch 1 off on this thread since the last call; clears it. */
int wicked_cobol_take_recovery_took_over(void) {
    int took = wicked_cobol_recovery_took_over;
    wicked_cobol_recovery_took_over = 0;
    return took;
}

static void *wicked_cobol_scanner_create(void) {
    return calloc(1, sizeof(WickedCobolScanner));
}

static bool is_white_space(int c) {
    return iswspace(c) || c == ';' || c == ',';
}

#define MAX_KEYWORDS 9
static const int number_of_comment_entry_keywords = MAX_KEYWORDS;
static char* any_content_keyword[] = {
    "author",
    "installation",
    "date-written",
    "date-compiled",
    "security",
    "identification division",
    "environment division",
    "data division",
    "procedure division",
};

static bool start_with_word( TSLexer *lexer, char *words[], int number_of_words) {
    while(lexer->lookahead == ' ' || lexer->lookahead == '\t') {
        lexer->advance(lexer, true);
    }

    char *keyword_pointer[MAX_KEYWORDS];
    bool continue_check[MAX_KEYWORDS];
    for(int i=0; i<number_of_words; ++i) {
        keyword_pointer[i] = words[i];
        continue_check[i] = true;
    }

    while(true) {
        // At the end of the line
        if(lexer->get_column(lexer) > 71 || lexer->lookahead == '\n' || lexer->lookahead == 0) {
            return false;
        }

        // If all keyword matching fails, move to the end of the line
        bool all_match_failed = true;
        for(int i=0; i<number_of_words; ++i) {
            if(continue_check[i]) {
                all_match_failed = false;
            }
        }

        if(all_match_failed) {
            for(; lexer->get_column(lexer) < 71 && lexer->lookahead != '\n' && lexer->lookahead != 0;
            lexer->advance(lexer, true)) {
            }
            return false;
        }

        // If the head of the line matches any of specified keywords, return true;
        // WICKED PATCH 6: read and advance only the keywords still matching, so a pointer never
        // passes its keyword's terminating NUL (wicked-estate#636).
        char c = lexer->lookahead;
        for(int i=0; i<number_of_words; ++i) {
            if(continue_check[i] && *(keyword_pointer[i]) == 0) {
                return true;
            }
        }

        // matching keywords
        for(int i=0; i<number_of_words; ++i) {
            if(continue_check[i]) {
                char k = *(keyword_pointer[i]);
                continue_check[i] = c == towupper(k) || c == towlower(k);
                (keyword_pointer[i])++;
            }
        }

        // next character
        lexer->advance(lexer, true);
    }

    return false;
}

// Case-insensitive character match
static bool char_eq_i(int c, char expected) {
    return towlower(c) == towlower(expected);
}

// Try to match a keyword case-insensitively, advancing lexer if matched
static bool match_keyword(TSLexer *lexer, const char *keyword) {
    for (const char *p = keyword; *p; p++) {
        if (!char_eq_i(lexer->lookahead, *p)) {
            return false;
        }
        lexer->advance(lexer, false);
    }
    return true;
}

// Scan EXEC ... END-EXEC block
static bool scan_exec_block(TSLexer *lexer) {
    if (!match_keyword(lexer, "EXEC")) {
        return false;
    }
    if (!iswspace(lexer->lookahead)) {
        return false;
    }

    while (iswspace(lexer->lookahead)) {
        lexer->advance(lexer, false);
    }

    // Only accept known EXEC types: SQL, CICS, DLI
    char first = towlower(lexer->lookahead);
    if (first != 's' && first != 'c' && first != 'd') {
        return false;
    }

    // Scan until END-EXEC (case insensitive state machine)
    const char *end_exec = "END-EXEC";
    int state = 0;
    // WICKED PATCH 3: fail at the next block opener: a word `EXEC`, then blanks, then a word
    // starting like SQL / CICS / DLI (the opener test above), outside literals and comments.
    // `next`: letters of `EXEC` matched at a word start; 4 = all of them, 5 = blanks after.
    const char *exec_word = "EXEC";
    int next = 0;
    bool word_start = false;
    int quote = 0;
    bool comment = false;
    int prev = 0;
    int col = (int)lexer->get_column(lexer);

    while (lexer->lookahead != 0) {
        char c = lexer->lookahead;
        int w = lexer->lookahead;

        if (w == '\n') {
            comment = false;
            quote = 0;
            next = next >= 4 ? 5 : 0;
            word_start = true;
        } else if (comment) {
        } else if (quote) {
            if (w == quote) {
                quote = 0;
            }
        } else if (col == 6 && (w == '*' || w == '/')) {
            comment = true;
        } else if (w == '\'' || w == '"') {
            quote = w;
            next = 0;
            word_start = false;
        } else if ((w == '-' && prev == '-') || (w == '>' && prev == '*')) {
            comment = true;
            next = 0;
        } else {
            if (next >= 4) {
                if (is_white_space(w)) {
                    next = 5;
                } else if (next == 5) {
                    int t = towlower(w);
                    if (t == 's' || t == 'c' || t == 'd') {
                        return false;
                    }
                    next = 0;
                } else {
                    next = 0;
                }
            } else if (next > 0) {
                next = char_eq_i(w, exec_word[next]) ? next + 1 : 0;
            } else if (word_start && char_eq_i(w, 'E')) {
                next = 1;
            }
            word_start = is_white_space(w) || w == '.';
        }
        prev = w;
        col = w == '\n' ? 0 : col + 1;

        if (char_eq_i(c, end_exec[state])) {
            state++;
            if (end_exec[state] == 0) {
                lexer->advance(lexer, false);
                lexer->mark_end(lexer);
                lexer->result_symbol = EXEC_BLOCK;
                return true;
            }
        } else if (char_eq_i(c, 'E')) {
            state = 1;
        } else {
            state = 0;
        }

        lexer->advance(lexer, false);
    }

    return false;
}

static bool wicked_cobol_scanner_scan(void *payload, TSLexer *lexer,
                                            const bool *valid_symbols) {
    if(lexer->lookahead == 0) {
        return false;
    }

    // WICKED PATCH 1: tree-sitter offers every external token only while it recovers from an
    // error. No parse state of this grammar has all of them valid at once.
    const bool every_token = valid_symbols[WHITE_SPACES] && valid_symbols[LINE_PREFIX_COMMENT]
        && valid_symbols[LINE_SUFFIX_COMMENT] && valid_symbols[LINE_COMMENT]
        && valid_symbols[COMMENT_ENTRY] && valid_symbols[multiline_string]
        && valid_symbols[EXEC_BLOCK];
    // WICKED PATCH 4: once recovery has taken over the parse, recover as upstream does.
    WickedCobolScanner *state = (WickedCobolScanner *)payload;
    if(state != NULL && !state->upstream_recovery) {
        if(every_token) {
            state->recovering++;
            if(state->recovering > RECOVERY_FLOOR
               && state->recovering * RECOVERY_RATIO > state->other) {
                state->upstream_recovery = true;
                wicked_cobol_recovery_took_over = 1;
            }
        } else {
            state->other++;
        }
    }
    const bool recovering = every_token && !(state != NULL && state->upstream_recovery);

    if(valid_symbols[WHITE_SPACES]) {
        if(is_white_space(lexer->lookahead)) {
            while(is_white_space(lexer->lookahead)) {
                lexer->advance(lexer, true);
            }
            lexer->result_symbol = WHITE_SPACES;
            lexer->mark_end(lexer);
            return true;
        }
    }

    if(valid_symbols[LINE_PREFIX_COMMENT] && lexer->get_column(lexer) <= 5) {
        // WICKED PATCH 5: stop at end of input, where the column no longer advances.
        while(lexer->get_column(lexer) <= 5 && lexer->lookahead != 0) {
            lexer->advance(lexer, true);
        }
        lexer->result_symbol = LINE_PREFIX_COMMENT;
        lexer->mark_end(lexer);
        return true;
    }

    if(valid_symbols[LINE_COMMENT]) {
        if(lexer->get_column(lexer) == 6) {
            if(lexer->lookahead == '*' || lexer->lookahead == '/') {
                while(lexer->lookahead != '\n' && lexer->lookahead != 0) {
                    lexer->advance(lexer, true);
                }
                lexer->result_symbol = LINE_COMMENT;
                lexer->mark_end(lexer);
                return true;
            } else {
                lexer->advance(lexer, true);
                lexer->mark_end(lexer);
                return false;
            }
        }
    }

    if(valid_symbols[LINE_SUFFIX_COMMENT]) {
        if(lexer->get_column(lexer) >= 72) {
            while(lexer->lookahead != '\n' && lexer->lookahead != 0) {
                lexer->advance(lexer, true);
            }
            lexer->result_symbol = LINE_SUFFIX_COMMENT;
            lexer->mark_end(lexer);
            return true;
        }
    }

    if(valid_symbols[COMMENT_ENTRY] && !recovering) {
        if(!start_with_word(lexer, any_content_keyword, number_of_comment_entry_keywords)) {
            lexer->mark_end(lexer);
            lexer->result_symbol = COMMENT_ENTRY;
            return true;
        } else {
            return false;
        }
    }

    // WICKED PATCH 2: enter the literal branch only on `"`, so EXEC_BLOCK below is still tried.
    if(valid_symbols[multiline_string] && !recovering && lexer->lookahead == '"') {
        while(true) {
            if(lexer->lookahead != '"') {
                return false;
            }
            lexer->advance(lexer, false);
            while(lexer->lookahead != '"' && lexer->lookahead != 0 && lexer->get_column(lexer) < 72) {
                lexer->advance(lexer, false);
            }
            if(lexer->lookahead == '"') {
                lexer->result_symbol = multiline_string;
                lexer->advance(lexer, false);
                lexer->mark_end(lexer);
                return true;
            }
            while(lexer->lookahead != 0 && lexer->lookahead != '\n') {
                lexer->advance(lexer, true);
            }
            if(lexer->lookahead == 0) {
                return false;
            }
            lexer->advance(lexer, true);
            int i;
            for(i=0; i<=5; ++i) {
                if(lexer->lookahead == 0 || lexer->lookahead == '\n') {
                    return false;
                }
                lexer->advance(lexer, true);
            }

            if(lexer->lookahead != '-') {
                return false;
            }

            lexer->advance(lexer, true);
            while(lexer->lookahead == ' ' && lexer->get_column(lexer) < 72) {
                lexer->advance(lexer, true);
            }
        }
    }

    if(valid_symbols[EXEC_BLOCK]) {
        if(scan_exec_block(lexer)) {
            return true;
        }
    }

    return false;
}

static unsigned wicked_cobol_scanner_serialize(void *payload, char *buffer) {
    return 0;
}

static void wicked_cobol_scanner_deserialize(void *payload, const char *buffer, unsigned length) {
}

static void wicked_cobol_scanner_destroy(void *payload) {
    free(payload);
}
