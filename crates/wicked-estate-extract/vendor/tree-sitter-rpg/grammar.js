/**
 * Free-format RPG IV (ILE RPG) tree-sitter grammar — symbols + calls subset for code_intel.
 *
 * Authored via the template-extrapolate method (structure borrowed from existing tree-sitter
 * grammars; rules written for RPG free-form syntax) and validated NOT by upstream pedigree but by
 * a corpus parse-gate (zero ERROR/MISSING nodes across real sample files) + extraction-count
 * assertions. This is intentionally NOT a full-language grammar: it delimits declarations
 * (dcl-proc / dcl-pi / dcl-s / dcl-c / dcl-f / dcl-ds), captures call expressions, and parses
 * statement bodies loosely enough that real free-format source yields a clean tree.
 *
 * Fixed-format RPG (column 6 spec types: H/F/D/C/P, positional fields) is column-oriented and is
 * handled by a separate line extractor, not this grammar.
 *
 * RPG is case-insensitive — keywords are matched via the `ci()` char-class helper.
 */

// Case-insensitive keyword token: ci('dcl-proc') matches DCL-PROC, Dcl-Proc, dcl-proc, ...
function ci(word) {
  return token(
    new RegExp(
      word
        .split('')
        .map((c) =>
          /[a-zA-Z]/.test(c)
            ? `[${c.toLowerCase()}${c.toUpperCase()}]`
            : c.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'),
        )
        .join(''),
    ),
  );
}

