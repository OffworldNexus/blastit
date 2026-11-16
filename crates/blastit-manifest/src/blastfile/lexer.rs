//! The `Blastfile` lexer: one byte-indexed pass (§5.1–§5.4).
//!
//! The lexer never decides syntax. It emits [`TokenKind::Word`] for any bare
//! run and lets the parser classify it, emits [`TokenKind::Equals`] and
//! [`TokenKind::Unknown`] only so the parser can refuse them with §5.3's
//! sentences, and keeps every space and comment as trivia, so the printer can
//! return an untouched file byte for byte.
//!
//! Every scan walks `char_indices()` of a shrinking `&str`, and every cursor
//! advance is derived from the iterator's remaining slice: there is no `i += 1`
//! anywhere, so no mutant can freeze a scan and no loop can run past the input
//! (`LESSONS.md`, `MB.1.4`). [`MAX_TOKENS`] and [`MAX_TRIVIA`] are checked in
//! the one [`Lexer::push`] and [`Lexer::push_trivia`] every item goes through.

use std::ops::Range;

use super::tokens::{Lexed, Token, TokenKind, Trivia, TriviaKind};
use super::{MAX_TOKENS, MAX_TRIVIA};
use crate::error::{ManifestError, Position};

/// Lexes one `Blastfile` into its tokens and trivia.
///
/// # Errors
///
/// The first refusal of §5.3, at the offending token's `line:col`, or
/// [`ManifestError::TooManyTokens`]/[`ManifestError::TooManyTrivia`] when the
/// stream passes §4.5's caps.
pub(crate) fn lex(text: &str) -> Result<Lexed, ManifestError> {
    Lexer::new(text).run()
}

/// The two budgets one lexing pass enforces (§4.5).
///
/// Production has exactly one instance, [`Caps::PRODUCTION`], built from
/// [`MAX_TOKENS`] and [`MAX_TRIVIA`]. The unit tests lower it so a cap witness
/// trips the real `push`/`push_trivia` check with a handful of items instead of
/// allocating in proportion to the production budget; the refusal sentence
/// always names the production budget, and
/// `tests/critical_blastfile_lexer.rs` proves that boundary end to end.
#[derive(Clone, Copy)]
struct Caps {
    /// Most tokens this pass may push, excluding `Eof`.
    tokens: usize,
    /// Most trivia runs this pass may push.
    trivia: usize,
}

impl Caps {
    /// §4.5's budgets, which production always uses.
    const PRODUCTION: Self = Self {
        tokens: MAX_TOKENS,
        trivia: MAX_TRIVIA,
    };
}

/// One pass over one source: the cursor, the position, and the two buffers.
struct Lexer<'a> {
    /// The whole source, kept so a cursor position can be derived from it.
    text: &'a str,
    /// Byte offset of the next unconsumed character.
    at: usize,
    /// 1-based line of the next unconsumed character.
    line: u32,
    /// 1-based column of the next unconsumed character, counted in `char`s.
    col: u32,
    /// Every token so far, in source order.
    tokens: Vec<Token>,
    /// Every trivia run so far, in source order.
    trivia: Vec<Trivia>,
    /// Index of the first trivia node no token has claimed yet.
    leading_start: usize,
    /// The budgets this pass enforces (§4.5).
    caps: Caps,
}

impl<'a> Lexer<'a> {
    /// A cursor at the start of `text`, with production's budgets.
    fn new(text: &'a str) -> Self {
        Self::with_caps(text, Caps::PRODUCTION)
    }

    /// A cursor at the start of `text`, with `caps` (§4.5).
    ///
    /// Only the unit tests pass anything but [`Caps::PRODUCTION`].
    fn with_caps(text: &'a str, caps: Caps) -> Self {
        Self {
            text,
            at: 0,
            line: 1,
            col: 1,
            tokens: Vec::new(),
            trivia: Vec::new(),
            leading_start: 0,
            caps,
        }
    }

