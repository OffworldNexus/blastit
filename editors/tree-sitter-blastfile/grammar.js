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
// One case-insensitive spelling per SQL keyword: `ci('SELECT')` is
// `[Ss][Ee][Ll][Ee][Cc][Tt]`, i.e. all 2^6 spellings of the word as one token.
// tree-sitter's regex dialect has no `i` flag, so the class is built from the
// word itself (docs/design/blastfile-lsp-zed.md §9.2a decision 3).
const ci = (word) =>
  new RegExp(
    word
      .split('')
      .map((c) => `[${c.toUpperCase()}${c.toLowerCase()}]`)
      .join(''),
  );

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
      $.query_decl, // NEW (MB.34.6): `query` owns its value now, so it is a rule of its own
      $.directive,
      $.endpoint,
      $.grant,
      $.use_directive,
      $.table_grant,
      $.mode,
    ),

    // `query '…'` is the one directive whose value is SQL, and it is the one
    // directive that does *not* go through the shared `keyword` rule: with
    // `'query'` in `keyword` the state after `query` is shared with every other
    // directive, `raw_string` (one token) is a valid token there, and the
    // longest-match lexer always beat `sql_string`'s opening quote, so no
    // `sql_string` node was ever produced (§9.2a decision 1). `query` is not a
    // value anywhere, and `query` with a non-string value is already a parse
    // error in `blastit-manifest` (`any_string`), so nothing is lost -- and the
    // grammar stays *looser* than the language, never stricter.
    //
    // The value is `optional` on purpose: `query` is a legal *name* (a table, a
    // function), and `blastit fmt` writes `functions query` and `select query`
    // as canonical, so a bare `query` token must also be usable as one. With a
    // required value, `query` leaving `keyword` (below) left no production that
    // accepts the token in a value position, and those files -- accepted by
    // `blastit fmt` and error-free under `MB.22.4`'s grammar -- became `ERROR`
    // nodes (reviewer finding 1). A *valueless* `query` is refused by the
    // manifest (`any_string`), so accepting it here is looseness, not a false
    // green: the LSP owns the squiggle and the editor paints no red.
    query_decl: $ => seq('query', optional(choice($.sql_string, $.string))),
    // The `"…"` spelling keeps `$.string` and its `escape` children: an interior
    // that may hold `\"` would need a second tokenizer that knows the escapes,
    // and it would colour half a statement differently from the other half
    // (§9.2a decision 5).
    sql_string: $ => seq(
      "'",
      repeat(choice(
        $.sql_keyword,
        $.sql_parameter,
        $.sql_ellipsis,
        $.sql_number,
        $.sql_word,
        $.sql_operator,
        $.sql_other,
      )),
      "'",
    ),
    // Every interior token is `token.immediate(prec(n, …))` with n > 0, i.e.
    // ABOVE the extras' default precedence 0. That is what stops the `#` of a
    // SQL operator (`#>`, `#>>`) from starting a Blastfile `comment` extra
    // *inside* the literal and eating the closing quote: measured, `'a#b'`
    // parses as word/operator/word with no ERROR and no MISSING (§9.2a
    // decision 4).
    //
    // The keyword list is colour only: every entry also matches `sql_word`'s
    // alphabet, so extending or trimming it changes which token is painted and
    // never whether the file parses.
    sql_keyword: $ => token.immediate(prec(2, choice(
      ci('SELECT'), ci('FROM'), ci('WHERE'), ci('INSERT'), ci('INTO'), ci('VALUES'),
      ci('UPDATE'), ci('SET'), ci('DELETE'), ci('RETURNING'), ci('WITH'), ci('AS'),
      ci('ON'), ci('CONFLICT'), ci('DO'), ci('NOTHING'), ci('JOIN'), ci('LEFT'),
      ci('RIGHT'), ci('INNER'), ci('OUTER'), ci('FULL'), ci('CROSS'), ci('GROUP'),
      ci('BY'), ci('ORDER'), ci('LIMIT'), ci('OFFSET'), ci('HAVING'), ci('DISTINCT'),
      ci('UNION'), ci('ALL'), ci('CASE'), ci('WHEN'), ci('THEN'), ci('ELSE'), ci('END'),
      ci('IS'), ci('NULL'), ci('TRUE'), ci('FALSE'), ci('NOT'), ci('AND'), ci('OR'),
      ci('IN'), ci('EXISTS'), ci('LIKE'), ci('ILIKE'), ci('BETWEEN'), ci('ASC'),
      ci('DESC'), ci('BEGIN'), ci('COMMIT'), ci('ROLLBACK'), ci('SAVEPOINT'),
      ci('CREATE'), ci('TABLE'), ci('INDEX'), ci('ALTER'), ci('DROP'), ci('TRUNCATE'),
      ci('COPY'), ci('PROGRAM'), ci('GRANT'), ci('REVOKE'), ci('TIME'),
      ci('ZONE'), ci('CALL'), ci('EXPLAIN'), ci('ANALYZE'), ci('VACUUM'),
      ci('COUNT'), ci('SUM'), ci('MIN'), ci('MAX'), ci('COALESCE'), ci('NOW'),
      ci('LOWER'), ci('UPPER'),
    ))),
    sql_parameter: $ => token.immediate(prec(2, choice('$?', /\$[0-9]+/))),
    // Three dots in `IN (…, ...)` are one token, never three operators.
    sql_ellipsis: $ => token.immediate(prec(2, '...')),
    sql_number: $ => token.immediate(prec(2, /[0-9]+(\.[0-9]+)?/)),
    sql_word: $ => token.immediate(prec(2, /[A-Za-z_][A-Za-z0-9_$]*/)),
    sql_operator: $ => token.immediate(prec(2, /[-+*\/<>=~!@#%^&|?:.]+/)),
    // The one-character fallback that makes `sql_string` accept exactly
    // `'[^']*'`: precedence 1 still beats the extras (0) and loses every tie to a
    // specific rule above, so nothing else is ever split by it. Its language is
    // the structural proof of "never stricter than `raw_string`" (§9.2a
    // decision 2) -- the corpus is the regression, this is the argument.
    sql_other: $ => token.immediate(prec(1, /[^']/)),

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

    // `query` is deliberately absent: it is `query_decl`, whose own rule keeps
    // `sql_string` reachable (§9.2a decision 1). Everything else here is a
    // directive keyword that may take values.
    keyword: $ => choice(
      'transactions', 'functions', 'unrestricted', 'select', 'insert', 'update', 'delete',
      'body', 'preload', 'root', 'index', 'immutable', 'fallback', 'egress', 'any',
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
