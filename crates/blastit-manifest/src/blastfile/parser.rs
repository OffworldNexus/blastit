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
//! index, so no scan can be frozen by a mutant (`LESSONS.md`, `MB.1.4`).
//!
//! Nothing here lowers into a `Manifest`: that is `MB.21.3.8`'s, and a parser
//! that validated would let the two leaves disagree about which document is
//! legal.

use std::collections::{HashMap, HashSet};

use super::lexer::lex;
use super::syntax::{Builder, Cst, NodeKind, TokenId};
use super::tokens::{Lexed, Token, TokenKind};
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
/// [`ManifestError::TooLarge`] over 1 MiB, the lexer's own refusals (§5.3), and
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
    /// Every token, so a token's index is its `TokenId`.
    tokens: &'a [Token],
    /// The tokens not yet consumed; `Eof` is the last one.
    rest: std::slice::Iter<'a, Token>,
    /// The arena being built.
    builder: Builder,
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
            tokens: &lexed.tokens,
            rest: lexed.tokens.iter(),
            builder: Builder::new(),
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
        self.builder
            .build(lexed.tokens.clone(), lexed.trivia.clone(), self.text)
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
    fn position_of(&self, offset: u32) -> Position {
        let end = (offset as usize).min(self.text.len());
        let before = self.text.get(..end).unwrap_or("");
        let line = before.bytes().filter(|byte| *byte == b'\n').count() as u32 + 1;
        let col = before
            .rsplit('\n')
            .next()
            .map_or(1, |tail| tail.chars().count() as u32 + 1);
        Position::at(line, col)
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
    fn pattern(&mut self) -> Result<TokenId, ManifestError> {
        self.builder.start(NodeKind::Pattern);
        let token = match self.peek() {
            TokenKind::Str => self.bump()?,
            TokenKind::Word if self.peek_word() == Some("regex") => {
                self.bump()?;
                match self.peek() {
                    TokenKind::RawStr => self.bump()?,
                    TokenKind::Str => {
                        return Err(ManifestError::RegexNeedsRawString { at: self.at() });
                    }
                    _ => return Err(ManifestError::RegexWithoutRawString { at: self.at() }),
                }
            }
            _ => return Err(self.unexpected("a pattern")),
        };
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
                    self.comma_list("a `\"…\"` pattern", starts_string, Parser::string_item)?;
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

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::indexing_slicing
)]
mod tests {
    use super::super::syntax::{Cst, NodeKind, print_lossless};
    use super::super::{MAX_LIST_ITEMS, MAX_SOURCE_BYTES};
    use super::parse;

    /// The tree of `text`, which must parse.
    fn parsed(text: &str) -> Cst {
        parse(text).expect("the document parses")
    }

    /// The one refusal `text` produces, rendered as the user sees it.
    fn refusal(text: &str) -> String {
        match parse(text) {
            Ok(_) => panic!("expected a refusal for {text:?}"),
            Err(errors) => errors
                .errors()
                .first()
                .map(ToString::to_string)
                .unwrap_or_else(|| "no error".to_owned()),
        }
    }

    /// The node kinds of the top level, in source order.
    fn top(cst: &Cst) -> Vec<NodeKind> {
        cst.child_nodes(cst.root())
            .filter_map(|node| cst.kind(node))
            .collect()
    }

    /// Every node kind of the tree, in pre-order.
    fn kinds(cst: &Cst) -> Vec<NodeKind> {
        let mut out = Vec::new();
        let mut stack = vec![cst.root()];
        while let Some(node) = stack.pop() {
            out.push(cst.kind(node).expect("a node"));
            let mut children: Vec<_> = cst.child_nodes(node).collect();
            children.reverse();
            stack.extend(children);
        }
        out
    }

    /// How many nodes of `kind` the tree holds.
    fn count(cst: &Cst, kind: NodeKind) -> usize {
        kinds(cst).iter().filter(|found| **found == kind).count()
    }

    // ---- one test per production of §6.1 --------------------------------

