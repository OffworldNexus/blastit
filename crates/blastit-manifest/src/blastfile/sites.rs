//! Where each warning's declaration sits in the tree (§5.5).
//!
//! The warning triggers stay in `validate::warnings`, and their sentences stay
//! `error.rs`'s: this module adds one thing, the CST site of each warning, in
//! `warnings`' own order. It exists because the editor has to squiggle the line
//! a developer edits, and the lowered [`crate::model::Manifest`] keeps no span
//! for that.
//!
//! A site is computed where the tree is still in memory — the LSP's
//! [`super::check`] is the only caller that keeps it — so no variant gains a
//! field and no existing constructor changes. A warning whose declaration the
//! walk cannot find gets [`Position::NONE`], which the editor draws at `(0,0)`.
//!
//! The walk is linear in the tree: every `view`, `trait`, `postgres` and
//! `entrypoint` is visited once, and each lookup keys on the name the warning
//! prints, never on a second reading of the source.

use super::lower::DEFAULT_STORE;
use super::parser::line_starts;
use super::syntax::{Cst, NodeId, NodeKind, TokenId};
use super::tokens::{TokenKind, str_value};
use crate::error::{ManifestWarning, Position};

/// The 1-based CST site of every warning, in `warnings`' order (§5.5).
///
/// `sites.len() == warnings.len()` by construction, so a caller zips the two
/// lists and never has to guess which warning lost its position.
pub(crate) fn sites(cst: &Cst, source: &str, warnings: &[ManifestWarning]) -> Vec<Position> {
    let starts = line_starts(source);
    warnings
        .iter()
        .map(|warning| site(cst, source, &starts, warning))
        .collect()
}

/// One warning's site, or [`Position::NONE`] when the declaration is absent.
fn site(cst: &Cst, source: &str, starts: &[u32], warning: &ManifestWarning) -> Position {
    match warning {
        ManifestWarning::UnattachedGroup { group } => cst
            .traits()
            .find(|trait_node| name_of(cst, *trait_node) == Some(group.as_str()))
            .and_then(|trait_node| name_token(cst, trait_node))
            .map_or(Position::NONE, |token| {
                token_position(cst, source, starts, token)
            }),
        ManifestWarning::UngrantedProxy { proxy } => cst
            .stores()
            .find(|store| store_name(cst, *store) == proxy.as_str())
            .map_or(Position::NONE, |store| {
                declaration_site(cst, source, starts, store)
            }),
        ManifestWarning::UnrestrictedRoute { route, proxy } => {
            let Some(view) = view_named(cst, route) else {
                return Position::NONE;
            };
            cst.grants(view)
                .filter(|grant| grant_store(cst, *grant) == proxy.as_str())
                .find_map(|grant| word_token(cst, grant, "unrestricted"))
                .map_or(Position::NONE, |token| {
                    token_position(cst, source, starts, token)
                })
        }
        ManifestWarning::RawIngress { route } => view_named(cst, route)
            .and_then(|view| raw_body(cst, view))
            .and_then(|body| codec_token(cst, body))
            .map_or(Position::NONE, |token| {
                token_position(cst, source, starts, token)
            }),
        ManifestWarning::EgressRoute { route } => view_named(cst, route)
            .and_then(|view| cst.first_of_kind(view, NodeKind::Egress))
            .and_then(|egress| cst.first_token(egress))
            .map_or(Position::NONE, |token| {
                token_position(cst, source, starts, token)
            }),
        ManifestWarning::EntrypointNotReached { entry } => cst
            .entrypoint()
            .filter(|node| {
                cst.strings(*node)
                    .any(|text| str_value(text) == entry.as_str())
            })
            .and_then(|node| cst.first_token(node))
            .map_or(Position::NONE, |token| {
                token_position(cst, source, starts, token)
            }),
    }
}

