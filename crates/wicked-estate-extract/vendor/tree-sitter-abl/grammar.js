/**
 * Progress OpenEdge ABL (Advanced Business Language / 4GL) — in-house symbols+calls subset.
 *
 * Authored via the template-extrapolate method (the comprehensive usagi-coffee/tree-sitter-abl
 * grammar is ~97MB of generated parser.c — far too large to vendor near GitHub's 100MiB limit —
 * so this is a deliberately minimal grammar that delimits the constructs a code graph needs:
 * CLASS / INTERFACE / METHOD / CONSTRUCTOR / DESTRUCTOR / FUNCTION / PROCEDURE definitions, RUN
 * statements, and parenthesised function calls. Statement bodies are parsed loosely (token soup)
 * so real source yields a clean tree. Trust basis is the corpus parse-gate + extraction-count
 * test in wicked-estate-extract, exactly like the in-house RPG grammar.
 *
 * ABL specifics handled here:
 *   - keywords are case-insensitive (ci helper);
 *   - identifiers may contain hyphens (NO-UNDO, DYNAMIC-FUNCTION) and the unified identifier
 *     token also spans dotted qualified names (Progress.Lang.Object) so the statement terminator
 *     '.' is always a standalone token;
 *   - compound blocks open with ':' and close with 'END [keyword].';
 *   - structural keywords carry high lexical precedence so a token-soup body never swallows END.
 */

// Case-insensitive keyword token builder. `precedence` lifts structural keywords above identifier
// in the lexer so a token-soup body never mis-lexes END/CLASS/… as an identifier.
function ci(word, precedence) {
  const pattern = word
    .split('')
    .map((c) => (/[a-zA-Z]/.test(c) ? `[${c.toLowerCase()}${c.toUpperCase()}]` : c.replace(/[.*+?^${}()|[\]\\-]/g, '\\$&')))
    .join('');
  return token(prec(precedence || 0, new RegExp(pattern)));
}

