/**
 * LotusScript (IBM Lotus Notes / Domino) — in-house symbols+calls subset.
 *
 * No upstream tree-sitter grammar exists for LotusScript. Authored via the template-extrapolate
 * method; LotusScript is a BASIC/VBA dialect. This is a deliberately minimal grammar that delimits
 * Class / Sub / Function / Property / Type definitions and captures Call statements and function
 * calls. Bodies are parsed as token soup so real source yields a clean tree. Trust basis is the
 * corpus parse-gate + extraction-count test in wicked-estate-extract (same contract as RPG/ABL).
 *
 * LotusScript specifics handled here:
 *   - keywords are case-insensitive; statements are newline-terminated but newlines are NOT
 *     significant here (bodies are token soup, so blocks are delimited by `End X` keywords only);
 *   - `End Sub` / `End Function` / … are matched as single multi-word tokens, so a body never has
 *     to disambiguate a bare `End`; inner `End If` / `End Select` / … are soup tokens;
 *   - three string-literal forms: "…", |…|, {…};
 *   - comments: `'` to end-of-line and `%REM … %END REM` blocks;
 *   - identifiers may be dotted (obj.method) so member calls are captured as one name.
 */

// Case-insensitive keyword token (optionally at elevated lexical precedence).
function ci(word, precedence) {
  const pattern = word
    .split('')
    .map((c) => (/[a-zA-Z]/.test(c) ? `[${c.toLowerCase()}${c.toUpperCase()}]` : c.replace(/[.*+?^${}()|[\]\\-]/g, '\\$&')))
    .join('');
  return token(prec(precedence || 0, new RegExp(pattern)));
}

// Multi-word "End X" token (e.g. End Sub) at high precedence so it always wins over identifier.
function endkw(word) {
  const tail = word
    .split('')
    .map((c) => `[${c.toLowerCase()}${c.toUpperCase()}]`)
    .join('');
  return token(prec(4, new RegExp('[Ee][Nn][Dd][ \\t]+' + tail)));
}

module.exports = grammar({
  name: 'lotusscript',

  extras: ($) => [/\s/, $.comment],

  word: ($) => $.identifier,

  conflicts: ($) => [],

  rules: {
    source_file: ($) => repeat($._item),

    // `'` line comment and %REM … %END REM block. The block content is matched as [^%]* (it stops
    // at the first '%', i.e. the closing %END) — a %REM block containing a literal '%' is a known
    // minor limitation, acceptable for a symbols+calls grammar.
    comment: ($) =>
      token(
        choice(
          seq("'", /[^\n]*/),
          seq(/%[rR][eE][mM]/, /[^%]*/, /%[eE][nN][dD][ \t]+[rR][eE][mM]/),
        ),
      ),

    _item: ($) =>
      choice(
        $.class_definition,
        $.sub_definition,
        $.function_definition,
        $.property_definition,
        $.type_definition,
        $.call_statement,
        $.call_expression,
        $._token,
      ),

    // Class … End Class — may carry member subs/functions/properties.
    // `As Base` (inheritance) is left to body soup — the class name is what we capture.
    class_definition: ($) =>
      seq(
        ci('CLASS', 3),
        field('name', $.identifier),
        repeat($._class_item),
        endkw('Class'),
      ),
    _class_item: ($) =>
      choice(
        $.sub_definition,
        $.function_definition,
        $.property_definition,
        $.call_statement,
        $.call_expression,
        $._token,
      ),

    // Sub name(args) … End Sub.  Name is the first identifier after the keyword; the rest of the
    // signature (params) and the body are token soup.
    sub_definition: ($) =>
      seq(
        ci('SUB', 3),
        field('name', $.identifier),
        repeat($._stmt),
        endkw('Sub'),
      ),

    function_definition: ($) =>
      seq(
        ci('FUNCTION', 3),
        field('name', $.identifier),
        repeat($._stmt),
        endkw('Function'),
      ),

    // Property Get/Set name … End Property.  GET/SET stay plain identifiers (so body `Set x = …`
    // is unaffected); 'kind' is the identifier after PROPERTY, 'name' the one after that.
    property_definition: ($) =>
      seq(
        ci('PROPERTY', 3),
        field('kind', $.identifier),
        field('name', $.identifier),
        repeat($._stmt),
        endkw('Property'),
      ),

    type_definition: ($) =>
      seq(
        ci('TYPE', 3),
        field('name', $.identifier),
        repeat($._stmt),
        endkw('Type'),
      ),

    // Call target(args)  — explicit Call statement. prec.right so a following '(' attaches as the
    // call's arguments rather than starting a new soup statement (no significant newlines here).
    call_statement: ($) =>
      prec.right(seq(ci('CALL', 3), field('function', $.identifier), optional($.arguments))),

    // name(args) — function/method call expression.
    call_expression: ($) => prec(5, seq(field('function', $.identifier), $.arguments)),
    arguments: ($) => seq('(', optional($._arg_list), ')'),
    _arg_list: ($) => repeat1($._arg_token),
    _arg_token: ($) =>
      choice($.call_expression, $.identifier, $.number, $.string, $.operator, ':', ',', '=', '[', ']'),

    // Body statement soup.
    _stmt: ($) => choice($.call_statement, $.call_expression, $._token),

    _token: ($) =>
      choice(
        $.identifier,
        $.number,
        $.string,
        $._end_inner,
        $.operator,
        $.directive,
        ':',
        ',',
        '=',
        '(',
        ')',
        '[',
        ']',
      ),

    // Inner block closers consumed as body soup (NOT definition closers).
    _end_inner: ($) =>
      choice(
        endkw('If'),
        endkw('Select'),
        endkw('With'),
        endkw('ForAll'),
        endkw('Do'),
      ),

    operator: ($) => token(choice('+', '-', '*', '/', '<', '>', '<=', '>=', '<>', '&', '.', '_', '\\')),

    // %INCLUDE, %If, … preprocessor directives (kept simple — %REM blocks are comments above).
    directive: ($) => token(/%[A-Za-z][A-Za-z]*/),

    // Dotted identifier so member calls (obj.method) are captured as one name. Suffix chars %&!#@$.
    identifier: ($) => /[A-Za-z_][A-Za-z0-9_]*[%&!#@$]?(\.[A-Za-z_][A-Za-z0-9_]*[%&!#@$]?)*/,

    number: ($) => /[0-9]+(\.[0-9]+)?/,

    // "…", |…|, and {…} string forms.
    string: ($) =>
      choice(
        token(seq('"', /[^"]*/, '"')),
        token(seq('|', /[^|]*/, '|')),
        token(seq('{', /[^}]*/, '}')),
      ),
  },
});