    #[test]
    fn app_parses() {
        let cst = parsed("app \"shop\"\n");
        assert_eq!(top(&cst), vec![NodeKind::App]);
        let app = cst.app().expect("an app");
        assert_eq!(cst.words(app).collect::<Vec<_>>(), vec!["app"]);
        assert_eq!(cst.strings(app).collect::<Vec<_>>(), vec!["\"shop\""]);
    }

    #[test]
    fn entrypoint_python_parses() {
        let text = "entrypoint python \"shop.wsgi:application\" {\n    preload \"shop.urls\", \"shop.signals\"\n}\n";
        let cst = parsed(text);
        assert_eq!(top(&cst), vec![NodeKind::Entrypoint]);
        let entry = cst.entrypoint().expect("an entrypoint");
        assert_eq!(
            cst.words(entry).collect::<Vec<_>>(),
            vec!["entrypoint", "python"]
        );
        assert_eq!(
            cst.strings(entry).collect::<Vec<_>>(),
            vec!["\"shop.wsgi:application\""]
        );
        let preload = cst
            .first_of_kind(entry, NodeKind::Preload)
            .expect("a preload");
        assert_eq!(
            cst.strings(preload).collect::<Vec<_>>(),
            vec!["\"shop.urls\"", "\"shop.signals\""]
        );
        assert_eq!(print_lossless(&cst), text);
    }

    #[test]
    fn entrypoint_php_parses() {
        let cst = parsed("entrypoint php \"public/index.php\"\n");
        assert_eq!(top(&cst), vec![NodeKind::Entrypoint]);
        let entry = cst.entrypoint().expect("an entrypoint");
        assert_eq!(
            cst.words(entry).collect::<Vec<_>>(),
            vec!["entrypoint", "php"]
        );
        assert_eq!(
            cst.strings(entry).collect::<Vec<_>>(),
            vec!["\"public/index.php\""]
        );
    }

    #[test]
    fn store_url_parses() {
        let cst = parsed("postgres $DATABASE_URL\n");
        assert_eq!(top(&cst), vec![NodeKind::Store]);
        let store = cst.stores().next().expect("a store");
        assert_eq!(cst.envs(store).collect::<Vec<_>>(), vec!["DATABASE_URL"]);
        assert_eq!(cst.store_slots(store).count(), 0);
    }

    #[test]
    fn store_named_parses() {
        let cst = parsed("postgres analytics $ANALYTICS_URL\n");
        let store = cst.stores().next().expect("a store");
        assert_eq!(
            cst.words(store).collect::<Vec<_>>(),
            vec!["postgres", "analytics"]
        );
        assert_eq!(cst.envs(store).collect::<Vec<_>>(), vec!["ANALYTICS_URL"]);
    }

    #[test]
    fn store_block_parses() {
        let text = "postgres {\n    host $DB_HOST\n    port $DB_PORT\n}\n";
        let cst = parsed(text);
        let store = cst.stores().next().expect("a store");
        let slots: Vec<&str> = cst
            .store_slots(store)
            .flat_map(|slot| cst.words(slot))
            .collect();
        assert_eq!(slots, vec!["host", "port"]);
        let envs: Vec<&str> = cst
            .store_slots(store)
            .flat_map(|slot| cst.envs(slot))
            .collect();
        assert_eq!(envs, vec!["DB_HOST", "DB_PORT"]);
        assert_eq!(print_lossless(&cst), text);
    }

    #[test]
    fn trait_parses() {
        let text = concat!(
            "trait session {\n",
            "    postgres {\n",
            "        transactions\n",
            "        query \"SET TIME ZONE 'UTC'\"\n",
            "    }\n",
            "}\n",
        );
        let cst = parsed(text);
        assert_eq!(top(&cst), vec![NodeKind::Trait]);
        let trait_ = cst.traits().next().expect("a trait");
        assert_eq!(
            cst.words(trait_).collect::<Vec<_>>(),
            vec!["trait", "session"]
        );
        let grant = cst.grants(trait_).next().expect("a grant");
        assert!(cst.first_of_kind(grant, NodeKind::GrantBlock).is_some());
        assert_eq!(count(&cst, NodeKind::Query), 1);
        assert_eq!(print_lossless(&cst), text);
    }

