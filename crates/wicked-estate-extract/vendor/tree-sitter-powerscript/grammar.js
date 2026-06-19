/**
 * PowerBuilder PowerScript (.srw/.sru/.srf/.sra/.srm exports) — in-house symbols+calls subset.
 *
 * No tree-sitter grammar exists (the canonical reference is the ANTLR grammars-v4 PowerBuilder
 * grammar + the PowerScript Reference). Authored via the template-extrapolate method: a minimal
 * grammar delimiting `type … end type` object declarations, function/subroutine/event bodies, and
 * `on … end on` blocks, and capturing function-call-syntax calls. Other content (the export
 * header, `forward`/`prototypes`/`variables` blocks, statement bodies) is token soup. Trust basis
 * is the corpus parse-gate + extraction-count test (RPG/ABL contract).
 *
 * PowerScript specifics handled here:
 *   - keywords are case-insensitive;
 *   - blocks close with two-word `end type` / `end function` / `end subroutine` / `end event` /
 *     `end on` / `end forward` / `end prototypes` / `end variables` — matched as single multi-word
 *     tokens so a token-soup body never disambiguates a bare `end`; inner `end if` / `end choose`
 *     / `end while` / `end for` are soup tokens;
 *   - `//` line and slash-star block comments;
 *   - `&` line continuation and `~` string escapes are just soup characters here;
 *   - the `$PBExportHeader$…` / `HA$PBExportHeader$…` first line(s) are soup.
 */

function ci(word, precedence) {
  const pattern = word
    .split('')
    .map((c) => (/[a-zA-Z]/.test(c) ? `[${c.toLowerCase()}${c.toUpperCase()}]` : c.replace(/[.*+?^${}()|[\]\\-]/g, '\\$&')))
    .join('');
  return token(prec(precedence || 0, new RegExp(pattern)));
}

// Multi-word "end X" token at high precedence.
function endkw(word) {
  const tail = word
    .split('')
    .map((c) => `[${c.toLowerCase()}${c.toUpperCase()}]`)
    .join('');
  return token(prec(4, new RegExp('[Ee][Nn][Dd][ \\t]+' + tail)));
}