    /// Lexes until the input ends, then appends `Eof`.
    fn run(mut self) -> Result<Lexed, ManifestError> {
        loop {
            self.trivia()?;
            let start = self.at;
            let Some(c) = self.peek() else {
                break;
            };
            match c {
                '\n' => {
                    self.bump();
                    self.push(TokenKind::Newline, start)?;
                }
                '\r' => self.carriage_return(start)?,
                '{' => self.one(TokenKind::LBrace, start)?,
                '}' => self.one(TokenKind::RBrace, start)?,
                '(' => self.one(TokenKind::LParen, start)?,
                ')' => self.one(TokenKind::RParen, start)?,
                ',' => self.one(TokenKind::Comma, start)?,
                ';' => self.one(TokenKind::Semi, start)?,
                '=' => self.one(TokenKind::Equals, start)?,
                '.' => self.one(TokenKind::Dot, start)?,
                '"' => self.quoted('"', start)?,
                '\'' => self.quoted('\'', start)?,
                '$' => self.env(start)?,
                '-' => {
                    return Err(ManifestError::BareHyphen {
                        at: self.position(),
                    });
                }
                c if is_word_start(c) => self.word(start)?,
                c if !c.is_ascii() => {
                    return Err(ManifestError::NonAsciiByte {
                        at: self.position(),
                        byte: leading_byte(c),
                    });
                }
                c => {
                    self.bump();
                    self.push(TokenKind::Unknown(c as u8), start)?;
                }
            }
        }
        self.push(TokenKind::Eof, self.at)?;
        Ok(Lexed {
            tokens: self.tokens,
            trivia: self.trivia,
        })
    }

