/**
 * Tree-sitter grammar for `Blastfile`, Blastit's routing table and permission
 * model.
 *
 * The grammar is a colourist, not a validator: `blastit-manifest` decides what a
 * `Blastfile` is and its LSP publishes the red. This grammar must therefore never
 * be *stricter* than the parser's language -- a file `blastit` accepts has to
 * parse here with no ERROR or MISSING node -- and it may be looser (a wrong
 * colour is cheaper than a wrong squiggle).
 *
 * Two hard rules from the accepted design (`docs/design/blastfile-lsp-zed.md`
 * sections 9.2 and 9.3):
 *  - a newline ends a statement, so newlines are NOT extras;
 *  - keywords are contextual, so a keyword is legal wherever a name is.
 */
module.exports = grammar({
  name: 'blastfile',

  // Not /\s/: a newline ends a statement. A lone \r is legal *between* tokens
  // because the parser accepts \r\n and the \r must not become its own token.
  extras: $ => [/[ \t\r]/, $.comment],
  word: $ => $.name,

  rules: {
    source_file: $ => repeat(choice($._terminator, $._section)),
    _terminator: $ => choice('\n', ';'),

    _section: $ => choice(
      $.app_decl,
      $.entrypoint_decl,
      $.store_decl,
      $.trait_decl,
      $.view_decl,
      $.static_decl,
    ),

    app_decl: $ => seq('app', $.string),
    entrypoint_decl: $ => seq('entrypoint', $.language, $.string, optional($.block)),
    language: $ => choice('python', 'php'),
    store_decl: $ => seq('postgres', optional($.name), choice($.env_var, $.block)),
    env_var: $ => /\$[A-Z_][A-Z0-9_]*/,
    trait_decl: $ => seq('trait', $.name, $.block),
    static_decl: $ => seq('static', $.string, $.block),
    // METHODS then PATTERNS, each comma-separated, the first pattern taking no
    // comma: `view products GET "/products", "/products/{id:int}" {`. `view
    // cart {` has neither, so both groups are optional at the call site (a
    // separate `_view_header` rule would match the empty string, which
    // tree-sitter refuses).
    view_decl: $ => seq('view', $.name, optional($._methods), optional($._patterns), $.block),
    _methods: $ => seq($.method, repeat(seq(',', optional($._terminator), $.method))),
    _patterns: $ => seq($._pattern, repeat(seq(',', optional($._terminator), $._pattern))),
    _pattern: $ => choice($.string, $.regex_raw_string),
    // A method is a word whose first byte is A-Z; the manifest's lexer reads
    // every such word as a method (parser.rs `is_method_word`), so a custom
    // method like `M-SEARCH` must not be an ERROR node here.
    method: $ => /[A-Z][A-Z0-9_-]*/,

    block: $ => seq('{', repeat(choice($._terminator, $._statement)), '}'),
    _statement: $ => choice(
      $.directive,
      $.endpoint,
      $.grant,
      $.use_directive,
      $.table_grant,
      $.mode,
    ),

    // Values are optional: `prefork` has none, `egress any` has one.
    directive: $ => seq($.keyword, optional($._values)),
    // A comma separates list items, but `body raw unvalidated` writes two values
    // with no comma at all, so the separator is optional -- the lexer still ends
    // the statement at a newline because a keyword only starts a new directive.
    _values: $ => seq(
      $._value,
      repeat(seq(optional(seq(',', optional($._terminator))), $._value)),
    ),
    // `immutable` is a directive and takes the same two spellings a `view`
    // pattern does -- `"..."` and `regex '...'` -- so both `_pattern` items are
    // `_value`s here and bare `immutable` is the valueless directive it already
    // was. `prec(1)` resolves the one overlap the addition creates: `regex` also
    // matches `$.name`, and the longer `regex_raw_string` must win over a name
    // followed by a raw string (the `table_grant` idiom below).
    _value: $ => choice(
      $.codec, $.env_var, $.string, prec(1, $.regex_raw_string), $.raw_string,
      $.name, $.dotted_name,
    ),
    // A function name is `Word ('.' Word)*` (docs/design/blastfile-dsl.md §6.1),
    // and `blastit fmt` writes the qualified form: `functions pg_catalog.unaccent`
    // is one function in three tokens. `repeat1`, not `repeat`: a bare name is
    // `$.name` in the choice above, and a `repeat` here would make this rule
    // match the same input and collide with it.
    dotted_name: $ => seq($.name, repeat1(seq('.', $.name))),
    use_directive: $ => seq('use', $.name, repeat(seq(',', optional($._terminator), $.name))),
    // `postgres [name] { ... }` or `postgres [name] unrestricted`.
    grant: $ => seq('postgres', optional($.name), choice($.block, alias('unrestricted', $.keyword))),
    // The two `prec(1, ...)` rules resolve the grammar's only two overlaps: a
    // bare `transactions` is a mode rather than a valueless directive, and
    // `select products` is a table grant rather than a directive with a value.
    mode: $ => prec(1, alias(choice('transactions', 'functions', 'unrestricted'), $.keyword)),
    table_grant: $ => prec(1, seq(
      alias(choice('select', 'insert', 'update', 'delete'), $.keyword),
      $._table,
      repeat(seq(',', optional($._terminator), $._table)),
    )),
    // `prec(1)` is what makes `select products` a table grant rather than a
    // directive whose value is the name `products`. Only the four grant verbs
    // can reach a table grant, so `egress any` stays a directive.
    _table: $ => prec(1, seq(
      field('table', alias($.name, $.table)),
      optional(seq(
        '(',
        field('column', alias($.name, $.column)),
        repeat(seq(',', optional($._terminator), alias($.name, $.column))),
        ')',
      )),
    )),
    endpoint: $ => seq(
      $.method,
      repeat(seq(',', optional($._terminator), $.method)),
      $._pattern,
      optional($.block),
    ),

    keyword: $ => choice(
      'transactions', 'functions', 'unrestricted', 'select', 'insert', 'update', 'delete',
      'query', 'body', 'preload', 'root', 'index', 'immutable', 'fallback', 'egress', 'any',
      'prefork', 'host', 'port', 'user', 'password', 'database', 'sslmode', 'ca_cert',
    ),
    codec: $ => choice('json', 'form', 'none', 'raw', 'unvalidated'),

    // Two string nodes: "..." with escapes, '...' verbatim and possibly multi-line.
    string: $ => seq('"', repeat(choice($.escape, /[^"\\]+/)), '"'),
    escape: $ => token.immediate(/\\(["\\nt])/),
    raw_string: $ => /'[^']*'/,

    // `regex '...'` is its own node so @string.regex cannot also fire
    // @string.special on the inner literal.
    regex_raw_string: $ => seq('regex', alias($.raw_string, $.regex_body)),

    name: $ => /[a-z0-9][a-z0-9_-]*/,
    comment: $ => /#[^\n]*/,
  },
});
