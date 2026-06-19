/**
 * Minimal nginx configuration grammar — EXAMPLE wicked-estate language plugin.
 *
 * This is a deliberately small grammar (blocks + directives + comments) that exists to demonstrate
 * the runtime plugin mechanism end-to-end, NOT to be a complete nginx grammar. It is built into a
 * shared library and dropped into the wicked-estate plugins directory — it is never compiled into
 * the wicked-estate core, which is exactly what lets a plugin carry its own (here: Apache-2.0)
 * license without touching the MIT core.
 */
module.exports = grammar({
  name: 'nginx',

  extras: ($) => [/\s/, $.comment],

  rules: {
    config: ($) => repeat($._item),

    comment: ($) => token(seq('#', /[^\n]*/)),

    _item: ($) => choice($.block, $.directive),

    // `name args… { … }` — e.g. `server { … }`, `location /api { … }`, `upstream backend { … }`.
    block: ($) =>
      seq(field('name', $.identifier), repeat($._value), '{', repeat($._item), '}'),

    // `name args… ;` — e.g. `listen 80;`, `proxy_pass http://backend;`.
    directive: ($) => seq(field('name', $.identifier), repeat($._value), ';'),

    // The block/directive name is a strict identifier; arguments are broad `word`s (paths, IPs,
    // URLs, mime types, $variables, sizes) — any run of non-whitespace, non-delimiter characters.
    _value: ($) => choice($.string, $.word),

    string: ($) =>
      choice(token(seq('"', /[^"]*/, '"')), token(seq("'", /[^']*/, "'"))),

    word: ($) => token(/[^\s;{}#"']+/),

    identifier: ($) => token(/[A-Za-z_][A-Za-z0-9_]*/),
  },
});