module.exports = grammar({
  name: 'abl',

  extras: ($) => [/\s/, $.comment],

  word: ($) => $.identifier,

  conflicts: ($) => [],

  rules: {
    source_file: ($) => repeat($._item),

    // ABL block comment /* */ (approximate — does not model nesting) + newer // line comment.
    comment: ($) =>
      token(
        choice(
          seq('//', /[^\n]*/),
          seq('/*', /[^*]*\*+([^/*][^*]*\*+)*/, '/'),
        ),
      ),

    _item: ($) =>
      choice(
        $.class_definition,
        $.interface_definition,
        $.method_definition,
        $.constructor_definition,
        $.destructor_definition,
        $.function_definition,
        $.procedure_definition,
        $.run_statement,
        $.call_expression,
        $._token,
      ),

    // ── Compound block: ':' opens, 'END [closer] .' closes ──────────────────────
    _block: ($) =>
      seq(
        ':',
        repeat($._item),
        ci('END', 3),
        optional($._block_closer),
        '.',
      ),
    _block_closer: ($) =>
      choice(
        ci('CLASS', 3),
        ci('INTERFACE', 3),
        ci('METHOD', 3),
        ci('CONSTRUCTOR', 3),
        ci('DESTRUCTOR', 3),
        ci('FUNCTION', 3),
        ci('PROCEDURE', 3),
        ci('ENUM', 3),
        ci('GET', 3),
        ci('SET', 3),
      ),

    _modifier: ($) =>
      choice(
        ci('PUBLIC'),
        ci('PRIVATE'),
        ci('PROTECTED'),
        ci('STATIC'),
        ci('ABSTRACT'),
        ci('OVERRIDE'),
        ci('FINAL'),
        ci('SERIALIZABLE'),
      ),

    // ── Definitions ─────────────────────────────────────────────────────────────
    // CLASS qualname [INHERITS x] [IMPLEMENTS y,z] [options] : ... END [CLASS].
    // (Class options such as ABSTRACT/FINAL/SERIALIZABLE follow the name → _header_token.)
    class_definition: ($) =>
      seq(ci('CLASS', 3), field('name', $.identifier), repeat($._header_token), $._block),

    interface_definition: ($) =>
      seq(ci('INTERFACE', 3), field('name', $.identifier), repeat($._header_token), $._block),

    // METHOD [modifier]* returntype name (params) : ... END [METHOD].   (abstract: ends with '.')
    method_definition: ($) =>
      seq(
        ci('METHOD', 3),
        repeat($._modifier),
        field('return_type', $.identifier),
        field('name', $.identifier),
        $.parameters,
        choice($._block, '.'),
      ),

    constructor_definition: ($) =>
      seq(
        ci('CONSTRUCTOR', 3),
        repeat($._modifier),
        field('name', $.identifier),
        $.parameters,
        $._block,
      ),

    destructor_definition: ($) =>
      seq(
        ci('DESTRUCTOR', 3),
        repeat($._modifier),
        field('name', $.identifier),
        $.parameters,
        $._block,
      ),

    // FUNCTION name RETURNS type [access] [(params)] : ... END [FUNCTION].  (forward: ends '.')
    function_definition: ($) =>
      seq(
        ci('FUNCTION', 3),
        field('name', $.identifier),
        repeat($._header_token),
        choice($._block, '.'),
      ),

    // PROCEDURE name [options] : ... END [PROCEDURE].
    procedure_definition: ($) =>
      seq(ci('PROCEDURE', 3), field('name', $.identifier), repeat($._header_token), $._block),

    // RUN target — target is a program name/handle or a quoted path. Trailing args (IN h, params,
    // NO-ERROR, '.') fall through to token-soup; the captured target is the call edge we need.
    run_statement: ($) => seq(ci('RUN', 3), field('procedure', choice($.identifier, $.string))),

    // ── Calls ─────────────────────────────────────────────────────────────────────
    // name(args) — also catches obj:method(args) in token soup (the method name is captured).
    call_expression: ($) =>
      prec(5, seq(field('function', $.identifier), '(', optional($._arg_list), ')')),
    // Arg tokens exclude bare parens so the closing ')' is unambiguous; nested calls recurse.
    _arg_list: ($) => repeat1($._arg_token),
    _arg_token: ($) =>
      choice(
        $.call_expression,
        $.identifier,
        $.number,
        $.string,
        $._modifier,
        $.operator,
        ':',
        '.',
        ',',
        '=',
        '[',
        ']',
      ),

    parameters: ($) => seq('(', repeat($._param_token), ')'),
    _param_token: ($) =>
      choice(
        $.identifier,
        $.number,
        $.string,
        $._modifier,
        $.operator,
        ',',
        ':',
        '=',
        '[',
        ']',
        $.call_expression,
      ),

    // Header tokens: everything in a definition header before the ':' block-opener (or '.').
    // Excludes ':' (block open) and '.' (terminator); includes parens so INHERITS/param lists pass.
    _header_token: ($) =>
      choice($.identifier, $.number, $.string, $._modifier, $.parameters, $.operator, ',', '=', '[', ']'),

    // Catch-all body token: anything that is not a structural boundary.
    _token: ($) =>
      choice(
        $.identifier,
        $.number,
        $.string,
        $._modifier,
        $.operator,
        ':',
        '.',
        ',',
        '=',
        '(',
        ')',
        '[',
        ']',
      ),

    // ABL symbolic operators (logical/comparison word-operators AND/OR/EQ/… lex as identifiers).
    operator: ($) => token(choice('+', '-', '*', '/', '<', '>', '<=', '>=', '<>', '^')),

    // Unified identifier: hyphen-bearing (NO-UNDO) and dotted qualified names (Progress.Lang.Object).
    // Because dotted segments must be followed by a word char, the terminator '.' stays standalone.
    identifier: ($) => /[A-Za-z_#$%][A-Za-z0-9_#$%-]*(\.[A-Za-z_#$%][A-Za-z0-9_#$%-]*)*/,

    number: ($) => /[0-9]+(\.[0-9]+)?/,

    string: ($) => choice(token(seq('"', /[^"]*/, '"')), token(seq("'", /[^']*/, "'"))),
  },
});