    #[test]
    fn view_header_parses() {
        let text =
            "view products GET, POST \"/products\", \"/products/{id:int}\" {\n    body json\n}\n";
        let cst = parsed(text);
        assert_eq!(top(&cst), vec![NodeKind::View]);
        let view = cst.views().next().expect("a view");
        assert_eq!(
            cst.words(view).collect::<Vec<_>>(),
            vec!["view", "products"]
        );
        let header = cst
            .first_of_kind(view, NodeKind::ViewHeader)
            .expect("a header");
        assert_eq!(cst.words(header).collect::<Vec<_>>(), vec!["GET", "POST"]);
        assert_eq!(cst.patterns(header).count(), 2);
        assert_eq!(cst.endpoints(view).count(), 0);
        assert_eq!(count(&cst, NodeKind::Body), 1);
        assert_eq!(print_lossless(&cst), text);
    }

    #[test]
    fn view_block_parses() {
        let text = concat!(
            "view cart {\n",
            "    GET \"/cart\"\n",
            "    POST \"/cart/items\" { body form }\n",
            "    postgres { select carts(id) }\n",
            "}\n",
        );
        let cst = parsed(text);
        let view = cst.views().next().expect("a view");
        let endpoints: Vec<_> = cst.endpoints(view).collect();
        assert_eq!(endpoints.len(), 2);
        assert_eq!(cst.words(endpoints[0]).collect::<Vec<_>>(), vec!["GET"]);
        assert_eq!(cst.patterns(endpoints[0]).count(), 1);
        let block = cst
            .first_of_kind(endpoints[1], NodeKind::EndpointBlock)
            .expect("an endpoint block");
        assert!(cst.first_of_kind(block, NodeKind::Body).is_some());
        assert_eq!(cst.grants(view).count(), 1);
        assert_eq!(print_lossless(&cst), text);
    }

    #[test]
    fn grant_block_parses() {
        let text = concat!(
            "view catalog GET \"/products/{id:int}\" {\n",
            "    postgres {\n",
            "        transactions\n",
            "        functions st_distance, pg_catalog.unaccent\n",
            "        select products(id, name), categories\n",
            "        insert orders(id, total)\n",
            "        update products(price)\n",
            "        delete archived_products\n",
            "        query 'SELECT count(*) FROM products WHERE price > $?'\n",
            "    }\n",
            "}\n",
        );
        let cst = parsed(text);
        let view = cst.views().next().expect("a view");
        let grant = cst.grants(view).next().expect("a grant");
        let block = cst
            .first_of_kind(grant, NodeKind::GrantBlock)
            .expect("a grant block");
        let directives: Vec<&str> = cst
            .words(block)
            .filter(|word| {
                matches!(
                    *word,
                    "transactions" | "functions" | "select" | "insert" | "update" | "delete"
                )
            })
            .collect();
        assert_eq!(
            directives,
            vec![
                "transactions",
                "functions",
                "select",
                "insert",
                "update",
                "delete"
            ]
        );
        let query = cst.first_of_kind(block, NodeKind::Query).expect("a query");
        assert_eq!(cst.words(query).collect::<Vec<_>>(), vec!["query"]);
        let tables: Vec<_> = cst.of_kind(block, NodeKind::TableGrant).collect();
        assert_eq!(tables.len(), 4);
        assert_eq!(
            cst.words(tables[0]).collect::<Vec<_>>(),
            vec!["products", "id", "name"]
        );
        assert_eq!(count(&cst, NodeKind::Query), 1);
        assert_eq!(print_lossless(&cst), text);
    }

    #[test]
    fn static_parses() {
        let text = concat!(
            "static \"/assets\" {\n",
            "    root \"public/assets\"\n",
            "    index \"index.html\"\n",
            "    fallback \"index.html\"\n",
            "    immutable \"/assets/*\", \"/fonts/*\"\n",
            "}\n",
        );
        let cst = parsed(text);
        assert_eq!(top(&cst), vec![NodeKind::Static]);
        let mount = cst.statics().next().expect("a mount");
        assert_eq!(cst.strings(mount).collect::<Vec<_>>(), vec!["\"/assets\""]);
        let directives: Vec<_> = cst.of_kind(mount, NodeKind::StaticDirective).collect();
        assert_eq!(directives.len(), 4);
        assert_eq!(
            cst.words(directives[3]).collect::<Vec<_>>(),
            vec!["immutable"]
        );
        assert_eq!(
            cst.strings(directives[3]).collect::<Vec<_>>(),
            vec!["\"/assets/*\"", "\"/fonts/*\""]
        );
        assert_eq!(print_lossless(&cst), text);
    }