    /// The source from the cursor on.
    fn rest(&self) -> &'a str {
        self.text.get(self.at..).unwrap_or("")
    }

    /// The next unconsumed character, without consuming it.
    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    /// The cursor's position, 1-based.
    fn position(&self) -> Position {
        Position::at(self.line, self.col)
    }

    /// Consumes one character, keeping `line` and `col` current.
    ///
    /// The new offset is derived from the iterator's remaining slice rather
    /// than added to the old one, so the step is the iterator itself.
    fn bump(&mut self) -> Option<char> {
        let mut chars = self.rest().chars();
        let c = chars.next()?;
        self.at = self.text.len() - chars.as_str().len();
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    /// Consumes `run`, a prefix of [`Self::rest`] that contains no newline.
    fn advance(&mut self, run: &str) {
        self.col += run.chars().count() as u32;
        let remaining = self.rest().get(run.len()..).unwrap_or("");
        self.at = self.text.len() - remaining.len();
    }

    /// Pushes one token, checking §4.5's token cap.
    ///
    /// `Eof` is exempt: the cap bounds the file's tokens, and the parser needs
    /// its terminator whatever the count.
    fn push(&mut self, kind: TokenKind, start: usize) -> Result<(), ManifestError> {
        if kind != TokenKind::Eof && self.tokens.len() >= self.caps.tokens {
            return Err(ManifestError::TooManyTokens {
                tokens: self.tokens.len() + 1,
            });
        }
        let leading = self.leading_start as u32..self.trivia.len() as u32;
        self.tokens.push(Token {
            kind,
            span: span(start, self.at),
            leading,
        });
        self.leading_start = self.trivia.len();
        Ok(())
    }

    /// Pushes one trivia node, checking §4.5's trivia cap.
    fn push_trivia(&mut self, kind: TriviaKind, start: usize) -> Result<(), ManifestError> {
        if self.trivia.len() >= self.caps.trivia {
            return Err(ManifestError::TooManyTrivia {
                runs: self.trivia.len() + 1,
            });
        }
        self.trivia.push(Trivia {
            kind,
            span: span(start, self.at),
        });
        Ok(())
    }

    /// Consumes every space run and comment before the next token.
    fn trivia(&mut self) -> Result<(), ManifestError> {
        loop {
            match self.peek() {
                Some(' ') | Some('\t') => {
                    let start = self.at;
                    let run = space_run(self.rest());
                    self.advance(run);
                    self.push_trivia(TriviaKind::Space, start)?;
                }
                Some('#') => {
                    let start = self.at;
                    let run = comment_run(self.rest());
                    self.advance(run);
                    self.push_trivia(TriviaKind::Comment, start)?;
                }
                _ => return Ok(()),
            }
        }
    }

    /// Consumes one one-byte token.
    fn one(&mut self, kind: TokenKind, start: usize) -> Result<(), ManifestError> {
        self.bump();
        self.push(kind, start)
    }

    /// Consumes a `\n`, or a `\r\n` as one token, and refuses a lone `\r`.
    fn carriage_return(&mut self, start: usize) -> Result<(), ManifestError> {
        let at = self.position();
        self.bump();
        if self.peek() == Some('\n') {
            self.bump();
            self.push(TokenKind::Newline, start)
        } else {
            Err(ManifestError::CarriageReturn { at })
        }
    }

    /// Consumes one bare run as a [`TokenKind::Word`].
    fn word(&mut self, start: usize) -> Result<(), ManifestError> {
        let run = word_run(self.rest());
        self.advance(run);
        self.push(TokenKind::Word, start)
    }

    /// Consumes `$IDENT`, or refuses the `$` with §5.3's sentence.
    fn env(&mut self, start: usize) -> Result<(), ManifestError> {
        let at = self.position();
        let rest = self.rest();
        let mut chars = rest.chars();
        chars.next();
        let after = chars.as_str();
        match after.chars().next() {
            Some(c) if c.is_ascii_uppercase() || c == '_' => {
                let run = env_run(rest);
                self.advance(run);
                self.push(TokenKind::Env, start)
            }
            Some(c) if c.is_ascii_lowercase() => Err(ManifestError::EnvName {
                at,
                name: name_run(after).to_string(),
            }),
            _ => Err(ManifestError::EnvMarker { at }),
        }
    }

    /// Consumes one `"…"` or `'…'` string, with `"`'s escapes.
    fn quoted(&mut self, quote: char, start: usize) -> Result<(), ManifestError> {
        let opened = self.position();
        self.bump();
        loop {
            let at = self.position();
            let Some(c) = self.bump() else {
                return Err(unterminated(quote, opened));
            };
            if c == quote {
                break;
            }
            if quote == '"' && c == '\\' {
                let Some(escape) = self.bump() else {
                    return Err(unterminated(quote, opened));
                };
                if !matches!(escape, '"' | '\\' | 'n' | 't') {
                    return Err(ManifestError::InvalidEscape { at, escape });
                }
            }
        }
        let kind = if quote == '"' {
            TokenKind::Str
        } else {
            TokenKind::RawStr
        };
        self.push(kind, start)
    }
}

/// The `unterminated …` refusal for `quote`, naming the opening line (§5.3).
fn unterminated(quote: char, opened: Position) -> ManifestError {
    if quote == '"' {
        ManifestError::UnterminatedString {
            at: opened,
            opened: opened.line(),
        }
    } else {
        ManifestError::UnterminatedRawString {
            at: opened,
            opened: opened.line(),
        }
    }
}

