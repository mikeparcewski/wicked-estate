/**
 * Informix 4GL (I4GL) — in-house symbols+calls subset.
 *
 * No usable tree-sitter grammar exists (the canonical reference is the ANTLR grammars-v4 informix
 * grammar). Authored via the template-extrapolate method: a minimal grammar delimiting MAIN /
 * FUNCTION / REPORT / GLOBALS blocks and capturing CALL/RUN statements and function calls.
 * Embedded SQL (SELECT … INTO …, DECLARE … CURSOR, etc. — I4GL has no EXEC SQL prefix) is parsed
 * as token soup. Trust basis is the corpus parse-gate + extraction-count test (RPG/ABL contract).
 *
 * I4GL specifics handled here:
 *   - keywords are case-insensitive (UPPERCASE by convention);
 *   - statements are newline-terminated but newlines are NOT significant (token-soup bodies), so
 *     blocks are delimited by `END MAIN` / `END FUNCTION` / … multi-word tokens only;
 *   - comments: `#` and `--` to end-of-line, and `{ … }` blocks;
 *   - identifiers may be dotted (table.column, record.field); the `$` host-variable sigil and `*`
 *     are soup operators so embedded SQL parses cleanly.
 */

function ci(word, precedence) {
  const pattern = word
    .split('')
    .map((c) => (/[a-zA-Z]/.test(c) ? `[${c.toLowerCase()}${c.toUpperCase()}]` : c.replace(/[.*+?^${}()|[\]\\-]/g, '\\$&')))
    .join('');
  return token(prec(precedence || 0, new RegExp(pattern)));
}

// Multi-word "END X" token at high precedence so a token-soup body never disambiguates a bare END.
function endkw(word) {
  const tail = word
    .split('')
    .map((c) => `[${c.toLowerCase()}${c.toUpperCase()}]`)
    .join('');
  return token(prec(4, new RegExp('[Ee][Nn][Dd][ \\t]+' + tail)));
}

module.exports = grammar({
  name: 'informix4gl',

  extras: ($) => [/\s/, $.comment],

  word: ($) => $.identifier,

  conflicts: ($) => [],

  rules: {
    source_file: ($) => repeat($._item),

    // `#` and `--` line comments; `{ … }` block comment (non-nesting).
    comment: ($) =>
      token(choice(seq('#', /[^\n]*/), seq('--', /[^\n]*/), seq('{', /[^}]*/, '}'))),

    _item: ($) =>
      choice(
        $.main_definition,
        $.function_definition,
        $.report_definition,
        $.call_statement,
        $.run_statement,
        $.call_expression,
        $._token,
      ),

    // MAIN … END MAIN (program entry). The MAIN keyword is aliased to a named node so the entry
    // point is capturable (its text — "MAIN" — becomes the symbol name).
    main_definition: ($) =>
      seq(field('name', alias(ci('MAIN', 3), $.main_keyword)), repeat($._stmt), endkw('MAIN')),

    // FUNCTION name(params) … END FUNCTION. Params are bare identifiers; types are DEFINEd in body.
    function_definition: ($) =>
      seq(ci('FUNCTION', 3), field('name', $.identifier), repeat($._stmt), endkw('FUNCTION')),

    // REPORT name(params) … END REPORT.
    report_definition: ($) =>
      seq(ci('REPORT', 3), field('name', $.identifier), repeat($._stmt), endkw('REPORT')),

    // GLOBALS is not extracted as a named symbol; both the `GLOBALS "file"` and
    // `GLOBALS … END GLOBALS` forms parse as token soup (END GLOBALS is an inner-end token).

    // CALL func(args) [RETURNING …] — RETURNING clause falls through to soup.
    call_statement: ($) =>
      prec.right(seq(ci('CALL', 3), field('function', $.identifier), optional($.arguments))),

    // RUN "cmd"  or  RUN cmd_var — runs an external OS command/program.
    run_statement: ($) => seq(ci('RUN', 3), field('command', choice($.string, $.identifier))),

    // func(args) — function call in an expression / as a value.
    call_expression: ($) => prec(5, seq(field('function', $.identifier), $.arguments)),
    arguments: ($) => seq('(', optional($._arg_list), ')'),
    _arg_list: ($) => repeat1($._arg_token),
    _arg_token: ($) =>
      choice($.call_expression, $.identifier, $.number, $.string, $.operator, ',', '=', '[', ']'),

    _stmt: ($) => choice($.call_statement, $.run_statement, $.call_expression, $._token),

    _token: ($) =>
      choice(
        $.identifier,
        $.number,
        $.string,
        $._end_inner,
        $.operator,
        ',',
        '=',
        '(',
        ')',
        '[',
        ']',
      ),

    // Inner block closers (DEFINE … RECORD … END RECORD; IF/FOR/WHILE/CASE/FOREACH; screen blocks).
    _end_inner: ($) =>
      choice(
        endkw('IF'),
        endkw('FOR'),
        endkw('WHILE'),
        endkw('CASE'),
        endkw('FOREACH'),
        endkw('RECORD'),
        endkw('MENU'),
        endkw('CONSTRUCT'),
        endkw('INPUT'),
        endkw('DISPLAY'),
        endkw('GLOBALS'),
      ),

    // `$` is the dialect host-variable sigil; `*` covers SELECT * and record.* in soup.
    operator: ($) =>
      token(choice('+', '-', '*', '/', '<', '>', '<=', '>=', '<>', '!=', '||', '.', '$', '@')),

    // Dotted identifier so table.column / record.field are one name.
    identifier: ($) => /[A-Za-z_][A-Za-z0-9_]*(\.[A-Za-z_][A-Za-z0-9_]*)*/,

    number: ($) => /[0-9]+(\.[0-9]+)?/,

    string: ($) => choice(token(seq('"', /[^"]*/, '"')), token(seq("'", /[^']*/, "'"))),
  },
});