/// The `view` whose name is `name`.
fn view_named(cst: &Cst, name: &str) -> Option<NodeId> {
    cst.views().find(|view| name_of(cst, *view) == Some(name))
}

/// The name a `view`, `trait` or store writes after its keyword.
fn name_of(cst: &Cst, node: NodeId) -> Option<&str> {
    cst.words(node).nth(1)
}

/// The lowered name of a store node: the written name, or [`DEFAULT_STORE`] for
/// the unnamed `postgres $DATABASE_URL` / `postgres { … }` forms (§7.4).
fn store_name(cst: &Cst, store: NodeId) -> &str {
    cst.words(store).nth(1).unwrap_or(DEFAULT_STORE)
}

/// The lowered name of the store a `postgres` grant names: the written name, or
/// [`DEFAULT_STORE`] for `postgres { … }` and `postgres unrestricted` (§7.4).
fn grant_store(cst: &Cst, grant: NodeId) -> &str {
    match cst.words(grant).nth(1) {
        Some("unrestricted") | None => DEFAULT_STORE,
        Some(name) => name,
    }
}

/// The name token of a node whose first word is its keyword.
fn name_token(cst: &Cst, node: NodeId) -> Option<TokenId> {
    word_tokens(cst, node).nth(1)
}

/// A store's site: its name token, or the `postgres` keyword for the unnamed
/// form — the declaration a developer grants or deletes either way.
fn declaration_site(cst: &Cst, source: &str, starts: &[u32], store: NodeId) -> Position {
    let token = name_token(cst, store).or_else(|| cst.first_token(store));
    token.map_or(Position::NONE, |token| {
        token_position(cst, source, starts, token)
    })
}

/// The `Word` tokens directly under `node`, in source order.
fn word_tokens(cst: &Cst, node: NodeId) -> impl Iterator<Item = TokenId> + '_ {
    cst.child_tokens(node)
        .filter(|token| matches!(cst.token_kind(*token), Some(TokenKind::Word)))
}

/// The token of `word` written directly under `node`.
fn word_token(cst: &Cst, node: NodeId, word: &str) -> Option<TokenId> {
    word_tokens(cst, node).find(|token| cst.token_text(*token) == word)
}

/// The `body` node of a view whose codec is `raw`: the view's own directive
/// first, then the endpoints' own (§10.3).
fn raw_body(cst: &Cst, view: NodeId) -> Option<NodeId> {
    let view_body = cst.first_of_kind(view, NodeKind::Body);
    let candidates = view_body
        .into_iter()
        .chain(cst.endpoints(view).filter_map(|endpoint| {
            cst.first_of_kind(endpoint, NodeKind::EndpointBlock)
                .and_then(|block| cst.first_of_kind(block, NodeKind::Body))
        }));
    candidates
        .into_iter()
        .find(|body| codec_of(cst, *body) == Some("raw"))
}

/// A `body` node's codec as written.
fn codec_of(cst: &Cst, body: NodeId) -> Option<&str> {
    cst.words(body).nth(1)
}

/// The codec token of a `body` node: the word the warning is about.
fn codec_token(cst: &Cst, body: NodeId) -> Option<TokenId> {
    name_token(cst, body)
}

/// The 1-based `line:col` of a token, `col` in `char`s, matching `parser`'s own
/// conversion so an error and a warning can never disagree about a position.
fn token_position(cst: &Cst, source: &str, starts: &[u32], token: TokenId) -> Position {
    token_span_start(cst, token).map_or(Position::NONE, |start| position_of(source, starts, start))
}

/// The byte offset a token starts at.
fn token_span_start(cst: &Cst, token: TokenId) -> Option<u32> {
    cst.token_span(token).map(|span| span.start)
}

