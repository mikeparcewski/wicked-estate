/**
 * Crystal Reports formula language (Crystal Syntax) — in-house symbols+calls subset.
 *
 * No tree-sitter grammar exists for the Crystal Reports formula language (the SAP "Crystal" here is
 * NOT the Crystal programming language). Authored from the SAP / i-net Crystal-Syntax references.
 * A formula is an expression fragment (no module/function blocks): the meaningful code-graph
 * signals are variable declarations (the Shared/Global cross-formula state) and references —
 * `{@OtherFormula}` formula invocations (the formula-to-formula call graph) and `Name(args)`
 * built-in/function calls. Everything else is token soup. Trust basis is the corpus parse-gate.
 *
 * Crystal Syntax specifics handled here:
 *   - keywords (scope Local/Global/Shared, the *Var type words, control keywords) are
 *     case-insensitive;
 *   - `{Table.Field}` field refs, `{@Formula}` formula refs, `{?Parameter}` parameter refs — the
 *     three brace forms are distinguished by their opener tokens (`{`, `{@`, `{?`);
 *   - `:=` is assignment, `=` comparison; `+` and `&` concatenate; strings are "…" or '…';
 *   - comments are `//` to end-of-line only (Crystal Syntax has NO block comments);
 *   - `;` separates statements; the last expression is the formula's return value.
 */

function ci(word, precedence) {
  const pattern = word
    .split('')
    .map((c) => (/[a-zA-Z]/.test(c) ? `[${c.toLowerCase()}${c.toUpperCase()}]` : c.replace(/[.*+?^${}()|[\]\\-]/g, '\\$&')))
    .join('');
  return token(prec(precedence || 0, new RegExp(pattern)));
}

module.exports = grammar({
  name: 'crystal_formula',

  extras: ($) => [/[ \t\r\n]/, $.comment],

  word: ($) => $.identifier,

  conflicts: ($) => [],

  rules: {
    source_file: ($) => repeat($._item),

    comment: ($) => token(seq('//', /[^\n]*/)),

    _item: ($) =>
      choice(
        $.variable_declaration,
        $.formula_ref,
        $.field_ref,
        $.parameter_ref,
        $.call_expression,
        $._token,
      ),

    // [Local|Global|Shared] <Type>Var [array] [range] NAME  — the declaration head; any `:= expr`
    // initialiser and the `;` fall through to soup. Default scope (no keyword) is Global.
    variable_declaration: ($) =>
      seq(
        optional($._scope),
        $._var_type,
        repeat($._var_modifier),
        field('name', $.identifier),
      ),
    _scope: ($) => choice(ci('LOCAL', 3), ci('GLOBAL', 3), ci('SHARED', 3)),
    _var_type: ($) =>
      choice(
        ci('NumberVar', 3),
        ci('CurrencyVar', 3),
        ci('StringVar', 3),
        ci('BooleanVar', 3),
        ci('DateVar', 3),
        ci('TimeVar', 3),
        ci('DateTimeVar', 3),
      ),
    _var_modifier: ($) => choice(ci('array', 3), ci('range', 3)),

    // {@FormulaName} — a reference to another formula (the formula-to-formula call edge).
    formula_ref: ($) => seq('{@', field('name', $.brace_name), '}'),
    // {?ParameterName} — a parameter reference.
    parameter_ref: ($) => seq('{?', field('name', $.brace_name), '}'),
    // {Table.Field} — a database field reference (data access).
    field_ref: ($) => seq('{', field('name', $.brace_name), '}'),
    brace_name: ($) => /[^}]+/,

    // Name(args) — built-in or user function call.
    call_expression: ($) => prec(5, seq(field('function', $.identifier), $.arguments)),
    arguments: ($) => seq('(', optional($._arg_list), ')'),
    _arg_list: ($) => repeat1($._arg_token),
    _arg_token: ($) =>
      choice(
        $.call_expression,
        $.formula_ref,
        $.field_ref,
        $.parameter_ref,
        $.identifier,
        $.number,
        $.string,
        $.operator,
        ',',
        ';',
      ),

    _token: ($) =>
      choice($.identifier, $.number, $.string, $.operator, ',', ';', ':', '(', ')', '[', ']'),

    operator: ($) =>
      token(choice(':=', '+', '-', '*', '/', '<', '>', '<=', '>=', '<>', '=', '&', '.', '%')),

    identifier: ($) => /[A-Za-z_][A-Za-z0-9_]*/,

    number: ($) => /[0-9]+(\.[0-9]+)?/,

    string: ($) => choice(token(seq('"', /[^"]*/, '"')), token(seq("'", /[^']*/, "'"))),
  },
});
