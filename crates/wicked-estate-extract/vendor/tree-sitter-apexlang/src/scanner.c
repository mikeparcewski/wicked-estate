// External scanner for tree-sitter-apexlang: the body of a fenced block.
//
// A fenced block is
//
//     key:
//         ```plsql
//         <body lines>
//         ```
//
// The grammar lexes the opening fence line itself; this scanner is asked for FENCE_BODY at the
// start of the next line. The body is every following line up to (not including) the first line
// whose text, after leading blanks, is exactly ``` followed only by blanks. The token therefore
// starts at the first body line and ends at the start of the closing fence line: its byte range is
// exactly the text between the fences. An empty block (the closing fence on the next line)
// yields no body token. A missing closing fence ends the body at end of input, and the grammar's
// missing ``` then shows as a MISSING node, never as silently swallowed text.

#include "tree_sitter/parser.h"

#include <stdbool.h>

enum TokenType {
  FENCE_BODY,
  ERROR_SENTINEL,
};

void *tree_sitter_apexlang_external_scanner_create(void) { return NULL; }

void tree_sitter_apexlang_external_scanner_destroy(void *payload) { (void)payload; }

unsigned tree_sitter_apexlang_external_scanner_serialize(void *payload, char *buffer) {
  (void)payload;
  (void)buffer;
  return 0;
}

void tree_sitter_apexlang_external_scanner_deserialize(void *payload, const char *buffer,
                                                       unsigned length) {
  (void)payload;
  (void)buffer;
  (void)length;
}

static bool is_blank(int32_t c) { return c == ' ' || c == '\t'; }

// At a line start: consume leading blanks and report whether the rest of the line is a closing
// fence. Consumes characters; the caller has already marked the token end before this line.
static bool line_is_closing_fence(TSLexer *lexer) {
  while (is_blank(lexer->lookahead)) lexer->advance(lexer, false);
  for (int i = 0; i < 3; i++) {
    if (lexer->lookahead != '`') return false;
    lexer->advance(lexer, false);
  }
  while (is_blank(lexer->lookahead)) lexer->advance(lexer, false);
  if (lexer->lookahead == '\r') lexer->advance(lexer, false);
  return lexer->lookahead == '\n' || lexer->eof(lexer);
}

bool tree_sitter_apexlang_external_scanner_scan(void *payload, TSLexer *lexer,
                                                const bool *valid_symbols) {
  (void)payload;
  if (valid_symbols[ERROR_SENTINEL] || !valid_symbols[FENCE_BODY]) return false;

  bool consumed = false;
  lexer->result_symbol = FENCE_BODY;
  for (;;) {
    if (lexer->eof(lexer)) return consumed;
    if (line_is_closing_fence(lexer)) return consumed;
    // Not the closing fence: the whole line (including its newline) belongs to the body.
    while (lexer->lookahead != '\n' && !lexer->eof(lexer)) lexer->advance(lexer, false);
    if (lexer->lookahead == '\n') lexer->advance(lexer, false);
    lexer->mark_end(lexer);
    consumed = true;
  }
}
