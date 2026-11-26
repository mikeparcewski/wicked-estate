/* wicked-estate#636 finding 3, in the vendored COBOL scanner: start_with_word must not read past
 * a keyword it has already ruled out. "author" ends at an unmapped page, so reading past its NUL
 * while "identification division" is still matching faults. Built with -I on the scanner's dir. */
#include "scanner.c"
#include "harness.h"

int main(void) {
    char *author = guarded(sizeof "author");
    memcpy(author, "author", sizeof "author");
    char *words[] = {author, "identification division"};
    StringLexer lexer = string_lexer("IDENTIFICATION DIVISION.\n");
    bool matched = start_with_word(&lexer.base, words, 2);
    printf("matched: %d\n", matched);
    return matched ? 0 : 1;
}
