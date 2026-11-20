//! The lexer's token and trivia types (§5.1, §5.2).
//!
//! A token names a byte range of the source, never a copy of it, so the lexer
//! allocates one [`Token`] per token and nothing per byte. [`Token::leading`]
//! names the trivia that preceded it, which is what makes the stream lossless:
//! every byte of the source is in exactly one token span or one trivia span.

use std::ops::Range;

/// What one token is: §5.1's closed set, and nothing else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TokenKind {
    /// One run of `[A-Za-z0-9_-]`, where a `-` belongs to the run only when it
    /// follows an alphanumeric. The parser classifies it as a keyword, a name
    /// or a `Method` (§4.2).
    Word,
    /// An escaped `"…"` string: `\"`, `\\`, `\n` and `\t` only.
    Str,
    /// A raw `'…'` string: every byte verbatim, newlines included, no escapes.
    RawStr,
    /// `$IDENT`, `[A-Z_][A-Z0-9_]*`: a name, never a value (§17.2).
    Env,
    /// `{`
    LBrace,
    /// `}`
    RBrace,
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `,`
    Comma,
    /// `;`
    Semi,
    /// `\n` or `\r\n`, one token: it terminates a statement (§6.6).
    Newline,
    /// `=`, lexed only so the parser can refuse it with §5.3's sentence.
    Equals,
    /// `.`, legal only inside a `functions` name (§6.7).
    Dot,
    /// Any other byte, carried so the parser can name it in hex (§5.1).
    Unknown(u8),
    /// End of input: the parser's only terminating token.
    Eof,
}

/// One token: what it is, where it is, and the trivia before it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Token {
    /// What the token is.
    pub(crate) kind: TokenKind,
    /// The token's byte range in the source.
    pub(crate) span: Range<u32>,
    /// The range of [`Trivia`] indices immediately before this token.
    pub(crate) leading: Range<u32>,
}

/// What one piece of trivia is (§5.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TriviaKind {
    /// A run of spaces and tabs.
    Space,
    /// `#` to the end of the line: the `#` included, the newline not.
    Comment,
}

/// One trivia node: what it is and where it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Trivia {
    /// What the trivia is.
    pub(crate) kind: TriviaKind,
    /// The trivia's byte range in the source.
    pub(crate) span: Range<u32>,
}

/// The lexer's product: an input buffer, not a tree (§5.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Lexed {
    /// Every token in source order, `Eof` last.
    pub(crate) tokens: Vec<Token>,
    /// Every trivia run in source order.
    pub(crate) trivia: Vec<Trivia>,
}

/// The value of a `Str` token: its text with the quotes removed and §5.1's four
/// escapes decoded.
///
/// A token's text is the source spelling, quotes included, because the tree has
/// to print back byte for byte (P1, §14.3). Everything that needs the *value* —
/// a pattern, an `app`, a `query` — calls this, so the one escape table lives
/// here instead of in each caller.
///
/// The lexer refuses every other escape (§5.3), so the fallback arm cannot be
/// reached from a parsed document; it keeps a stray backslash rather than
/// dropping it, because a decoder that silently loses a byte is worse than one
/// that returns it.
pub(crate) fn str_value(text: &str) -> String {
    let body = text
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(text);
    let mut value = String::with_capacity(body.len());
    let mut characters = body.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            value.push(character);
            continue;
        }
        match characters.next() {
            Some('"') => value.push('"'),
            Some('\\') => value.push('\\'),
            Some('n') => value.push('\n'),
            Some('t') => value.push('\t'),
            Some(other) => {
                value.push('\\');
                value.push(other);
            }
            None => value.push('\\'),
        }
    }
    value
}

/// The value of a `RawStr` token: its text with the quotes removed, byte for
/// byte (§5.1).
///
/// A raw string has no escapes, so this only strips the two quotes — which is
/// the point: a backslash reaches the `regex` compiler exactly as written
/// (§17.4).
pub(crate) fn raw_value(text: &str) -> &str {
    text.strip_prefix('\'')
        .and_then(|rest| rest.strip_suffix('\''))
        .unwrap_or(text)
}