    #[test]
    fn pattern_forms_parse() {
        let cst = parsed("view v \"/a\", regex '/v[0-9]+/items' {\n    body none\n}\n");
        let view = cst.views().next().expect("a view");
        let header = cst
            .first_of_kind(view, NodeKind::ViewHeader)
            .expect("a header");
        let patterns: Vec<_> = cst.patterns(header).collect();
        assert_eq!(patterns.len(), 2);
        assert_eq!(cst.node_text(patterns[0]), "\"/a\"");
        assert_eq!(cst.node_text(patterns[1]), "regex '/v[0-9]+/items'");
    }

    #[test]
    fn body_forms_parse() {
        for (text, expected) in [
            ("body json", vec!["body", "json"]),
            ("body form", vec!["body", "form"]),
            ("body none", vec!["body", "none"]),
            ("body raw", vec!["body", "raw"]),
            ("body raw unvalidated", vec!["body", "raw", "unvalidated"]),
        ] {
            let source = format!("view v \"/x\" {{\n    {text}\n}}\n");
            let cst = parsed(&source);
            let view = cst.views().next().expect("a view");
            let body = cst.first_of_kind(view, NodeKind::Body).expect("a body");
            assert_eq!(cst.words(body).collect::<Vec<_>>(), expected, "{text}");
        }
    }

    #[test]
    fn use_egress_and_prefork_parse() {
        let cst =
            parsed("view v \"/x\" {\n    use session, catalog\n    egress any\n    prefork\n}\n");
        let view = cst.views().next().expect("a view");
        let use_ = cst.first_of_kind(view, NodeKind::Use).expect("a use");
        assert_eq!(
            cst.words(use_).collect::<Vec<_>>(),
            vec!["use", "session", "catalog"]
        );
        let egress = cst
            .first_of_kind(view, NodeKind::Egress)
            .expect("an egress");
        assert_eq!(cst.words(egress).collect::<Vec<_>>(), vec!["egress", "any"]);
        assert!(cst.first_of_kind(view, NodeKind::Prefork).is_some());
    }

    #[test]
    fn regex_is_reserved_in_a_pattern_list_only() {
        // §6.3's table gives `regex` one "Where" — a pattern list — so it is an
        // ordinary name everywhere else (the table wins over the prose).
        let cst = parsed("view regex \"/x\" { }\npostgres regex $URL\n");
        let view = cst.views().next().expect("a view");
        assert_eq!(cst.words(view).collect::<Vec<_>>(), vec!["view", "regex"]);
        let store = cst.stores().next().expect("a store");
        assert_eq!(
            cst.words(store).collect::<Vec<_>>(),
            vec!["postgres", "regex"]
        );
        assert_eq!(cst.envs(store).collect::<Vec<_>>(), vec!["URL"]);
    }

    #[test]
    fn a_grant_names_a_store() {
        let cst = parsed("view v \"/x\" {\n    postgres analytics unrestricted\n}\n");
        let view = cst.views().next().expect("a view");
        let grant = cst.grants(view).next().expect("a grant");
        assert_eq!(
            cst.words(grant).collect::<Vec<_>>(),
            vec!["postgres", "analytics", "unrestricted"]
        );
    }

    #[test]
    fn an_empty_file_is_just_the_root() {
        let cst = parsed("");
        assert!(top(&cst).is_empty());
        assert!(cst.app().is_none());
        assert!(cst.entrypoint().is_none());
        assert_eq!(cst.views().count(), 0);
    }

    // ---- §6.6: newline and `;` continuation -----------------------------