module.exports = grammar({
  name: 'rpg',

  extras: ($) => [/\s/, $.comment, $.directive],

  word: ($) => $.identifier,

  conflicts: ($) => [],

  rules: {
    source_file: ($) => repeat($._statement),

    // Free-format line comment.
    comment: ($) => token(seq('//', /[^\n]*/)),

    // Compiler directives (line-oriented): **FREE, /free, /end-free, /copy, /include, /if, etc.
    // Listed explicitly (not `/<word>`) so the division operator `a / b` is never mis-lexed.
    directive: ($) =>
      token(
        choice(
          /\*\*[fF][rR][eE][eE][^\n]*/,
          seq(
            '/',
            choice(
              ci('free'),
              ci('end-free'),
              ci('copy'),
              ci('include'),
              ci('if'),
              ci('elseif'),
              ci('else'),
              ci('endif'),
              ci('eof'),
              ci('define'),
              ci('undefine'),
              ci('set'),
              ci('restore'),
              ci('space'),
              ci('title'),
            ),
            /[^\n]*/,
          ),
        ),
      ),

    _statement: ($) =>
      choice(
        $.ctl_opt,
        $.file_decl,
        $.var_decl,
        $.const_decl,
        $.ds_decl,
        $.proc_decl,
        $.pi_decl,
        $.if_statement,
        $.dou_statement,
        $.dow_statement,
        $.for_statement,
        $.select_statement,
        $.monitor_statement,
        $.return_statement,
        $.exec_statement,
        $.assignment,
        $.expression_statement,
      ),

    ctl_opt: ($) => seq(ci('ctl-opt'), repeat($._kw_token), ';'),
    file_decl: ($) => seq(ci('dcl-f'), field('name', $.identifier), repeat($._kw_token), ';'),
    var_decl: ($) => seq(ci('dcl-s'), field('name', $.identifier), repeat($._kw_token), ';'),
    const_decl: ($) => seq(ci('dcl-c'), field('name', $.identifier), repeat($._kw_token), ';'),

    ds_decl: ($) =>
      seq(
        ci('dcl-ds'),
        field('name', $.identifier),
        repeat($._kw_token),
        ';',
        repeat($.subfield_decl),
        ci('end-ds'),
        optional($.identifier),
        ';',
      ),
    subfield_decl: ($) => seq(field('name', $.identifier), repeat($._kw_token), ';'),

    proc_decl: ($) =>
      seq(
        ci('dcl-proc'),
        field('name', $.identifier),
        repeat($._kw_token),
        ';',
        repeat($._statement),
        ci('end-proc'),
        optional($.identifier),
        ';',
      ),

    pi_decl: ($) =>
      seq(
        ci('dcl-pi'),
        field('name', choice($.identifier, $.star_name)),
        repeat($._kw_token),
        ';',
        repeat($.parm_decl),
        ci('end-pi'),
        ';',
      ),
    parm_decl: ($) => seq(field('name', $.identifier), repeat($._kw_token), ';'),

    if_statement: ($) =>
      seq(
        ci('if'),
        $._expression,
        ';',
        repeat($._statement),
        repeat($.elseif_clause),
        optional($.else_clause),
        ci('endif'),
        ';',
      ),
    elseif_clause: ($) =>
      seq(choice(ci('elseif'), ci('elsif')), $._expression, ';', repeat($._statement)),
    else_clause: ($) => seq(ci('else'), ';', repeat($._statement)),

    dou_statement: ($) =>
      seq(ci('dou'), $._expression, ';', repeat($._statement), ci('enddo'), ';'),
    dow_statement: ($) =>
      seq(ci('dow'), $._expression, ';', repeat($._statement), ci('enddo'), ';'),
    for_statement: ($) =>
      seq(
        ci('for'),
        field('var', $.identifier),
        '=',
        $._expression,
        choice(ci('to'), ci('downto')),
        $._expression,
        optional(seq(ci('by'), $._expression)),
        ';',
        repeat($._statement),
        ci('endfor'),
        ';',
      ),

    select_statement: ($) =>
      seq(
        ci('select'),
        ';',
        repeat($.when_clause),
        optional($.other_clause),
        ci('endsl'),
        ';',
      ),
    when_clause: ($) => seq(ci('when'), $._expression, ';', repeat($._statement)),
    other_clause: ($) => seq(ci('other'), ';', repeat($._statement)),

    monitor_statement: ($) =>
      seq(
        ci('monitor'),
        ';',
        repeat($._statement),
        repeat($.on_error_clause),
        ci('endmon'),
        ';',
      ),
    on_error_clause: ($) => seq(ci('on-error'), repeat($._kw_token), ';', repeat($._statement)),

    return_statement: ($) => seq(ci('return'), optional($._expression), ';'),

    // callp NAME(args);  — explicit procedure call op.
    exec_statement: ($) => seq(ci('callp'), $._expression, ';'),

    expression_statement: ($) => seq($._expression, ';'),

    assignment: ($) =>
      prec.right(
        1,
        seq(field('target', $._primary), choice('=', '+=', '-=', '*=', '/='), $._expression, ';'),
      ),

    _expression: ($) => choice($.binary_expression, $.unary_expression, $._primary),

    _primary: ($) =>
      choice(
        $.call_expression,
        $.builtin,
        $.paren_expression,
        $.identifier,
        $.number,
        $.string,
        $.star_name,
      ),

    call_expression: ($) =>
      prec(
        5,
        seq(field('function', $.identifier), '(', optional($._arg_list), ')'),
      ),
    builtin: ($) =>
      prec(5, seq(field('name', $.builtin_name), '(', optional($._arg_list), ')')),
    builtin_name: ($) => token(/%[a-zA-Z][a-zA-Z0-9]*/),

    _arg_list: ($) => seq($._expression, repeat(seq(choice(':', ','), $._expression))),

    binary_expression: ($) =>
      prec.left(
        2,
        seq($._expression, $._binop, $._expression),
      ),
    _binop: ($) =>
      choice('+', '-', '*', '/', '**', '=', '<>', '<', '>', '<=', '>=', ci('and'), ci('or')),

    unary_expression: ($) => prec(4, seq(choice('-', ci('not')), $._expression)),

    paren_expression: ($) => seq('(', $._expression, ')'),

    // Tokens permitted inside declaration keyword lists: packed(11:2), inz(*blanks), dim(10),
    // const, export, likeds(x), etc. Calls/builtins/parens cover the parenthesized keyword args.
    _kw_token: ($) =>
      choice(
        $.builtin,
        $.keyword_arg,
        $.paren_expression,
        $.identifier,
        $.number,
        $.string,
        $.star_name,
        ':',
      ),

    // A type/keyword spec in a declaration: packed(11:2), char(50), dim(10), likeds(x).
    // Structurally an `ident(args)` like a call, but it is NOT a procedure call — keeping it a
    // distinct node means the `call_expression` query captures ONLY real calls (no false edges).
    keyword_arg: ($) =>
      prec(6, seq(field('keyword', $.identifier), '(', optional($._arg_list), ')')),

    star_name: ($) => token(choice('*n', '*N', /\*[a-zA-Z][a-zA-Z0-9]*/)),

    identifier: ($) => /[A-Za-z@#$][A-Za-z0-9@#$_]*/,
    number: ($) => /[0-9]+(\.[0-9]+)?/,
    string: ($) =>
      choice(token(seq("'", /[^']*/, "'")), token(seq('"', /[^"]*/, '"'))),
  },
});
