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