    #[test]
    fn newlines_and_semicolons_continue() {
        let text = concat!(
            "app \"shop\"; postgres $URL\n",
            "view v GET \"/a\",\n",
            "    \"/b\" {\n",
            "    body json;\n",
            "    use a,\n",
            "        b;\n",
            "    postgres {\n",
            "        select t(\n",
            "            a,\n",
            "            b\n",
            "        )\n",
            "    }\n",
            "}\n",
        );
        let cst = parsed(text);
        assert_eq!(print_lossless(&cst), text);
        let view = cst.views().next().expect("a view");
        let header = cst
            .first_of_kind(view, NodeKind::ViewHeader)
            .expect("a header");
        assert_eq!(cst.patterns(header).count(), 2);
        let use_ = cst.first_of_kind(view, NodeKind::Use).expect("a use");
        assert_eq!(cst.words(use_).collect::<Vec<_>>(), vec!["use", "a", "b"]);
        let grant = cst.grants(view).next().expect("a grant");
        let block = cst
            .first_of_kind(grant, NodeKind::GrantBlock)
            .expect("a block");
        let table = cst
            .of_kind(block, NodeKind::TableGrant)
            .next()
            .expect("a table");
        assert_eq!(cst.words(table).collect::<Vec<_>>(), vec!["t", "a", "b"]);
    }

    #[test]
    fn a_semicolon_does_not_continue_a_statement() {
        // §6.6: only a `Newline` continues after a `,`, a `{` or an unclosed
        // `(`; a `;` always ends the statement.
        assert_eq!(
            refusal("view v {\n  use a,; b\n}\n"),
            "2:8: a list has no trailing comma; delete the `,`"
        );
        assert_eq!(
            refusal("view v {; GET \"/x\" }\n"),
            "1:9: expected a method or a directive, found `;`"
        );
    }

    // ---- §6.7: one case per row, message and `line:col` -----------------