/// The 1-based position of a byte offset, `col` in `char`s (§18).
fn position_of(source: &str, starts: &[u32], offset: u32) -> Position {
    let end = (offset as usize).min(source.len());
    // The number of line starts at or before `end` is the 1-based line.
    let line = starts
        .partition_point(|start| *start as usize <= end)
        .max(1);
    let start = starts.get(line.saturating_sub(1)).copied().unwrap_or(0);
    let col = source
        .get(start as usize..end)
        .map_or(1, |tail| tail.chars().count() as u32 + 1);
    Position::at(line as u32, col)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::super::parser::parse;
    use super::sites;
    use crate::error::{ManifestWarning, Position};

    /// Fixture B of §5.4: the four warnings that need views and a trait.
    const VIEWS: &str = concat!(
        "postgres $DATABASE_URL\n",
        "\n",
        "trait session {\n",
        "    postgres { transactions }\n",
        "}\n",
        "\n",
        "view open \"/a\" {\n",
        "    body raw\n",
        "}\n",
        "\n",
        "view wide \"/b\" {\n",
        "    postgres unrestricted\n",
        "}\n",
        "\n",
        "view outbound \"/c\" {\n",
        "    egress any\n",
        "}\n",
    );

    /// Every site of `VIEWS`, from the tree and the names §5.4 prints.
    #[test]
    fn every_warning_of_a_view_file_has_its_line() {
        let cst = parse(VIEWS).expect("the fixture parses");
        let warnings = vec![
            ManifestWarning::UnattachedGroup {
                group: "session".to_owned(),
            },
            ManifestWarning::RawIngress {
                route: "open".to_owned(),
            },
            ManifestWarning::UnrestrictedRoute {
                route: "wide".to_owned(),
                proxy: crate::model::ProxyName::from("db"),
            },
            ManifestWarning::EgressRoute {
                route: "outbound".to_owned(),
            },
        ];
        let found = sites(&cst, VIEWS, &warnings);
        assert_eq!(
            found,
            vec![
                Position::at(3, 7),
                Position::at(8, 10),
                Position::at(12, 14),
                Position::at(16, 5),
            ]
        );
        // Seven fragments, seven distinct sites: a `sites()` that returned one
        // position for every warning would fail this.
        let distinct: std::collections::BTreeSet<(u32, u32)> =
            found.iter().map(|at| (at.line(), at.col())).collect();
        assert_eq!(distinct.len(), found.len());
    }

    /// A warning whose declaration the tree does not contain is
    /// [`Position::NONE`], not the first line.
    #[test]
    fn a_missing_declaration_is_none() {
        let cst = parse(VIEWS).expect("the fixture parses");
        let warnings = vec![
            ManifestWarning::UnattachedGroup {
                group: "nosuchtrait".to_owned(),
            },
            ManifestWarning::UngrantedProxy {
                proxy: crate::model::ProxyName::from("analytics"),
            },
            ManifestWarning::EgressRoute {
                route: "noview".to_owned(),
            },
            ManifestWarning::EntrypointNotReached {
                entry: "public/index.php".to_owned(),
            },
        ];
        assert_eq!(
            sites(&cst, VIEWS, &warnings),
            vec![
                Position::NONE,
                Position::NONE,
                Position::NONE,
                Position::NONE,
            ]
        );
    }

    /// An empty document has no warnings, so it has no sites.
    #[test]
    fn an_empty_document_has_no_sites() {
        let cst = parse("").expect("an empty file parses");
        assert!(sites(&cst, "", &[]).is_empty());
    }

    /// The unnamed `postgres $DATABASE_URL` form is the store `db`, and its site
    /// is its own declaration — the keyword, since it writes no name.
    #[test]
    fn the_unnamed_store_is_located_at_its_keyword() {
        let text = "postgres $DATABASE_URL\n";
        let cst = parse(text).expect("the fixture parses");
        let warnings = vec![ManifestWarning::UngrantedProxy {
            proxy: crate::model::ProxyName::from("db"),
        }];
        assert_eq!(sites(&cst, text, &warnings), vec![Position::at(1, 1)]);
    }
}