module.exports = grammar({
  name: 'powerscript',

  extras: ($) => [/[ \t\r\n]/, $.comment],

  word: ($) => $.identifier,

  conflicts: ($) => [],

  rules: {
    source_file: ($) => repeat($._item),

    comment: ($) =>
      token(choice(seq('//', /[^\n]*/), seq('/*', /[^*]*\*+([^/*][^*]*\*+)*/, '/'))),

    _item: ($) =>
      choice(
        $.type_definition,
        $.prototypes_block,
        $.function_body,
        $.subroutine_body,
        $.event_body,
        $.on_body,
        $.block_end,
        $.call_expression,
        $._token,
      ),

    // `type prototypes` / `forward prototypes` … end prototypes. The interior holds function/
    // subroutine PROTOTYPE declarations (no body, no `end function`), so it is a distinct soup
    // region whose tokens absorb the FUNCTION/SUBROUTINE keywords — keeping them from starting a
    // function_body that would otherwise swallow content up to the next real `end function`.
    prototypes_block: ($) =>
      seq($._prototypes_opener, repeat($._proto_token), endkw('prototypes')),
    _prototypes_opener: ($) =>
      token(
        prec(
          4,
          /([Tt][Yy][Pp][Ee]|[Ff][Oo][Rr][Ww][Aa][Rr][Dd])[ \t]+[Pp][Rr][Oo][Tt][Oo][Tt][Yy][Pp][Ee][Ss]/,
        ),
      ),
    _proto_token: ($) =>
      choice(
        $.identifier,
        $.number,
        $.string,
        $.operator,
        ci('FUNCTION', 3),
        ci('SUBROUTINE', 3),
        ci('EVENT', 3),
        ',',
        '=',
        ';',
        ':',
        '(',
        ')',
        '[',
        ']',
      ),

    // [global|shared] type NAME [from ANCESTOR] [within X] [autoinstantiate] … end type.
    // (Scope words like `global`/`shared` precede this as soup items.) Captures name + ancestor.
    type_definition: ($) =>
      seq(
        ci('TYPE', 3),
        field('name', $.identifier),
        optional(seq(ci('FROM', 3), field('ancestor', $.identifier))),
        repeat($._stmt),
        endkw('type'),
      ),

    // [access] [scope] function <rettype> <name>(params) … end function.  Access/scope words
    // precede `function` as soup. rettype is the id after FUNCTION, name the next id.
    function_body: ($) =>
      seq(
        ci('FUNCTION', 3),
        field('rettype', $.identifier),
        field('name', $.identifier),
        repeat($._stmt),
        endkw('function'),
      ),

    subroutine_body: ($) =>
      seq(ci('SUBROUTINE', 3), field('name', $.identifier), repeat($._stmt), endkw('subroutine')),

    // event [type <rettype>] <name>(params) ; … end event.
    event_body: ($) =>
      seq(
        ci('EVENT', 3),
        optional(seq(ci('TYPE', 3), field('rettype', $.identifier))),
        field('name', $.identifier),
        repeat($._stmt),
        endkw('event'),
      ),

    // on <object>.create | <object>.destroy … end on.
    on_body: ($) => seq(ci('ON', 3), field('name', $.identifier), repeat($._stmt), endkw('on')),

    // `end forward` / `end variables` consumed as no-op items (their interiors — type decls and
    // variable decls — are plain soup). `end prototypes` is handled inside prototypes_block.
    block_end: ($) => choice(endkw('forward'), endkw('variables')),

    // func(args) / obj.method(args) (dotted identifier → member call captured as one name).
    call_expression: ($) => prec(5, seq(field('function', $.identifier), $.arguments)),
    arguments: ($) => seq('(', optional($._arg_list), ')'),
    _arg_list: ($) => repeat1($._arg_token),
    _arg_token: ($) =>
      choice($.call_expression, $.identifier, $.number, $.string, $.operator, ',', '=', '[', ']'),

    _stmt: ($) => choice($.call_expression, $._token),

    _token: ($) =>
      choice(
        $.identifier,
        $.number,
        $.string,
        $._end_inner,
        $._type_block_opener,
        $.operator,
        ',',
        '=',
        ';',
        ':',
        '(',
        ')',
        '[',
        ']',
      ),

    // `type variables` opens a soup block (closed by `end variables`). Matched as a single
    // high-precedence token so the leading `type` does NOT start a type_definition. (`type
    // prototypes` is handled by prototypes_block via _prototypes_opener.)
    _type_block_opener: ($) =>
      token(prec(4, /[Tt][Yy][Pp][Ee][ \t]+[Vv][Aa][Rr][Ii][Aa][Bb][Ll][Ee][Ss]/)),

    // Inner control-flow closers, consumed as body soup (NOT definition closers).
    _end_inner: ($) =>
      choice(endkw('if'), endkw('choose'), endkw('while'), endkw('for'), endkw('try'), endkw('case')),

    operator: ($) =>
      token(choice('+', '-', '*', '/', '<', '>', '<=', '>=', '<>', '^', '&', '.', '::', '$')),

    // Dotted identifier so obj.method / object.create are one name. `$` covers $PBExportHeader$ runs.
    identifier: ($) => /[A-Za-z_$][A-Za-z0-9_$]*(\.[A-Za-z_][A-Za-z0-9_]*)*/,

    number: ($) => /[0-9]+(\.[0-9]+)?/,

    // "…" and '…' strings; `~` escapes are consumed within (no special handling needed for soup).
    string: ($) => choice(token(seq('"', /([^"~]|~.)*/, '"')), token(seq("'", /([^'~]|~.)*/, "'"))),
  },
});