    #[test]
    fn every_refusal_row() {
        let cases: &[(&str, &str)] = &[
            (
                "View products \"/x\" { }\n",
                "1:1: `View` is not a keyword; the sections are `app`, `entrypoint`, `postgres`, `trait`, `view`, `static`",
            ),
            (
                "app \"a\"\napp \"b\"\n",
                "2:1: a file declares one `app`; the first is on line 1",
            ),
            (
                "view v {\n  fallback \"x\"\n}\n",
                "2:3: `fallback` is not a directive here; a view takes `body`, `use`, `egress`, `prefork`, `postgres` and `METHOD \"pattern\"`",
            ),
            (
                "view v {\n  get \"/x\"\n}\n",
                "2:3: `get` is not a method: methods are upper-case, `GET`",
            ),
            (
                "view GET \"/x\" { }\n",
                "1:6: a view name cannot be `GET`; names start with a lower-case letter or a digit",
            ),
            (
                "view v get \"/x\" { }\n",
                "1:8: `get` is not a method: methods are upper-case, `GET`",
            ),
            (
                "view v GET\n",
                "1:11: `view \"v\"` lists methods but no pattern; write the path it answers",
            ),
            (
                "view v { }\n",
                "1:8: view \"v\" has no endpoint: declare at least `GET \"/products\"`",
            ),
            (
                "view p GET \"/p\" {\n  POST \"/x\"\n}\n",
                "2:3: view \"p\" declares its methods in the header; `POST \"/x\"` belongs in a view without one",
            ),
            (
                "postgres $URL { host $H }\n",
                "1:15: `postgres $DATABASE_URL` and a store block are two different forms; write one",
            ),
            (
                "postgres {}\n",
                "1:10: a store block declares at least one slot: `postgres { host $DB_HOST; database $DB_DATABASE }`",
            ),
            (
                "postgres db {\n  url $URL\n}\n",
                "2:3: `url` is the one-line form; write `postgres $DATABASE_URL` (or a named `postgres db $DATABASE_URL`)",
            ),
            (
                "postgres {\n  host $A\n  host $B\n}\n",
                "3:3: `host` is written twice; a store block names each slot once",
            ),
            (
                "trait s {\n  view cart \"/c\"\n}\n",
                "2:3: a trait holds `postgres` blocks only; `view` belongs at the top level",
            ),
            (
                "trait s {\n  egress any\n}\n",
                "2:3: a trait holds `postgres` blocks only; `egress any` belongs on a view",
            ),
            (
                "view v {\n  GET \"/x\" {\n    postgres { select a }\n  }\n}\n",
                "3:5: a grant is per view; write the `postgres` block in the view body, where it applies to every endpoint",
            ),
            (
                "view v {\n  body json unvalidated\n}\n",
                "2:13: `unvalidated` acknowledges `raw` only; `body json` already validates the body",
            ),
            (
                "view v {\n  body\n}\n",
                "2:7: `body` needs a codec: `json`, `form`, `none` or `raw`",
            ),
            (
                "view v {\n  body xml\n}\n",
                "2:8: `xml` is not a codec; the codecs are `json`, `form`, `none`, `raw`",
            ),
            (
                "view v {\n  workers 4\n}\n",
                "2:3: `workers` was removed in 0.2.0: the scheduler owns parallelism, so delete the line and add nothing",
            ),
            (
                "entrypoint python \"x\" {\n  body json\n}\n",
                "2:3: the `entrypoint python` block holds `preload` only",
            ),
            (
                "entrypoint php \"a.php\"\nentrypoint php \"b.php\"\n",
                "2:1: a file declares one `entrypoint`; the first is on line 1",
            ),
            (
                "view v {\n  GET \"/x\"\n}\n}\n",
                "4:1: `}` closes nothing; the last open block is line 1",
            ),
            (
                "view v {\n  body json\n",
                "3:1: the block opened on line 1 is not closed before the end of the file",
            ),
            (
                "view v {\n  use , session\n}\n",
                "2:7: a list cannot start with `,`",
            ),
            (
                "view v {\n  use session,\n}\n",
                "2:14: a list has no trailing comma; delete the `,`",
            ),
            (
                "view _shop \"/x\" { }\n",
                "1:6: `_shop` is not a name; a view, trait or store name is `[a-z0-9][a-z0-9_-]*`",
            ),
            (
                "view v \"/x\" { }\nview v \"/y\" { }\n",
                "2:1: `view \"v\"` is declared twice; the first is on line 1",
            ),
            (
                "view v {\n  body json\n  body form\n}\n",
                "3:3: `body` is declared once per view; the one for a single method goes inside that endpoint",
            ),
            (
                "view v {\n  postgres { select a }\n  postgres { select b }\n}\n",
                "3:3: `postgres` is declared once per store in a view; put both grants in one block",
            ),
            (
                "view v {\n  GET \"/x\" {\n    body json\n    body form\n  }\n}\n",
                "4:5: `body` is declared once per endpoint",
            ),
            (
                "view v {\n  GET \"/x\" {\n    body json\n  }\n  .\n}\n",
                "5:3: `.` is only part of a qualified function name: `functions pg_catalog.st_distance`",
            ),
            (
                "postgres unrestricted\n",
                "1:10: `unrestricted` is a grant keyword, not a store name; write `postgres unrestricted` for the default store",
            ),
            (
                "view v {\n  GET regex \"x\"\n}\n",
                "2:13: `regex` takes a raw string: write `regex '/v[0-9]+/items'`",
            ),
            (
                "view v {\n  GET regex foo\n}\n",
                "2:13: `regex` must be followed by a raw string: `regex '/v[0-9]+/items'`",
            ),
            (
                "view v {\n  body = \"json\"\n}\n",
                "2:8: `=` is not Blastfile syntax; write the value after the keyword: `body json`",
            ),
            (
                "postgres \"postgres://x\"\n",
                "1:10: a quoted string is not a variable name; a store reads `$IDENT`: `postgres $DATABASE_URL`",
            ),
            (
                "postgres {\n  host \"x\"\n}\n",
                "2:8: a quoted string is not a variable name; a store reads `$IDENT`: `postgres $DATABASE_URL`",
            ),
            ("\u{1}b\n", "1:1: byte 0x1 is not Blastfile syntax"),
            ("\"x\"\n", "1:1: expected a section, found a string"),
        ];
        for (text, expected) in cases {
            assert_eq!(refusal(text), *expected, "{text:?}");
        }
    }

