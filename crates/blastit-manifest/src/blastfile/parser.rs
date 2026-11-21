//! The LL(1) `Blastfile` parser (§6): one pass, one token of lookahead, and
//! the one tree of §14.1 as its product.
//!
//! The parser never decides anything with two tokens. §6.2's FIRST sets are
//! disjoint, so every choice is `peek()`: the six top-level sections, the three
//! store forms, the three `view` forms (§6.3), and "endpoint or directive"
//! inside a view block — which is the one-bit rule that a `Word` starting
//! `A`–`Z` is a `Method` and everything else is a keyword (§4.2).
//!
//! Every loop is bounded by the input and every nesting by the grammar: the
//! depth counter refuses a third block (§4.5's `MAX_BLOCK_DEPTH`) before the
//! recursion can grow, and every list loop refuses its 257th item
//! (`MAX_LIST_ITEMS`). The cursor is a slice iterator, not a hand-written
//! index, so no scan can be frozen by a mutant (`LESSONS.md`).
//!
//! Nothing here lowers into a `Manifest`: `lower` does that. The one value
//! the parser does decide is a pattern's text, because §8.3's rows are refusals
//! at the pattern's own `line:col` (§8.1) and only the parser sees which
//! spelling was written. The compiled fragment is stored on the pattern's
//! token, so `lower` reads it instead of compiling the text a second time.

use std::collections::{BTreeMap, HashMap, HashSet};

use super::lexer::lex;
use super::pattern::{self, Compiled};
use super::syntax::{Builder, Cst, NodeKind, TokenId};
use super::tokens::{Lexed, Token, TokenKind, raw_value, str_value};
use super::{MAX_BLOCK_DEPTH, MAX_LIST_ITEMS, MAX_SOURCE_BYTES};
use crate::error::{ManifestError, ManifestErrors, Position, clipped};

/// Parses one `Blastfile` into the lossless tree of §14.1.
///
/// The size cap is checked before a byte is lexed (§4.5), so a hostile input is
/// refused by one `len()` rather than by a token stream that has to be built
/// first.
///
/// # Errors
///
/// [`ManifestError::TooLarge`] over 16 MiB, the lexer's own refusals (§5.3), and
/// §6.7's parse refusals — each at the offending token's `line:col`.
pub(crate) fn parse(text: &str) -> Result<Cst, ManifestErrors> {
    if text.len() > MAX_SOURCE_BYTES {
        return Err(ManifestError::TooLarge {
            bytes: text.len() as u64,
        }
        .into());
    }
    let lexed = lex(text).map_err(ManifestErrors::from)?;
    let mut parser = Parser::new(text, &lexed);
    parser.file().map_err(ManifestErrors::from)?;
    Ok(parser.into_cst(&lexed))
}

/// The byte offset of the start of every line, `0` first (§18's `line:col`).
///
/// One pass over the source, so a position is a binary search: the pattern hook
/// asks for one per pattern, and a per-item scan from byte 0 is `O(n²)` on a
/// generated file (`LESSONS.md`). `lower` builds the same index
/// once, so a lowering refusal is a `line:col` too.
pub(crate) fn line_starts(text: &str) -> Vec<u32> {
    let mut starts = Vec::new();
    starts.push(0);
    starts.extend(
        text.bytes()
            .enumerate()
            .filter(|(_, byte)| *byte == b'\n')
            .map(|(offset, _)| offset as u32 + 1),
    );
    starts
}

/// True for a `Word` the grammar reads as a `Method`: its first byte is `A`–`Z`
/// (§4.2, §6.3). Everything else is a keyword or a name.
fn is_method_word(word: &str) -> bool {
    word.as_bytes()
        .first()
        .is_some_and(|byte| byte.is_ascii_uppercase())
}

/// True for a name the grammar accepts: `[a-z0-9][a-z0-9_-]*` (§6.7).
fn is_name(word: &str) -> bool {
    let mut bytes = word.bytes();
    match bytes.next() {
        Some(first) if first.is_ascii_lowercase() || first.is_ascii_digit() => {}
        _ => return false,
    }
    bytes.all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-'
    })
}

/// True for a word the grammar uses as a directive somewhere (§6.1's closed
/// keyword set, plus `regex`, which §6.3 reserves in a pattern list).
///
/// A word outside this set cannot start a directive in any block, so inside a
/// view block it can only be a method attempt — and §6.3 refuses a lower-case
/// method with its own sentence rather than §6.7's "not a directive here".
/// Every keyword of the language is listed, the `body` codecs and the two
/// `store_grant`/`egress` sub-tokens included: `view v { json }` is a directive
/// in the wrong place (§6.7 row 3), not a lower-case method.
fn is_keyword(word: &str) -> bool {
    matches!(
        word,
        "app"
            | "entrypoint"
            | "postgres"
            | "trait"
            | "view"
            | "static"
            | "python"
            | "php"
            | "preload"
            | "host"
            | "port"
            | "user"
            | "password"
            | "database"
            | "sslmode"
            | "ca_cert"
            | "url"
            | "body"
            | "use"
            | "egress"
            | "prefork"
            | "root"
            | "index"
            | "fallback"
            | "immutable"
            | "transactions"
            | "functions"
            | "select"
            | "insert"
            | "update"
            | "delete"
            | "query"
            | "regex"
            | "workers"
            | "unrestricted"
            | "any"
            | "json"
            | "form"
            | "none"
            | "raw"
            | "unvalidated"
    )
}

/// True for a word that is a directive of a *block* — a view, a `static` block
/// or an endpoint — rather than a top-level section (§6.7 row 3).
///
/// At the top level such a word is "not a directive here", not "not a keyword":
/// `root "public"` outside a mount is a misplaced directive, and the sentence
/// says which sections the file does take.
fn is_block_directive(word: &str) -> bool {
    matches!(
        word,
        "body" | "use" | "egress" | "prefork" | "root" | "index" | "fallback" | "immutable"
    )
}