/// True for the bytes a word may start with: a letter, a digit or `_`.
fn is_word_start(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// The first UTF-8 byte of `c`, as it is written in the file.
fn leading_byte(c: char) -> u8 {
    let mut buffer = [0_u8; 4];
    c.encode_utf8(&mut buffer)
        .as_bytes()
        .first()
        .copied()
        .unwrap_or(0)
}

/// A byte range as a token or trivia span.
fn span(start: usize, end: usize) -> Range<u32> {
    start as u32..end as u32
}

/// The run of spaces and tabs at the start of `rest`.
fn space_run(rest: &str) -> &str {
    let mut end = rest.len();
    for (i, c) in rest.char_indices() {
        if !matches!(c, ' ' | '\t') {
            end = i;
            break;
        }
    }
    rest.get(..end).unwrap_or("")
}

/// The `#`-to-end-of-line comment at the start of `rest`, newline excluded.
fn comment_run(rest: &str) -> &str {
    let mut end = rest.len();
    for (i, c) in rest.char_indices() {
        if c == '\n' || c == '\r' {
            end = i;
            break;
        }
    }
    rest.get(..end).unwrap_or("")
}

/// The word at the start of `rest`: `[A-Za-z0-9_-]`, where a `-` belongs to
/// the run only after an alphanumeric (§5.1).
fn word_run(rest: &str) -> &str {
    let mut end = rest.len();
    let mut previous: Option<char> = None;
    for (i, c) in rest.char_indices() {
        let accepted = match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '_' => true,
            '-' => matches!(previous, Some(p) if p.is_ascii_alphanumeric()),
            _ => false,
        };
        if !accepted {
            end = i;
            break;
        }
        previous = Some(c);
    }
    rest.get(..end).unwrap_or("")
}

/// The `$IDENT` at the start of `rest`: the `$` and `[A-Z_][A-Z0-9_]*`.
fn env_run(rest: &str) -> &str {
    let mut end = rest.len();
    for (i, c) in rest.char_indices() {
        let accepted = if i == 0 {
            c == '$'
        } else {
            c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'
        };
        if !accepted {
            end = i;
            break;
        }
    }
    rest.get(..end).unwrap_or("")
}

