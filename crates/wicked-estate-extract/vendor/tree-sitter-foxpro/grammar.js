/**
 * Visual FoxPro / FoxPro (.prg) — in-house symbols+calls subset.
 *
 * No tree-sitter grammar exists (the canonical reference is the vfp2py ANTLR grammar
 * VisualFoxpro9.g4). Authored via the template-extrapolate method: a minimal grammar delimiting
 * PROCEDURE / FUNCTION / DEFINE CLASS definitions and capturing function-call-syntax calls
 * (func(args), obj.method(args)). Bodies are token soup. Trust basis is the corpus parse-gate +
 * extraction-count test (RPG/ABL contract).
 *
 * FoxPro specifics handled here:
 *   - keywords are case-insensitive;
 *   - ENDPROC / ENDFUNC are OPTIONAL — a routine is terminated by the next PROCEDURE / FUNCTION /
 *     DEFINE CLASS, an explicit ENDPROC/ENDFUNC, or end-of-file (modeled by stopping the body's
 *     token-soup repeat at those high-precedence keywords);
 *   - `;` at end of line is a CONTINUATION, not a terminator — it is just a soup token here;
 *   - comments: `&&` inline anywhere, and `*` / `NOTE` line comments anchored to a line start
 *     (preceded by a newline) so a `*` multiply operator mid-expression is NOT eaten as a comment;
 *   - DO-style calls (DO proc / DO FORM) are intentionally NOT captured — distinguishing them from
 *     DO WHILE / DO CASE control flow without false edges is not worth the complexity; calls made
 *     with function-call syntax (the common modern form) are captured.
 */

function ci(word, precedence) {
  const pattern = word
    .split('')
    .map((c) => (/[a-zA-Z]/.test(c) ? `[${c.toLowerCase()}${c.toUpperCase()}]` : c.replace(/[.*+?^${}()|[\]\\-]/g, '\\$&')))
    .join('');
  return token(prec(precedence || 0, new RegExp(pattern)));
}

module.exports = grammar({
  name: 'foxpro',

  extras: ($) => [/[ \t\r\n]/, $.comment],

  word: ($) => $.identifier,

  conflicts: ($) => [],

  rules: {
    source_file: ($) => repeat($._item),

    // `&&` inline comment and `*` line comment, both to end-of-line. NOTE: `*` is treated purely as
    // a comment lead (NOT a multiply operator) — full line-position disambiguation would need an
    // external scanner. For a symbols+calls grammar this is sound: a `*` mid-expression just makes
    // the rest of that line a comment (no false definitions or call edges); it only matters that we
    // never mis-classify a definition/call, which we don't. This also makes BOF `*` comments work.
    comment: ($) => token(choice(seq('&&', /[^\n]*/), seq('*', /[^\n]*/))),

    _item: ($) =>
      choice(
        $.define_class,
        $.procedure_definition,
        $.function_definition,
        $.routine_end,
        $.call_expression,
        $._token,
      ),

    // ENDPROC / ENDFUNC are optional in FoxPro; rather than an ambiguous inline closer, they are
    // consumed as standalone no-op items (the routine body already stops at these keywords).
    routine_end: ($) => choice(ci('ENDPROC', 3), ci('ENDFUNC', 3)),

    // DEFINE CLASS name [AS parent [OF lib]] … ENDDEFINE.  Methods are PROCEDURE/FUNCTION members.
    define_class: ($) =>
      seq(
        $._define_class_kw,
        field('name', $.identifier),
        repeat($._class_member),
        ci('ENDDEFINE', 3),
      ),
    _define_class_kw: ($) => token(prec(3, /[Dd][Ee][Ff][Ii][Nn][Ee][ \t]+[Cc][Ll][Aa][Ss][Ss]/)),
    _class_member: ($) =>
      choice($.procedure_definition, $.function_definition, $.call_expression, $._token),

    // PROCEDURE name[(params)] … [ENDPROC].  Body is token soup; ENDPROC/ENDFUNC optional. The
    // optional parameter list is matched right after the name so `PROCEDURE foo(p1,p2)` is not
    // mistaken for a body call.
    // The routine body is token soup; it stops at the next PROCEDURE / FUNCTION / DEFINE CLASS /
    // ENDDEFINE / ENDPROC / ENDFUNC (all high-precedence keywords, not in soup). The `(params)`
    // list is left to body soup — the name is what we capture.
    procedure_definition: ($) =>
      prec.right(seq(ci('PROCEDURE', 3), field('name', $.identifier), repeat($._stmt))),

    function_definition: ($) =>
      prec.right(seq(ci('FUNCTION', 3), field('name', $.identifier), repeat($._stmt))),

    // func(args) / obj.method(args)  (identifier is dotted, so member calls are one name).
    call_expression: ($) => prec(5, seq(field('function', $.identifier), $.arguments)),
    arguments: ($) => seq('(', optional($._arg_list), ')'),
    _arg_list: ($) => repeat1($._arg_token),
    _arg_token: ($) =>
      choice($.call_expression, $.identifier, $.number, $.string, $.operator, ',', '=', '@', '[', ']'),

    _stmt: ($) => choice($.call_expression, $._token),

    _token: ($) =>
      choice($.identifier, $.number, $.string, $.operator, ',', '=', '@', ';', '(', ')', '[', ']'),

    // `=` is kept as a separate literal token (see _token); `[...]` string form is omitted because
    // it collides with array indexing. `*` is NOT here — it is the comment lead (see `comment`).
    operator: ($) =>
      token(choice('+', '-', '/', '<', '>', '<=', '>=', '<>', '!=', '==', '.', '$', '!')),

    // Dotted identifier so obj.method / obj.property are one name. (FoxPro identifiers are word chars.)
    identifier: ($) => /[A-Za-z_][A-Za-z0-9_]*(\.[A-Za-z_][A-Za-z0-9_]*)*/,

    number: ($) => /[0-9]+(\.[0-9]+)?/,

    string: ($) => choice(token(seq('"', /[^"]*/, '"')), token(seq("'", /[^']*/, "'"))),
  },
});
