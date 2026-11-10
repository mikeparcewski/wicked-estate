/**
 * tree-sitter grammar for Oracle APEXlang (`.apx`), the core syntax only.
 *
 * Derived from the "Core layout and values" section of Oracle's `apexlang.ebnf`
 * (oracle/skills@014cdb29f12dfa6a689bbe8a80ee1c5f0bf15be1,
 * apex/apexlang/26.1/apexlang-generation/assets/grammar/apexlang.ebnf, UPL-1.0) and checked
 * against Oracle's generated applications (oracle/apex branch 26.2, UPL-1.0). The component and
 * property schema (the rest of the EBNF) is deliberately NOT encoded: SQLcl `apex validate` owns it.
 *
 * Newlines are significant. Indentation is not. A fenced block's body is one external token
 * (src/scanner.c): everything from the line after the opening fence up to the start of the line
 * whose trimmed text is the closing fence.
 */

module.exports = grammar({
  name: 'apexlang',

  extras: $ => [/[ \t]/, $.comment],

  // `_error_sentinel` is never valid in a parse state: when the parser offers it, it is in error
  // recovery (every external token is offered) and the scanner declines.
  externals: $ => [$.fence_body, $._error_sentinel],

  rules: {
    source_file: $ => repeat(choice($._nl, $.component)),

    // <kind> [<id>] ( ... )
    component: $ => seq(
      field('kind', $.identifier),
      optional(field('id', choice($.component_id, $.string))),
      '(',
      $._nl,
      repeat(choice($._nl, seq($._body_item, $._nl))),
      ')',
    ),

    _body_item: $ => choice($.component, $.group, $.property),

    // <name> { ... }
    group: $ => seq(
      field('name', $.identifier),
      '{',
      $._nl,
      repeat(choice($._nl, seq($._body_item, $._nl))),
      '}',
    ),

    // key: value | key: [ ... ] | key: { ... } | key: <newline> ```tag ... ```
    property: $ => seq(
      field('key', choice($.identifier, $.string)),
      ':',
      optional(field('value', choice(
        $.reference,
        $.string,
        $.text,
        $.array,
        $.object,
        $.fenced_block,
      ))),
    ),

    object: $ => seq(
      '{',
      $._nl,
      repeat(choice($._nl, seq($._body_item, $._nl))),
      '}',
    ),

    array: $ => choice(
      // inline: [ a b c ]
      seq('[', repeat($._array_value), ']'),
      // one or more values per line
      seq('[', $._nl, repeat(choice($._nl, seq(repeat1($._array_value), $._nl))), ']'),
    ),

    _array_value: $ => choice($.reference, $.string, $.word),

    // The opening fence sits on the line after `key:`. Lexing the newline, the indentation and the
    // backticks as ONE token keeps the grammar LR(1): after `key:` the next token is either this
    // or the newline that ends an empty property.
    fenced_block: $ => seq(
      alias($._fence_open, '```'),
      optional(field('language', $.fence_language)),
      $._nl,
      optional(field('body', $.fence_body)),
      '```',
    ),

    _fence_open: _ => token(seq(/\r?\n/, /[ \t]*/, '```')),

    fence_language: _ => token.immediate(/[A-Za-z0-9_+#.-]+/),

    // Names: component kinds, group names, property keys.
    identifier: _ => /[A-Za-z0-9_$][A-Za-z0-9_.$-]*/,

    // Component ids: identifiers, numbers, 'quoted' ids, APEX$ROW_ACTION, template placeholders
    // such as @APP_ALIAS@ (oracle/skills example-0).
    component_id: _ => /[^\s(){}:"\[\]][^\s()]*/,

    // `@` never starts `text` or `word`, so a malformed reference is an ERROR, not a silently
    // dropped edge.
    reference: _ => /@[^\s]+/,

    // `string` and `text` share a precedence so the LONGER match wins (`"ID" = :P1_ID` is text);
    // on a tie (`"ID"`), `string` wins because it is declared first.
    string: _ => /"([^"\\\r\n]|\\.)*"/,

    // Rest-of-line value without trailing blanks. Never starts with `@` (a reference) or `[` (an
    // array), so neither can degrade into text.
    text: _ => /[^\s@\[]([^\r\n]*[^\s])?/,

    // One whitespace-separated array value.
    word: _ => /[^\s@"\[\]][^\s\]]*/,

    comment: _ => token(prec(-2, choice(
      seq('//', /[^\r\n]*/),
      seq('/*', /[^*]*\*+([^/*][^*]*\*+)*/, '/'),
    ))),

    _nl: _ => /\r?\n/,
  },
});