/// The name-shaped run after a `$`, for §5.3's `$lower` refusal.
fn name_run(rest: &str) -> &str {
    let mut end = rest.len();
    for (i, c) in rest.char_indices() {
        if !(c.is_ascii_alphanumeric() || c == '_') {
            end = i;
            break;
        }
    }
    rest.get(..end).unwrap_or("")
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
    use std::ops::Range;

    use super::super::tokens::{Lexed, Token, TokenKind, TriviaKind};
    use super::super::{MAX_SOURCE_BYTES, MAX_TOKENS, MAX_TRIVIA};
    use super::{Caps, Lexer, lex};
    use crate::error::ManifestError;

    /// The tokens of `text`, which must lex.
    fn tokens(text: &str) -> Vec<Token> {
        lex(text).expect("the source lexes").tokens
    }

    /// The kinds of `text`'s tokens, `Eof` included.
    fn kinds(text: &str) -> Vec<TokenKind> {
        tokens(text).into_iter().map(|token| token.kind).collect()
    }

    /// The source text a span names.
    fn slice<'a>(source: &'a str, span: &Range<u32>) -> &'a str {
        source
            .get(span.start as usize..span.end as usize)
            .expect("a span is inside the source")
    }

    /// The texts of `text`'s tokens, `Eof` included.
    fn texts(source: &str) -> Vec<&str> {
        tokens(source)
            .iter()
            .map(|token| slice(source, &token.span))
            .collect()
    }

    /// The one refusal `text` produces, rendered as the user sees it.
    fn refusal(text: &str) -> String {
        match lex(text) {
            Ok(lexed) => panic!("expected a refusal, lexed {} tokens", lexed.tokens.len()),
            Err(error) => error.to_string(),
        }
    }

    /// Rebuilds the source from the spans, in order: tokens with their leading
    /// trivia. This is the lossless property the printer depends on.
    fn rebuild(source: &str, lexed: &Lexed) -> String {
        let mut out = String::new();
        for token in &lexed.tokens {
            for index in token.leading.clone() {
                let trivia = &lexed.trivia[index as usize];
                out.push_str(slice(source, &trivia.span));
            }
            out.push_str(slice(source, &token.span));
        }
        out
    }

    #[test]
    fn every_token_kind_is_lexed() {
        let source = "view products GET \"/a\" 'raw' $DATABASE_URL { } ( ) , ; = . @\n";
        assert_eq!(
            kinds(source),
            vec![
                TokenKind::Word,
                TokenKind::Word,
                TokenKind::Word,
                TokenKind::Str,
                TokenKind::RawStr,
                TokenKind::Env,
                TokenKind::LBrace,
                TokenKind::RBrace,
                TokenKind::LParen,
                TokenKind::RParen,
                TokenKind::Comma,
                TokenKind::Semi,
                TokenKind::Equals,
                TokenKind::Dot,
                TokenKind::Unknown(b'@'),
                TokenKind::Newline,
                TokenKind::Eof,
            ]
        );
        assert_eq!(
            texts(source),
            vec![
                "view",
                "products",
                "GET",
                "\"/a\"",
                "'raw'",
                "$DATABASE_URL",
                "{",
                "}",
                "(",
                ")",
                ",",
                ";",
                "=",
                ".",
                "@",
                "\n",
                "",
            ]
        );
        let lexed = lex(source).expect("lexes");
        assert_eq!(rebuild(source, &lexed), source);
    }

    #[test]
    fn word_rules() {
        let source = "2fa a-b a-1 x_y";
        assert_eq!(texts(source), vec!["2fa", "a-b", "a-1", "x_y", ""]);
        // A `-` belongs to the run only after an alphanumeric, so a leading
        // hyphen, a doubled one and one after `_` are all §5.3's refusal.
        let hyphen = "`-` is part of a name or a keyword only after a letter or a digit";
        assert_eq!(refusal("-x"), format!("1:1: {hyphen}"));
        assert_eq!(refusal("a--b"), format!("1:3: {hyphen}"));
        assert_eq!(refusal("a -b"), format!("1:3: {hyphen}"));
        assert_eq!(refusal("foo_-bar"), format!("1:5: {hyphen}"));
    }

    #[test]
    fn escaped_strings() {
        let source = r#""a\"b\\c\nd\te""#;
        assert_eq!(kinds(source), vec![TokenKind::Str, TokenKind::Eof]);
        assert_eq!(texts(source), vec![source, ""]);

        // A newline inside a string is allowed, and it moves the cursor.
        let source = "\"a\nb\" $?";
        assert_eq!(
            refusal(source),
            "2:4: `$` must start an environment variable name, `[A-Z_][A-Z0-9_]*`; \
             `$?` is a bind placeholder and belongs inside a `query '…'` string"
        );

        assert_eq!(
            refusal(r#""\q""#),
            "1:2: `\\q` is not an escape; the escapes are `\\\"`, `\\\\`, `\\n`, `\\t`"
        );
        assert_eq!(
            refusal(r#""abc"#),
            "1:1: unterminated string: the opening `\"` is on line 1"
        );
        assert_eq!(
            refusal("x\n\"abc"),
            "2:1: unterminated string: the opening `\"` is on line 2"
        );
        assert_eq!(
            refusal("\"abc\\"),
            "1:1: unterminated string: the opening `\"` is on line 1"
        );
    }

    #[test]
    fn raw_strings() {
        // `#` inside a raw string is data, not a comment.
        let source = "'a # b'";
        assert_eq!(kinds(source), vec![TokenKind::RawStr, TokenKind::Eof]);
        assert_eq!(texts(source), vec!["'a # b'", ""]);

        // §5.1: a raw string has no escapes, so `''` ends it and opens another
        // one. SQL holding a quote goes in a `"…"` string (the reference's
        // "String" row), which is why `'don''t'` is two tokens, not one.
        assert_eq!(texts("'don''t'"), vec!["'don'", "'t'", ""]);

        // A newline inside a raw string is allowed (SQL formatting), and it
        // moves the cursor.
        assert_eq!(
            refusal("'a\nb' $?"),
            "2:4: `$` must start an environment variable name, `[A-Z_][A-Z0-9_]*`; \
             `$?` is a bind placeholder and belongs inside a `query '…'` string"
        );

        assert_eq!(
            refusal("'abc"),
            "1:1: unterminated raw string: the opening `'` is on line 1"
        );
        assert_eq!(
            refusal("x\n'abc"),
            "2:1: unterminated raw string: the opening `'` is on line 2"
        );
    }

    #[test]
    fn env_names() {
        for source in ["$DATABASE_URL", "$A1_B", "$_X"] {
            assert_eq!(
                kinds(source),
                vec![TokenKind::Env, TokenKind::Eof],
                "{source}"
            );
            assert_eq!(texts(source), vec![source, ""], "{source}");
        }

        assert_eq!(
            refusal("$lower"),
            "1:1: `$lower` is not an environment variable name: names are upper-case, \
             `[A-Z_][A-Z0-9_]*`, like `$DATABASE_URL`"
        );
        let marker = "1:1: `$` must start an environment variable name, `[A-Z_][A-Z0-9_]*`; \
                      `$?` is a bind placeholder and belongs inside a `query '…'` string";
        for source in ["$1", "$?", "$", "$-", "$é"] {
            assert_eq!(refusal(source), marker, "{source}");
        }

        // The scan is greedy over `[A-Z0-9_]*`: `$Lower` is `$L` and a word.
        assert_eq!(
            kinds("$Lower"),
            vec![TokenKind::Env, TokenKind::Word, TokenKind::Eof]
        );
        assert_eq!(texts("$Lower"), vec!["$L", "ower", ""]);
    }

    #[test]
    fn carriage_returns() {
        let source = "a\r\nb";
        let lexed = lex(source).expect("lexes");
        assert_eq!(
            lexed
                .tokens
                .iter()
                .map(|token| token.kind)
                .collect::<Vec<_>>(),
            vec![
                TokenKind::Word,
                TokenKind::Newline,
                TokenKind::Word,
                TokenKind::Eof
            ]
        );
        assert_eq!(slice(source, &lexed.tokens[1].span), "\r\n");
        assert_eq!(slice(source, &lexed.tokens[2].span), "b");
        assert_eq!(rebuild(source, &lexed), source);

        // The cursor is on line 2 after the CRLF.
        assert_eq!(
            refusal("a\r\n$?"),
            "2:1: `$` must start an environment variable name, `[A-Z_][A-Z0-9_]*`; \
             `$?` is a bind placeholder and belongs inside a `query '…'` string"
        );
        assert_eq!(
            refusal("a\rb"),
            "1:2: carriage return without a newline is not Blastfile syntax"
        );
    }

    #[test]
    fn non_ascii_bytes() {
        let sentence = "byte 0xc3 is not Blastfile syntax; quote the text, or start a name \
                        with a letter or a digit";
        assert_eq!(refusal("é"), format!("1:1: {sentence}"));
        assert_eq!(refusal("aé"), format!("1:2: {sentence}"));
        // Inside either string form a non-ASCII byte is data.
        assert_eq!(kinds("\"é\""), vec![TokenKind::Str, TokenKind::Eof]);
        assert_eq!(kinds("'é'"), vec![TokenKind::RawStr, TokenKind::Eof]);
    }

    #[test]
    fn control_bytes_are_unknown() {
        // §5.1: any other byte is an `Unknown` token carrying the byte, so the
        // parser can name it in hex at its position.
        let source = "a\u{1}b";
        assert_eq!(
            kinds(source),
            vec![
                TokenKind::Word,
                TokenKind::Unknown(1),
                TokenKind::Word,
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn positions_count_chars() {
        // `"é"` is three chars, so the `$` is at column 5, not byte 6.
        assert_eq!(
            refusal("\"é\" $?"),
            "1:5: `$` must start an environment variable name, `[A-Z_][A-Z0-9_]*`; \
             `$?` is a bind placeholder and belongs inside a `query '…'` string"
        );
    }

    #[test]
    fn trivia_is_kept() {
        let source = "# header\n\nview v {  # trailing\n  body json\n}\n";
        let lexed = lex(source).expect("lexes");
        assert_eq!(rebuild(source, &lexed), source);
        let comments: Vec<&str> = lexed
            .trivia
            .iter()
            .filter(|trivia| trivia.kind == TriviaKind::Comment)
            .map(|trivia| slice(source, &trivia.span))
            .collect();
        assert_eq!(comments, vec!["# header", "# trailing"]);
        // The two spaces before the trailing comment are one Space run.
        assert!(
            lexed
                .trivia
                .iter()
                .any(|trivia| trivia.kind == TriviaKind::Space
                    && slice(source, &trivia.span) == "  ")
        );
        // A blank line is two Newline tokens, not trivia.
        let newlines = lexed
            .tokens
            .iter()
            .filter(|token| token.kind == TokenKind::Newline)
            .count();
        assert_eq!(newlines, 5);
    }

    #[test]
    fn comment_at_eof() {
        let source = "a # end";
        let lexed = lex(source).expect("lexes");
        assert_eq!(rebuild(source, &lexed), source);
        let eof = lexed.tokens.last().expect("an Eof");
        assert_eq!(eof.kind, TokenKind::Eof);
        // Both the space and the comment are the Eof token's leading trivia.
        assert_eq!(eof.leading, 0..2);
    }

    #[test]
    fn empty_input_is_just_eof() {
        let lexed = lex("").expect("lexes");
        assert_eq!(lexed.tokens.len(), 1);
        assert_eq!(lexed.tokens[0].kind, TokenKind::Eof);
        assert!(lexed.trivia.is_empty());
    }

    #[test]
    fn the_parser_rows_are_lexed_for_the_parser() {
        // §5.3's first two rows are the parser's refusals (§5.1): the lexer
        // must emit `Equals` and `Str` so `MB.21.3.2` can name them.
        assert_eq!(
            kinds("body = \"json\""),
            vec![
                TokenKind::Word,
                TokenKind::Equals,
                TokenKind::Str,
                TokenKind::Eof
            ]
        );
        assert_eq!(
            kinds("postgres \"postgres://…\""),
            vec![TokenKind::Word, TokenKind::Str, TokenKind::Eof]
        );
        let lexed = lex("body = \"json\"").expect("lexes");
        assert_eq!(slice("body = \"json\"", &lexed.tokens[1].span), "=");
    }

    #[test]
    fn the_token_cap_trips_while_pushing() {
        // `Caps` lowers the budget so this witness builds a handful of tokens,
        // not 4 194 304 of them; `push` is the same check production runs, and
        // `tests/critical_blastfile_lexer.rs` proves the real boundary end to
        // end (`R3b`). The sentence keeps naming the production budget.
        let caps = Caps {
            tokens: 9,
            trivia: usize::MAX,
        };
        let over = "a ".repeat(caps.tokens + 1);
        match Lexer::with_caps(&over, caps).run() {
            Ok(_) => panic!("expected the token cap"),
            Err(error) => {
                assert_eq!(
                    error.to_string(),
                    format!(
                        "{} tokens; Blastfile is limited to {MAX_TOKENS} — the file is too \
                         large: replace repeated `query '…'` lines with table or column grants \
                         (`select`, `insert`, `update`, `delete`), or fold the shared grants into \
                         a `trait` that many views `use`",
                        caps.tokens + 1
                    )
                );
                assert!(matches!(
                    error,
                    ManifestError::TooManyTokens { tokens } if tokens == caps.tokens + 1
                ));
            }
        }

        // Exactly the cap lexes; `Eof` is not counted against it.
        let at_cap = "a ".repeat(caps.tokens);
        let lexed = Lexer::with_caps(&at_cap, caps)
            .run()
            .expect("at the cap lexes");
        assert_eq!(lexed.tokens.len(), caps.tokens + 1);

        // The production budget is the one `push` reads when nothing lowers it.
        assert_eq!(Caps::PRODUCTION.tokens, MAX_TOKENS);
        assert_eq!(Caps::PRODUCTION.trivia, MAX_TRIVIA);
    }

    #[test]
    fn the_trivia_cap_trips_while_pushing() {
        // A space and one comment per line, so two trivia runs per Newline
        // token and the trivia cap trips before the token cap. Lowered the same
        // way as the token witness: the check is the production one.
        let caps = Caps {
            tokens: usize::MAX,
            trivia: 2 * 9,
        };
        let over = " #x\n".repeat(caps.trivia / 2 + 1);
        match Lexer::with_caps(&over, caps).run() {
            Ok(_) => panic!("expected the trivia cap"),
            Err(error) => {
                assert_eq!(
                    error.to_string(),
                    format!(
                        "{} trivia runs; Blastfile is limited to {MAX_TRIVIA} — the file is too \
                         large: delete the generated comments and blank lines, or fold the shared \
                         grants into a `trait` that many views `use`",
                        caps.trivia + 1
                    )
                );
                assert!(matches!(
                    error,
                    ManifestError::TooManyTrivia { runs } if runs == caps.trivia + 1
                ));
            }
        }
        let at_cap = " #x\n".repeat(caps.trivia / 2);
        assert!(Lexer::with_caps(&at_cap, caps).run().is_ok());
    }

    #[test]
    fn a_hundred_kibibytes_lex_in_one_pass() {
        let line = "view v GET \"/a\" { body json }\n";
        let count = 4_096;
        let source = line.repeat(count);
        assert!(source.len() > 100 * 1024);
        let lexed = lex(&source).expect("lexes");
        assert_eq!(lexed.tokens.len(), 9 * count + 1);
        assert_eq!(lexed.trivia.len(), 7 * count);
        assert_eq!(rebuild(&source, &lexed), source);
    }

    #[test]
    fn a_hostile_source_at_the_size_cap_is_refused_by_the_caps() {
        // The token and trivia walls are the `push` checks at a lowered budget
        // (see `the_token_cap_trips_while_pushing`); a 16 MiB wall of one-byte
        // tokens would cost four million `Token`s to prove the same sentence.
        let wall = Caps {
            tokens: 64,
            trivia: 64,
        };
        assert!(matches!(
            Lexer::with_caps(&"a ".repeat(wall.tokens + 1), wall).run(),
            Err(ManifestError::TooManyTokens { .. })
        ));
        assert!(matches!(
            Lexer::with_caps(&" #x\n".repeat(wall.trivia / 2 + 1), wall).run(),
            Err(ManifestError::TooManyTrivia { .. })
        ));

        // Each of these is one allocation of exactly the size cap, which is the
        // buffer the budget under test is named for.
        let unterminated = format!("\"{}", "a".repeat(MAX_SOURCE_BYTES));
        assert_eq!(
            refusal(&unterminated),
            "1:1: unterminated string: the opening `\"` is on line 1"
        );

        // A whole source of one run is one trivia node, not a megabyte of them.
        let spaces = " ".repeat(MAX_SOURCE_BYTES);
        let lexed = lex(&spaces).expect("one run of spaces");
        assert_eq!(lexed.tokens.len(), 1);
        assert_eq!(lexed.trivia.len(), 1);
        assert_eq!(slice(&spaces, &lexed.trivia[0].span), spaces.as_str());

        let hashes = "#".repeat(MAX_SOURCE_BYTES);
        let lexed = lex(&hashes).expect("one comment");
        assert_eq!(lexed.tokens.len(), 1);
        assert_eq!(lexed.trivia.len(), 1);
        assert_eq!(lexed.trivia[0].kind, TriviaKind::Comment);
    }
}