/// True when the next token starts a `Word` item.
fn starts_word(kind: TokenKind, _word: Option<&str>) -> bool {
    kind == TokenKind::Word
}

/// True when the next token starts a `Str` item.
fn starts_string(kind: TokenKind, _word: Option<&str>) -> bool {
    kind == TokenKind::Str
}

/// True when the next token starts a `Method` item.
fn starts_method(kind: TokenKind, word: Option<&str>) -> bool {
    kind == TokenKind::Word && word.is_some_and(is_method_word)
}

/// True when the next token starts a `pattern` item (§6.2).
fn starts_pattern(kind: TokenKind, word: Option<&str>) -> bool {
    kind == TokenKind::Str || (kind == TokenKind::Word && word == Some("regex"))
}

/// What one `view` block has declared so far, for §6.7's duplicate rows.
#[derive(Debug, Default)]
struct ViewState {
    /// How many endpoints the view declares, the header's included.
    endpoints: u32,
    /// Whether the view body already declares a `body`.
    body: bool,
    /// The stores the view body already grants; `None` is the default store.
    granted: HashSet<Option<String>>,
}

/// One parse: the source, the token stream, and the tree being built.
struct Parser<'a> {
    /// The source, for token text and `line:col`.
    text: &'a str,
    /// The byte offset of the start of every line, so a position is a binary
    /// search instead of a scan from byte 0 (`LESSONS.md`).
    line_starts: Vec<u32>,
    /// Every token, so a token's index is its `TokenId`.
    tokens: &'a [Token],
    /// The tokens not yet consumed; `Eof` is the last one.
    rest: std::slice::Iter<'a, Token>,
    /// The arena being built.
    builder: Builder,
    /// The compiled form of every `pattern`, by its token (§8.1).
    patterns: BTreeMap<TokenId, Compiled>,
    /// How many blocks are open, for §4.5's `MAX_BLOCK_DEPTH`.
    depth: u32,
    /// Offset of the most recently opened block, for the stray-`}` sentence.
    last_block: Option<u32>,
    /// Offset of the first `app`, for the duplicate sentence.
    app: Option<u32>,
    /// Offset of the first `entrypoint`, for the duplicate sentence.
    entrypoint: Option<u32>,
    /// Every `view` name and the offset of its first declaration.
    views: HashMap<String, u32>,
    /// Every `trait` name and the offset of its first declaration.
    traits: HashMap<String, u32>,
    /// Every named `postgres` store and the offset of its first declaration.
    stores: HashMap<String, u32>,
}

impl<'a> Parser<'a> {
    /// A parser at the first token of `lexed`.
    fn new(text: &'a str, lexed: &'a Lexed) -> Self {
        Self {
            text,
            line_starts: line_starts(text),
            tokens: &lexed.tokens,
            rest: lexed.tokens.iter(),
            builder: Builder::new(),
            patterns: BTreeMap::new(),
            depth: 0,
            last_block: None,
            app: None,
            entrypoint: None,
            views: HashMap::new(),
            traits: HashMap::new(),
            stores: HashMap::new(),
        }
    }

    /// Seals the tree, copying the token and trivia tables into it (§5.2).
    fn into_cst(self, lexed: &Lexed) -> Cst {
        self.builder.build(
            lexed.tokens.clone(),
            lexed.trivia.clone(),
            self.patterns,
            self.text,
        )
    }

    // ---- the cursor -----------------------------------------------------

    /// What the next token is; `Eof` past the end.
    fn peek(&self) -> TokenKind {
        self.rest
            .as_slice()
            .first()
            .map_or(TokenKind::Eof, |token| token.kind)
    }