    #[test]
    fn a_comma_first_is_refused_at_the_comma() {
        assert_eq!(
            refusal("view v {\n  use , session\n}\n"),
            "2:7: a list cannot start with `,`"
        );
        assert_eq!(
            refusal("view v {\n  postgres {\n    select t(, a)\n  }\n}\n"),
            "3:14: a list cannot start with `,`"
        );
    }

    #[test]
    fn a_trailing_comma_is_refused_at_the_comma() {
        assert_eq!(
            refusal("view v {\n  use session,\n}\n"),
            "2:14: a list has no trailing comma; delete the `,`"
        );
        assert_eq!(
            refusal("view v {\n  GET, \"/x\"\n}\n"),
            "2:6: a list has no trailing comma; delete the `,`"
        );
    }

    #[test]
    fn an_unclosed_block_names_its_opening_line() {
        assert_eq!(
            refusal("view v {\n  body json\n"),
            "3:1: the block opened on line 1 is not closed before the end of the file"
        );
        assert_eq!(
            refusal("trait t {\n  postgres {\n    transactions\n"),
            "4:1: the block opened on line 2 is not closed before the end of the file"
        );
    }

    #[test]
    fn a_third_block_is_refused_at_its_brace() {
        // Two levels parse in both shapes the grammar allows.
        assert!(parse("view v {\n  GET \"/x\" { body json }\n}\n").is_ok());
        assert!(parse("view v {\n  GET \"/x\"\n  postgres { select a }\n}\n").is_ok());
        assert!(parse("trait t {\n  postgres {\n    transactions\n  }\n}\n").is_ok());
        // A third level is unreachable through the grammar — an endpoint block
        // takes `body` only (§6.7) — so the counter is proved directly.
        let text = "{ { {";
        let lexed = super::super::lexer::lex(text).expect("lexes");
        let mut parser = super::Parser::new(text, &lexed);
        parser.open_block().expect("the first block");
        parser.open_block().expect("the second block");
        match parser.open_block() {
            Ok(_) => panic!("expected the depth budget"),
            Err(error) => assert_eq!(
                error.to_string(),
                "1:5: blocks nest two deep at most: a view's `postgres` block is the deepest"
            ),
        }
    }

    #[test]
    fn a_duplicate_section_names_the_first_line() {
        // Every section, with the first declaration on line 1 and again on
        // line 3, so the `first` slot cannot be a constant and `line_of` cannot
        // always answer 1.
        let cases: &[(&str, &str)] = &[
            (
                "app \"a\"\napp \"b\"\n",
                "2:1: a file declares one `app`; the first is on line 1",
            ),
            (
                "# c\n\napp \"a\"\napp \"b\"\n",
                "4:1: a file declares one `app`; the first is on line 3",
            ),
            (
                "entrypoint php \"a.php\"\nentrypoint php \"b.php\"\n",
                "2:1: a file declares one `entrypoint`; the first is on line 1",
            ),
            (
                "# c\n\nentrypoint php \"a.php\"\nentrypoint php \"b.php\"\n",
                "4:1: a file declares one `entrypoint`; the first is on line 3",
            ),
            (
                "view v \"/a\" { }\nview v \"/b\" { }\n",
                "2:1: `view \"v\"` is declared twice; the first is on line 1",
            ),
            (
                "# c\n\nview v \"/a\" { }\nview v \"/b\" { }\n",
                "4:1: `view \"v\"` is declared twice; the first is on line 3",
            ),
            (
                "trait t {\n}\ntrait t {\n}\n",
                "3:1: `trait \"t\"` is declared twice; the first is on line 1",
            ),
            (
                "# c\n\ntrait t {\n}\ntrait t {\n}\n",
                "5:1: `trait \"t\"` is declared twice; the first is on line 3",
            ),
            (
                "postgres db $A\npostgres db $B\n",
                "2:1: `store \"db\"` is declared twice; the first is on line 1",
            ),
            (
                "# c\n\npostgres db $A\npostgres db $B\n",
                "4:1: `store \"db\"` is declared twice; the first is on line 3",
            ),
        ];
        for (text, expected) in cases {
            assert_eq!(refusal(text), *expected, "{text:?}");
        }
    }