    /// The next token's `Word` text, when it is a `Word`.
    fn peek_word(&self) -> Option<&'a str> {
        let token = self.rest.as_slice().first()?;
        if token.kind != TokenKind::Word {
            return None;
        }
        self.text
            .get(token.span.start as usize..token.span.end as usize)
    }

    /// The next token's source text, whatever kind it is.
    fn here_text(&self) -> Option<&'a str> {
        let token = self.rest.as_slice().first()?;
        self.text
            .get(token.span.start as usize..token.span.end as usize)
    }

    /// The byte offset of the next token; the end of the source past `Eof`.
    fn here(&self) -> u32 {
        self.rest
            .as_slice()
            .first()
            .map_or(self.text.len() as u32, |token| token.span.start)
    }

    /// The next token's position, 1-based, `col` in `char`s (§18).
    fn at(&self) -> Position {
        self.position_of(self.here())
    }

    /// The position of a byte offset, 1-based, `col` in `char`s.
    ///
    /// The line is a binary search over [`line_starts`], so this is `O(log n)`
    /// rather than a scan from byte 0: the pattern hook calls it once per
    /// pattern, and a scan per item is `O(n²)` on a generated file
    /// (`LESSONS.md`).
    fn position_of(&self, offset: u32) -> Position {
        let end = (offset as usize).min(self.text.len());
        // The number of line starts at or before `end` is the 1-based line.
        let line = self
            .line_starts
            .partition_point(|start| *start as usize <= end)
            .max(1);
        let start = self
            .line_starts
            .get(line.saturating_sub(1))
            .copied()
            .unwrap_or(0);
        let col = self
            .text
            .get(start as usize..end)
            .map_or(1, |tail| tail.chars().count() as u32 + 1);
        Position::at(line as u32, col)
    }

    /// The line of a byte offset.
    fn line_of(&self, offset: u32) -> u32 {
        self.position_of(offset).line()
    }

    /// The source text a token spans.
    fn token_text(&self, token: TokenId) -> &'a str {
        self.tokens
            .get(token as usize)
            .and_then(|token| {
                self.text
                    .get(token.span.start as usize..token.span.end as usize)
            })
            .unwrap_or("")
    }

    /// Consumes the next token, emitting its leading trivia and the token.
    ///
    /// The index comes from the iterator's own length, so the step is the
    /// iterator and nothing can freeze it.
    fn bump(&mut self) -> Result<TokenId, ManifestError> {
        let index = self.tokens.len().saturating_sub(self.rest.len()) as u32;
        let Some(token) = self.rest.next() else {
            return Err(self.unexpected("a token"));
        };
        let leading = token.leading.clone();
        for trivia in leading {
            self.builder.trivia(trivia);
        }
        self.builder.token(index);
        Ok(index)
    }

    /// Consumes the next token when it is `kind`, or refuses with `what`.
    fn expect(&mut self, kind: TokenKind, what: &'static str) -> Result<TokenId, ManifestError> {
        if self.peek() != kind {
            return Err(self.unexpected(what));
        }
        self.bump()
    }

    /// Consumes every `Newline` and `;` token, leaving them in the tree (§6.6).
    ///
    /// Returns whether it consumed one, which is how a statement knows it was
    /// terminated.
    fn separators(&mut self) -> Result<bool, ManifestError> {
        let skipped = self
            .rest
            .as_slice()
            .iter()
            .take_while(|token| matches!(token.kind, TokenKind::Newline | TokenKind::Semi))
            .count();
        for _ in 0..skipped {
            self.bump()?;
        }
        Ok(skipped > 0)
    }

    /// Consumes every `Newline`, leaving them in the tree (§6.6).
    ///
    /// A `Newline` right after a `,`, a `{` or an unclosed `(` continues the
    /// statement rather than ending it, so those three places skip newlines
    /// only — a `;` there still ends the statement.
    fn newlines(&mut self) -> Result<(), ManifestError> {
        let skipped = self
            .rest
            .as_slice()
            .iter()
            .take_while(|token| token.kind == TokenKind::Newline)
            .count();
        for _ in 0..skipped {
            self.bump()?;
        }
        Ok(())
    }

    /// Ends one statement: a `Newline`, a `;`, the closing `}` or the end of
    /// the file must follow (§6.6).
    fn end_statement(&mut self) -> Result<(), ManifestError> {
        if !self.separators()? && !matches!(self.peek(), TokenKind::Eof | TokenKind::RBrace) {
            return Err(self.unexpected("a newline, `;` or `}`"));
        }
        Ok(())
    }

    // ---- refusals -------------------------------------------------------

    /// What the next token is, for an `expected …, found …` refusal.
    ///
    /// A `Word` or an `$IDENT` is a name and is quoted as written; a string is
    /// named by its class, never echoed, so no value that could be a credential
    /// reaches a message (§18).
    fn describe(&self, kind: TokenKind) -> String {
        match kind {
            TokenKind::Word | TokenKind::Env => match self.here_text() {
                Some(text) => format!("`{}`", clipped(text)),
                None => "a word".to_owned(),
            },
            TokenKind::Str => "a string".to_owned(),
            TokenKind::RawStr => "a raw string".to_owned(),
            TokenKind::LBrace => "`{`".to_owned(),
            TokenKind::RBrace => "`}`".to_owned(),
            TokenKind::LParen => "`(`".to_owned(),
            TokenKind::RParen => "`)`".to_owned(),
            TokenKind::Comma => "`,`".to_owned(),
            TokenKind::Semi => "`;`".to_owned(),
            TokenKind::Newline => "a newline".to_owned(),
            TokenKind::Equals => "`=`".to_owned(),
            TokenKind::Dot => "`.`".to_owned(),
            TokenKind::Unknown(byte) => format!("byte 0x{byte:x}"),
            TokenKind::Eof => "the end of the file".to_owned(),
        }
    }

    /// The refusal for "the grammar does not allow this token here".
    ///
    /// Three tokens have their own sentence wherever they appear — an unknown
    /// byte (§5.1), a `.` outside a `functions` name (§6.7) and an `=` (§5.3) —
    /// so every call site gets the specific one for free.
    fn unexpected(&self, expected: &'static str) -> ManifestError {
        let at = self.at();
        match self.peek() {
            TokenKind::Unknown(byte) => ManifestError::UnknownByte { at, byte },
            TokenKind::Dot => ManifestError::DotOutsideFunction { at },
            TokenKind::Equals => ManifestError::Equals { at },
            kind => ManifestError::Unexpected {
                at,
                expected,
                found: self.describe(kind),
            },
        }
    }

    /// The refusal for a block that reaches the end of the file unclosed.
    fn unclosed(&self, opened: u32) -> ManifestError {
        ManifestError::UnclosedBlock {
            at: self.at(),
            opened: self.line_of(opened),
        }
    }

    /// The refusal for a `}` with no open block.
    fn stray_brace(&self) -> ManifestError {
        match self.last_block {
            Some(opened) => ManifestError::StrayCloseBrace {
                at: self.at(),
                opened: self.line_of(opened),
            },
            // A file whose first token is `}` has no block to name, so the
            // generic sentence is the honest one.
            None => self.unexpected("a section"),
        }
    }

    /// The refusal for a lower-case word where a method belongs (§6.7).
    fn not_a_method(&self) -> ManifestError {
        match self.peek_word() {
            Some(word) => ManifestError::NotAMethod {
                at: self.at(),
                word: word.to_owned(),
                upper: word.to_ascii_uppercase(),
            },
            None => self.unexpected("a method"),
        }
    }

    /// The refusal for a word inside a `trait` that is not a `postgres` grant.
    fn trait_directive(&self, word: &str) -> ManifestError {
        let (what, place) = match word {
            "egress" => ("`egress any`".to_owned(), "on a view"),
            "body" | "use" | "prefork" => (format!("`{}`", clipped(word)), "on a view"),
            "view" | "app" | "entrypoint" | "trait" | "static" => {
                (format!("`{}`", clipped(word)), "at the top level")
            }
            other => {
                return ManifestError::NotADirectiveHere {
                    at: self.at(),
                    word: other.to_owned(),
                    block: "a trait",
                    takes: "`postgres`",
                };
            }
        };
        ManifestError::TraitHoldsGrantsOnly {
            at: self.at(),
            what,
            place,
        }
    }

    // ---- blocks ---------------------------------------------------------

    /// Consumes a `{`, checks §4.5's depth budget, and returns its offset.
    fn open_block(&mut self) -> Result<u32, ManifestError> {
        let opened = self.here();
        self.expect(TokenKind::LBrace, "`{`")?;
        self.depth = self.depth.saturating_add(1);
        if self.depth > MAX_BLOCK_DEPTH {
            return Err(ManifestError::BlockTooDeep {
                at: self.position_of(opened),
            });
        }
        self.last_block = Some(opened);
        Ok(opened)
    }

    /// Consumes the `}` that closes the block opened at `opened`.
    fn close_block(&mut self, opened: u32) -> Result<(), ManifestError> {
        if self.peek() == TokenKind::Eof {
            return Err(self.unclosed(opened));
        }
        self.expect(TokenKind::RBrace, "`}`")?;
        self.depth = self.depth.saturating_sub(1);
        Ok(())
    }

    // ---- leaves ---------------------------------------------------------

    /// Consumes a `"…"` string, or refuses with `what`.
    fn string_token(&mut self, what: &'static str) -> Result<TokenId, ManifestError> {
        if self.peek() != TokenKind::Str {
            return Err(self.unexpected(what));
        }
        self.bump()
    }

    /// Consumes a `"…"` or `'…'` string, or refuses with `what`.
    fn any_string(&mut self, what: &'static str) -> Result<TokenId, ManifestError> {
        if !matches!(self.peek(), TokenKind::Str | TokenKind::RawStr) {
            return Err(self.unexpected(what));
        }
        self.bump()
    }

    /// Consumes a `$IDENT`, refusing a quoted literal with §5.3's sentence.
    fn env_token(&mut self) -> Result<TokenId, ManifestError> {
        match self.peek() {
            TokenKind::Env => self.bump(),
            TokenKind::Str | TokenKind::RawStr => Err(ManifestError::QuotedStore { at: self.at() }),
            _ => Err(self.unexpected("`$IDENT`")),
        }
    }

    /// Consumes a `Word`, or refuses with `what`.
    fn word_token(&mut self, what: &'static str) -> Result<TokenId, ManifestError> {
        if self.peek() != TokenKind::Word {
            return Err(self.unexpected(what));
        }
        self.bump()
    }

    /// Consumes a name — `[a-z0-9][a-z0-9_-]*` — and returns it with its offset.
    fn name(&mut self, what: &'static str) -> Result<(String, u32), ManifestError> {
        let Some(word) = self.peek_word() else {
            return Err(self.unexpected(what));
        };
        if !is_name(word) {
            return Err(ManifestError::InvalidName {
                at: self.at(),
                name: word.to_owned(),
            });
        }
        let at = self.here();
        let name = word.to_owned();
        self.bump()?;
        Ok((name, at))
    }

    /// Consumes a `view` name, refusing a `Method` with §6.7's own sentence.
    fn view_name(&mut self) -> Result<(String, u32), ManifestError> {
        let Some(word) = self.peek_word() else {
            return Err(self.unexpected("a view name"));
        };
        if is_method_word(word) {
            return Err(ManifestError::MethodAsName {
                at: self.at(),
                name: word.to_owned(),
            });
        }
        self.name("a view name")
    }

    /// One `Str` item of a list.
    fn string_item(&mut self) -> Result<(), ManifestError> {
        self.string_token("a `\"…\"` string").map(|_| ())
    }

    /// One trait name of a `use` list.
    fn trait_name(&mut self) -> Result<(), ManifestError> {
        self.name("a trait name").map(|_| ())
    }

    /// One column of a table grant.
    fn column(&mut self) -> Result<(), ManifestError> {
        self.word_token("a column name").map(|_| ())
    }

    /// One table name of a `delete` grant.
    fn table_name(&mut self) -> Result<(), ManifestError> {
        self.word_token("a table name").map(|_| ())
    }

    /// One `function` of a `functions` grant: `Word ('.' Word)*`.
    fn function(&mut self) -> Result<(), ManifestError> {
        self.word_token("a function name")?;
        while self.peek() == TokenKind::Dot {
            self.bump()?;
            self.word_token("a function name")?;
        }
        Ok(())
    }

    /// One `table_grant`: `Word ('(' Word (',' Word)* ')')?`.
    fn table_grant(&mut self) -> Result<(), ManifestError> {
        self.builder.start(NodeKind::TableGrant);
        self.word_token("a table name")?;
        if self.peek() == TokenKind::LParen {
            self.bump()?;
            self.newlines()?;
            self.comma_list("a column name", starts_word, Parser::column)?;
            self.newlines()?;
            self.expect(TokenKind::RParen, "`)`")?;
        }
        self.builder.finish();
        Ok(())
    }

    /// One `pattern`: a `"…"` string, or `regex` and a `'…'` string.
    ///
    /// The pattern is compiled here, at its own position, so §8.3's rows and
    /// the `regex` crate's own message are parse refusals with a `line:col`
    /// (§8.1: both spellings are capped at `MAX_PATTERN_BYTES` "in the parser").
    /// The fragment it emits is stored in the tree, keyed by the pattern's
    /// token, so `lower` reads it instead of compiling the text a second time.
    fn pattern(&mut self) -> Result<TokenId, ManifestError> {
        self.builder.start(NodeKind::Pattern);
        let (token, compiled) = match self.peek() {
            TokenKind::Str => {
                let at = self.at();
                let token = self.bump()?;
                let text = str_value(self.token_text(token));
                (token, pattern::compile(&text, at)?)
            }
            TokenKind::Word if self.peek_word() == Some("regex") => {
                self.bump()?;
                match self.peek() {
                    TokenKind::RawStr => {
                        let at = self.at();
                        let token = self.bump()?;
                        let text = raw_value(self.token_text(token));
                        (token, pattern::compile_regex(text, at)?)
                    }
                    TokenKind::Str => {
                        return Err(ManifestError::RegexNeedsRawString { at: self.at() });
                    }
                    _ => return Err(ManifestError::RegexWithoutRawString { at: self.at() }),
                }
            }
            _ => return Err(self.unexpected("a pattern")),
        };
        self.patterns.insert(token, compiled);
        self.builder.finish();
        Ok(token)
    }

    // ---- lists ----------------------------------------------------------

    /// A comma-separated list of at least one item (§6.1, §4.5).
    ///
    /// A `Newline` right after a `,` continues the list (§6.6) and stays in the
    /// tree; a `,` with no item after it is §6.7's trailing-comma refusal, and
    /// the 257th item is §4.5's list cap.
    fn comma_list<T>(
        &mut self,
        what: &'static str,
        starts: fn(TokenKind, Option<&str>) -> bool,
        item: fn(&mut Self) -> Result<T, ManifestError>,
    ) -> Result<Vec<T>, ManifestError> {
        if self.peek() == TokenKind::Comma {
            return Err(ManifestError::ListStartsWithComma { at: self.at() });
        }
        if !starts(self.peek(), self.peek_word()) {
            return Err(self.unexpected(what));
        }
        let mut items = vec![item(self)?];
        while self.peek() == TokenKind::Comma {
            let comma = self.here();
            self.bump()?;
            self.newlines()?;
            if !starts(self.peek(), self.peek_word()) {
                if self.peek() == TokenKind::Comma {
                    return Err(self.unexpected(what));
                }
                return Err(ManifestError::TrailingComma {
                    at: self.position_of(comma),
                });
            }
            if items.len() >= MAX_LIST_ITEMS {
                return Err(ManifestError::TooManyListItems { at: self.at() });
            }
            items.push(item(self)?);
        }
        Ok(items)
    }

    /// `methods`: `Method (',' Method)*`.
    fn method_list(&mut self) -> Result<Vec<(String, u32)>, ManifestError> {
        self.comma_list("a method", starts_method, Parser::method_token)
    }

    /// One `Method` of a method list.
    fn method_token(&mut self) -> Result<(String, u32), ManifestError> {
        let Some(word) = self.peek_word() else {
            return Err(self.unexpected("a method"));
        };
        if !is_method_word(word) {
            return Err(self.not_a_method());
        }
        let at = self.here();
        let method = word.to_owned();
        self.bump()?;
        Ok((method, at))
    }

    /// `patterns`: `pattern (',' pattern)*`.
    fn pattern_list(&mut self) -> Result<Vec<TokenId>, ManifestError> {
        self.comma_list("a pattern", starts_pattern, Parser::pattern)
    }

    // ---- statements -----------------------------------------------------

    /// `file → statement* EOF`.
    fn file(&mut self) -> Result<(), ManifestError> {
        self.builder.start(NodeKind::File);
        loop {
            self.separators()?;
            match self.peek() {
                TokenKind::Eof => break,
                TokenKind::RBrace => return Err(self.stray_brace()),
                _ => {}
            }
            self.statement()?;
            self.end_statement()?;
        }
        self.bump()?;
        self.builder.finish();
        Ok(())
    }

    /// `statement → app | entrypoint | store | trait | view | static`.
    fn statement(&mut self) -> Result<(), ManifestError> {
        let Some(word) = self.peek_word() else {
            return Err(self.unexpected("a section"));
        };
        match word {
            "app" => self.app(),
            "entrypoint" => self.entrypoint(),
            "postgres" => self.store(),
            "trait" => self.trait_(),
            "view" => self.view(),
            "static" => self.static_(),
            "workers" => Err(ManifestError::WorkersRemoved { at: self.at() }),
            // §6.7 row 3: a directive of a block written at the top level is
            // "not a directive here", naming the sections the file does take.
            other if is_block_directive(other) => Err(ManifestError::NotADirectiveHere {
                at: self.at(),
                word: other.to_owned(),
                block: "the top level",
                takes: "`app`, `entrypoint`, `postgres`, `trait`, `view` and `static`",
            }),
            other => Err(ManifestError::UnknownKeyword {
                at: self.at(),
                word: other.to_owned(),
            }),
        }
    }

    /// `app → 'app' Str`.
    fn app(&mut self) -> Result<(), ManifestError> {
        if let Some(first) = self.app {
            return Err(ManifestError::DuplicateApp {
                at: self.at(),
                first: self.line_of(first),
            });
        }
        self.app = Some(self.here());
        self.builder.start(NodeKind::App);
        self.bump()?;
        self.string_token("a `\"…\"` string")?;
        self.builder.finish();
        Ok(())
    }

    /// `entrypoint → 'entrypoint' ( python | php )`.
    fn entrypoint(&mut self) -> Result<(), ManifestError> {
        if let Some(first) = self.entrypoint {
            return Err(ManifestError::DuplicateEntrypoint {
                at: self.at(),
                first: self.line_of(first),
            });
        }
        self.entrypoint = Some(self.here());
        self.builder.start(NodeKind::Entrypoint);
        self.bump()?;
        match self.peek_word() {
            Some("python") => {
                self.bump()?;
                self.string_token("a `\"…\"` callable")?;
                if self.peek() == TokenKind::LBrace {
                    self.python_block()?;
                }
            }
            Some("php") => {
                self.bump()?;
                self.string_token("a `\"…\"` path")?;
            }
            Some("workers") => return Err(ManifestError::WorkersRemoved { at: self.at() }),
            Some(_) | None => return Err(self.unexpected("`python` or `php`")),
        }
        self.builder.finish();
        Ok(())
    }

    /// `python_block → '{' 'preload' Str (',' Str)* '}'`.
    fn python_block(&mut self) -> Result<(), ManifestError> {
        let opened = self.open_block()?;
        loop {
            self.newlines()?;
            if self.peek() == TokenKind::Eof {
                return Err(self.unclosed(opened));
            }
            if self.peek() == TokenKind::RBrace {
                break;
            }
            match self.peek_word() {
                Some("preload") => {
                    self.builder.start(NodeKind::Preload);
                    self.bump()?;
                    self.comma_list("a `\"…\"` module", starts_string, Parser::string_item)?;
                    self.builder.finish();
                }
                Some("workers") => return Err(ManifestError::WorkersRemoved { at: self.at() }),
                Some(_) | None => {
                    return Err(ManifestError::EntrypointBlockHoldsPreload { at: self.at() });
                }
            }
            self.end_statement()?;
        }
        self.close_block(opened)
    }

    /// `store → 'postgres' ( Env | store_block | store_name ( Env | store_block ) )`.
    fn store(&mut self) -> Result<(), ManifestError> {
        self.builder.start(NodeKind::Store);
        let keyword = self.here();
        self.bump()?;
        match self.peek() {
            TokenKind::Env => {
                self.bump()?;
            }
            TokenKind::LBrace => self.store_block()?,
            TokenKind::Word => {
                if self.peek_word() == Some("unrestricted") {
                    return Err(ManifestError::StoreNameReserved { at: self.at() });
                }
                let (name, at) = self.name("a store name")?;
                if let Some(first) = self.stores.get(&name).copied() {
                    return Err(ManifestError::DuplicateName {
                        at: self.position_of(keyword),
                        kind: "store",
                        name,
                        first: self.line_of(first),
                    });
                }
                self.stores.insert(name, at);
                match self.peek() {
                    TokenKind::Env => {
                        self.bump()?;
                    }
                    TokenKind::LBrace => self.store_block()?,
                    _ => return Err(self.unexpected("`$IDENT` or `{`")),
                }
            }
            TokenKind::Str | TokenKind::RawStr => {
                return Err(ManifestError::QuotedStore { at: self.at() });
            }
            _ => return Err(self.unexpected("`$IDENT`, a store name or `{`")),
        }
        // §6.7: a URL and a block are two forms, so a second one is refused
        // rather than read as the next statement.
        if matches!(self.peek(), TokenKind::Env | TokenKind::LBrace) {
            return Err(ManifestError::StoreTwoForms { at: self.at() });
        }
        self.builder.finish();
        Ok(())
    }

    /// `store_block → '{' slot* '}'`.
    fn store_block(&mut self) -> Result<(), ManifestError> {
        self.builder.start(NodeKind::StoreBlock);
        let opened = self.open_block()?;
        let mut slots: Vec<&'static str> = Vec::new();
        loop {
            self.newlines()?;
            if self.peek() == TokenKind::Eof {
                return Err(self.unclosed(opened));
            }
            if self.peek() == TokenKind::RBrace {
                break;
            }
            let Some(word) = self.peek_word() else {
                return Err(self.unexpected("a store slot"));
            };
            let slot: &'static str = match word {
                "host" => "host",
                "port" => "port",
                "user" => "user",
                "password" => "password",
                "database" => "database",
                "sslmode" => "sslmode",
                "ca_cert" => "ca_cert",
                "url" => return Err(ManifestError::UrlSlot { at: self.at() }),
                "workers" => return Err(ManifestError::WorkersRemoved { at: self.at() }),
                other => {
                    return Err(ManifestError::NotADirectiveHere {
                        at: self.at(),
                        word: other.to_owned(),
                        block: "a store block",
                        takes: "`host`, `port`, `user`, `password`, `database`, `sslmode` and `ca_cert`",
                    });
                }
            };

            if slots.contains(&slot) {
                return Err(ManifestError::DuplicateSlot {
                    at: self.at(),
                    slot,
                });
            }
            slots.push(slot);
            self.builder.start(NodeKind::Slot);
            self.bump()?;
            self.env_token()?;
            self.builder.finish();
            self.end_statement()?;
        }
        if slots.is_empty() {
            return Err(ManifestError::EmptyStoreBlock {
                at: self.position_of(opened),
            });
        }
        self.close_block(opened)?;
        self.builder.finish();
        Ok(())
    }

    /// `trait → 'trait' Word '{' store_grant* '}'`.
    fn trait_(&mut self) -> Result<(), ManifestError> {
        self.builder.start(NodeKind::Trait);
        let keyword = self.here();
        self.bump()?;
        let (name, at) = self.name("a trait name")?;
        if let Some(first) = self.traits.get(&name).copied() {
            return Err(ManifestError::DuplicateName {
                at: self.position_of(keyword),
                kind: "trait",
                name,
                first: self.line_of(first),
            });
        }
        self.traits.insert(name, at);
        let opened = self.open_block()?;
        loop {
            self.newlines()?;
            if self.peek() == TokenKind::Eof {
                return Err(self.unclosed(opened));
            }
            if self.peek() == TokenKind::RBrace {
                break;
            }
            match self.peek_word() {
                // A trait's grants are not deduplicated: §6.7 refuses two
                // `postgres` blocks for one store *in a view*, and a trait's
                // grants are unioned by `lower`.
                Some("postgres") => self.store_grant(None)?,
                Some("workers") => return Err(ManifestError::WorkersRemoved { at: self.at() }),
                Some(other) => return Err(self.trait_directive(other)),
                None => return Err(self.unexpected("`postgres`")),
            }
            self.end_statement()?;
        }
        self.close_block(opened)?;
        self.builder.finish();
        Ok(())
    }

    /// `view → 'view' Word ( view_header view_block | view_block )`.
    fn view(&mut self) -> Result<(), ManifestError> {
        self.builder.start(NodeKind::View);
        let keyword = self.here();
        self.bump()?;
        let (name, at) = self.view_name()?;
        if let Some(first) = self.views.get(&name).copied() {
            return Err(ManifestError::DuplicateName {
                at: self.position_of(keyword),
                kind: "view",
                name,
                first: self.line_of(first),
            });
        }
        self.views.insert(name.clone(), at);
        // §6.3's three-way decision: `{`, a `Method`, or a pattern.
        let header = if self.peek() == TokenKind::LBrace {
            false
        } else {
            self.builder.start(NodeKind::ViewHeader);
            match self.peek() {
                TokenKind::Str => {
                    self.pattern_list()?;
                }
                TokenKind::Word if self.peek_word() == Some("regex") => {
                    self.pattern_list()?;
                }
                TokenKind::Word if self.peek_word().is_some_and(is_method_word) => {
                    self.method_list()?;
                    if !starts_pattern(self.peek(), self.peek_word()) {
                        return Err(ManifestError::MethodsWithoutPattern {
                            at: self.at(),
                            name: name.clone(),
                        });
                    }
                    self.pattern_list()?;
                }
                TokenKind::Word => return Err(self.not_a_method()),
                _ => return Err(self.unexpected("a method, a pattern or `{`")),
            }
            self.builder.finish();
            true
        };
        let opened = self.open_block()?;
        let mut state = ViewState {
            endpoints: u32::from(header),
            ..ViewState::default()
        };
        loop {
            self.newlines()?;
            if self.peek() == TokenKind::Eof {
                return Err(self.unclosed(opened));
            }
            if self.peek() == TokenKind::RBrace {
                break;
            }
            self.view_item(header, &name, &mut state)?;
            self.end_statement()?;
        }
        if state.endpoints == 0 {
            return Err(ManifestError::ViewWithoutEndpoint {
                at: self.position_of(opened),
                name,
            });
        }
        self.close_block(opened)?;
        self.builder.finish();
        Ok(())
    }

    /// `view_item → endpoint | view_directive`.
    fn view_item(
        &mut self,
        header: bool,
        name: &str,
        state: &mut ViewState,
    ) -> Result<(), ManifestError> {
        let Some(word) = self.peek_word() else {
            return Err(self.unexpected("a method or a directive"));
        };
        if is_method_word(word) {
            if header {
                // §6.7's row 8 names the endpoint it refuses, so the methods and
                // the pattern are read before the refusal is built.
                let methods = self.method_list()?;
                let pattern = self.pattern()?;
                let (method, at) = methods.first().cloned().unwrap_or_default();
                return Err(ManifestError::EndpointInHeaderView {
                    at: self.position_of(at),
                    name: name.to_owned(),
                    method,
                    pattern: self.token_text(pattern).to_owned(),
                });
            }
            return self.endpoint(state);
        }
        match word {
            "body" => {
                if state.body {
                    return Err(ManifestError::DuplicateBody { at: self.at() });
                }
                state.body = true;
                self.body_directive()
            }
            "use" => self.use_directive(),
            "egress" => self.egress(),
            "prefork" => self.prefork(),
            "postgres" => self.store_grant(Some(state)),
            "workers" => Err(ManifestError::WorkersRemoved { at: self.at() }),
            // §6.3: the method-vs-directive decision is one bit, so a word that
            // is no keyword of the language can only be a method attempt here —
            // and methods are upper-case.
            other if !is_keyword(other) => Err(self.not_a_method()),
            other => Err(ManifestError::NotADirectiveHere {
                at: self.at(),
                word: other.to_owned(),
                block: "a view",
                takes: "`body`, `use`, `egress`, `prefork`, `postgres` and `METHOD \"pattern\"`",
            }),
        }
    }

    /// `endpoint → methods pattern endpoint_block?`.
    fn endpoint(&mut self, state: &mut ViewState) -> Result<(), ManifestError> {
        self.builder.start(NodeKind::Endpoint);
        self.method_list()?;
        self.pattern()?;
        if self.peek() == TokenKind::LBrace {
            self.builder.start(NodeKind::EndpointBlock);
            let opened = self.open_block()?;
            let mut body = false;
            loop {
                self.newlines()?;
                if self.peek() == TokenKind::Eof {
                    return Err(self.unclosed(opened));
                }
                if self.peek() == TokenKind::RBrace {
                    break;
                }
                match self.peek_word() {
                    Some("body") => {
                        if body {
                            return Err(ManifestError::DuplicateEndpointBody { at: self.at() });
                        }
                        body = true;
                        self.body_directive()?;
                    }
                    Some("postgres") => {
                        return Err(ManifestError::GrantIsPerView { at: self.at() });
                    }
                    Some("workers") => return Err(ManifestError::WorkersRemoved { at: self.at() }),
                    Some(other) => {
                        return Err(ManifestError::NotADirectiveHere {
                            at: self.at(),
                            word: other.to_owned(),
                            block: "an endpoint",
                            takes: "`body`",
                        });
                    }
                    None => return Err(self.unexpected("`body`")),
                }
                self.end_statement()?;
            }
            self.close_block(opened)?;
            self.builder.finish();
        }
        self.builder.finish();
        state.endpoints = state.endpoints.saturating_add(1);
        Ok(())
    }

    /// `body → 'body' codec ( 'unvalidated' )?`.
    fn body_directive(&mut self) -> Result<(), ManifestError> {
        self.builder.start(NodeKind::Body);
        self.bump()?;
        let codec = match self.peek() {
            TokenKind::Word => match self.peek_word() {
                Some(codec @ ("json" | "form" | "none" | "raw")) => {
                    self.bump()?;
                    codec
                }
                Some("workers") => return Err(ManifestError::WorkersRemoved { at: self.at() }),
                Some(other) => {
                    return Err(ManifestError::UnknownCodec {
                        at: self.at(),
                        codec: other.to_owned(),
                    });
                }
                None => return Err(self.unexpected("a codec")),
            },
            TokenKind::Newline | TokenKind::Semi | TokenKind::RBrace | TokenKind::Eof => {
                return Err(ManifestError::BodyNeedsCodec { at: self.at() });
            }
            _ => return Err(self.unexpected("a codec")),
        };
        if self.peek_word() == Some("unvalidated") {
            if codec != "raw" {
                return Err(ManifestError::UnvalidatedNeedsRaw {
                    at: self.at(),
                    codec: codec.to_owned(),
                });
            }
            self.bump()?;
        }
        self.builder.finish();
        Ok(())
    }

    /// `use → 'use' Word (',' Word)*`.
    fn use_directive(&mut self) -> Result<(), ManifestError> {
        self.builder.start(NodeKind::Use);
        self.bump()?;
        self.comma_list("a trait name", starts_word, Parser::trait_name)?;
        self.builder.finish();
        Ok(())
    }

    /// `egress → 'egress' 'any'`.
    fn egress(&mut self) -> Result<(), ManifestError> {
        self.builder.start(NodeKind::Egress);
        self.bump()?;
        if self.peek_word() != Some("any") {
            return Err(self.unexpected("`any`"));
        }
        self.bump()?;
        self.builder.finish();
        Ok(())
    }

    /// `prefork → 'prefork'`.
    fn prefork(&mut self) -> Result<(), ManifestError> {
        self.builder.start(NodeKind::Prefork);
        self.bump()?;
        self.builder.finish();
        Ok(())
    }

    /// `store_grant → 'postgres' ( 'unrestricted' | store_name? grant_body )`.
    fn store_grant(&mut self, state: Option<&mut ViewState>) -> Result<(), ManifestError> {
        self.builder.start(NodeKind::Grant);
        let at = self.here();
        self.bump()?;
        let mut done = false;
        let store = match self.peek() {
            TokenKind::LBrace => None,
            TokenKind::Word if self.peek_word() == Some("unrestricted") => {
                self.bump()?;
                done = true;
                None
            }
            TokenKind::Word => Some(self.name("a store name")?.0),
            _ => return Err(self.unexpected("`unrestricted`, a store name or `{`")),
        };
        if !done {
            match self.peek() {
                TokenKind::LBrace => self.grant_block()?,
                TokenKind::Word if self.peek_word() == Some("unrestricted") => {
                    self.bump()?;
                }
                _ => return Err(self.unexpected("`unrestricted` or `{`")),
            }
        }
        if let Some(state) = state
            && !state.granted.insert(store)
        {
            return Err(ManifestError::DuplicateGrant {
                at: self.position_of(at),
            });
        }
        self.builder.finish();
        Ok(())
    }

    /// `grant_block → '{' grant* '}'`.
    fn grant_block(&mut self) -> Result<(), ManifestError> {
        self.builder.start(NodeKind::GrantBlock);
        let opened = self.open_block()?;
        loop {
            self.newlines()?;
            if self.peek() == TokenKind::Eof {
                return Err(self.unclosed(opened));
            }
            if self.peek() == TokenKind::RBrace {
                break;
            }
            self.grant()?;
            self.end_statement()?;
        }
        self.close_block(opened)?;
        self.builder.finish();
        Ok(())
    }

    /// One `grant` of a grant block (§6.1).
    fn grant(&mut self) -> Result<(), ManifestError> {
        let Some(word) = self.peek_word() else {
            return Err(self.unexpected("a grant"));
        };
        match word {
            "transactions" => {
                self.bump()?;
                Ok(())
            }
            "functions" => {
                self.bump()?;
                self.comma_list("a function name", starts_word, Parser::function)?;
                Ok(())
            }
            "select" | "insert" | "update" => {
                self.bump()?;
                self.comma_list("a table name", starts_word, Parser::table_grant)?;
                Ok(())
            }
            "delete" => {
                self.bump()?;
                self.comma_list("a table name", starts_word, Parser::table_name)?;
                Ok(())
            }
            "query" => {
                self.builder.start(NodeKind::Query);
                self.bump()?;
                self.any_string("a `'…'` or `\"…\"` string")?;
                self.builder.finish();
                Ok(())
            }
            "workers" => Err(ManifestError::WorkersRemoved { at: self.at() }),
            other => Err(ManifestError::NotADirectiveHere {
                at: self.at(),
                word: other.to_owned(),
                block: "a grant block",
                takes: "`transactions`, `functions`, `select`, `insert`, `update`, `delete` and `query`",
            }),
        }
    }

    /// `static → 'static' Str '{' static_directive* '}'`.
    fn static_(&mut self) -> Result<(), ManifestError> {
        self.builder.start(NodeKind::Static);
        self.bump()?;
        self.string_token("a `\"…\"` prefix")?;
        let opened = self.open_block()?;
        loop {
            self.newlines()?;
            if self.peek() == TokenKind::Eof {
                return Err(self.unclosed(opened));
            }
            if self.peek() == TokenKind::RBrace {
                break;
            }
            let Some(word) = self.peek_word() else {
                return Err(self.unexpected("a `static` directive"));
            };
            match word {
                "root" | "index" | "fallback" => {
                    self.builder.start(NodeKind::StaticDirective);
                    self.bump()?;
                    self.string_token("a `\"…\"` path")?;
                    self.builder.finish();
                }
                "immutable" => {
                    self.builder.start(NodeKind::StaticDirective);
                    self.bump()?;
                    // Bare `immutable` is a statement on its own (§9.4): the
                    // token that ends it is the separator, not an item.
                    if !matches!(
                        self.peek(),
                        TokenKind::Newline | TokenKind::Semi | TokenKind::RBrace
                    ) {
                        self.pattern_list()?;
                    }
                    self.builder.finish();
                }
                "workers" => return Err(ManifestError::WorkersRemoved { at: self.at() }),
                other => {
                    return Err(ManifestError::NotADirectiveHere {
                        at: self.at(),
                        word: other.to_owned(),
                        block: "a `static` block",
                        takes: "`root`, `index`, `fallback` and `immutable`",
                    });
                }
            }
            self.end_statement()?;
        }
        self.close_block(opened)?;
        self.builder.finish();
        Ok(())
    }
}