    #[test]
    fn a_query_outside_a_grant_block_is_refused() {
        assert_eq!(
            refusal("view v {\n  query 'SELECT 1'\n}\n"),
            "2:3: `query` is not a directive here; a view takes `body`, `use`, `egress`, `prefork`, `postgres` and `METHOD \"pattern\"`"
        );
    }

    #[test]
    fn a_body_on_a_static_mount_is_refused() {
        assert_eq!(
            refusal("static \"/\" {\n  body json\n}\n"),
            "2:3: `body` is not a directive here; a `static` block takes `root`, `index`, `fallback` and `immutable`"
        );
    }

    #[test]
    fn a_refusal_never_echoes_a_value() {
        let message = refusal("postgres \"postgres://user:hunter2@host/db\"\n");
        assert!(!message.contains("hunter2"), "{message}");
        assert!(!message.contains("postgres://"), "{message}");
        let message = refusal("\"hunter2\"\n");
        assert!(!message.contains("hunter2"), "{message}");
        assert!(message.contains("a string"), "{message}");
    }

    // ---- §4.5: the budgets ----------------------------------------------

    #[test]
    fn a_list_holds_at_most_256_items() {
        let list = |count: usize, item: &dyn Fn(usize) -> String| {
            (0..count).map(item).collect::<Vec<_>>().join(", ")
        };
        let documents = |count: usize| {
            vec![
                format!(
                    "view v {} \"/x\" {{ }}\n",
                    list(count, &|i| format!("M{i}"))
                ),
                format!("view v {} {{ }}\n", list(count, &|i| format!("\"/p{i}\""))),
                format!(
                    "view v {{\n  GET \"/x\"\n  postgres {{\n    select t({})\n  }}\n}}\n",
                    list(count, &|i| format!("c{i}"))
                ),
                format!(
                    "entrypoint python \"x\" {{\n  preload {}\n}}\n",
                    list(count, &|i| format!("\"m{i}\""))
                ),
                format!(
                    "static \"/\" {{\n  immutable {}\n}}\n",
                    list(count, &|i| format!("\"/p{i}\""))
                ),
                format!(
                    "view v {{\n  GET \"/x\"\n  postgres {{\n    select {}\n  }}\n}}\n",
                    list(count, &|i| format!("t{i}"))
                ),
            ]
        };
        for text in documents(MAX_LIST_ITEMS) {
            assert!(parse(&text).is_ok(), "{:.80} -> {}", text, refusal(&text));
        }
        for text in documents(MAX_LIST_ITEMS + 1) {
            assert!(
                refusal(&text).ends_with("a list holds at most 256 items"),
                "{:.80}",
                text
            );
        }
    }

    #[test]
    fn a_wall_of_braces_is_refused_without_recursion() {
        let wall = "{".repeat(100_000);
        assert_eq!(refusal(&wall), "1:1: expected a section, found `{`");
        let nested = format!("view v {{ {}", "postgres { ".repeat(10_000));
        assert!(
            refusal(&nested).contains("is not a directive here"),
            "{}",
            refusal(&nested)
        );
        let views = "view v { ".repeat(10_000);
        assert!(
            refusal(&views).contains("is not a directive here"),
            "{}",
            refusal(&views)
        );
    }

    #[test]
    fn a_quarter_megabyte_parses() {
        let mut text = String::new();
        let mut count = 0;
        while text.len() < 256 * 1024 {
            text.push_str(&format!(
                "view v{count} GET \"/p{count}\" {{\n  body json\n}}\n"
            ));
            count += 1;
        }
        let cst = parsed(&text);
        assert_eq!(cst.views().count(), count);
        assert_eq!(print_lossless(&cst), text);
    }

    #[test]
    fn the_size_cap_is_checked_before_lexing() {
        let over = "a".repeat(MAX_SOURCE_BYTES + 1);
        assert_eq!(
            refusal(&over),
            format!(
                "the file is {} bytes; Blastfile is limited to 1048576 bytes — split the grants into a `trait`, or delete a generated block",
                MAX_SOURCE_BYTES + 1
            )
        );
        assert!(parse(&" ".repeat(MAX_SOURCE_BYTES)).is_ok());
    }
}
