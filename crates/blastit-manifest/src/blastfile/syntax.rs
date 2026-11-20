//! The one tree every reader walks: the lossless CST arena (§14.1).
//!
//! A [`Cst`] is an arena of [`Element`]s in source order. A node names the
//! range of its children, and a token or trivia element names a byte span of
//! the source the tree owns. The parser builds it in one pre-order pass, so a
//! node's children are contiguous and ordered and a walk from [`Cst::root`]
//! visits every element exactly once. Every source byte is in exactly one
//! token or trivia span, which is what makes [`print_lossless`] return the
//! original text byte for byte (P1, §14.3).
//!
//! There is no second AST: `lower` and the printer
//! read this arena through the accessors below, and the node
//! kinds are exactly §6.4's list. The accessors return node ids, never a
//! parallel struct, so the tree `lower` reads is the tree `print` writes.

use std::collections::BTreeMap;
use std::ops::Range;

use super::pattern::Compiled;
use super::tokens::{Token, TokenKind, Trivia, TriviaKind};

/// An index into [`Cst`]'s element arena.
pub(crate) type NodeId = u32;

/// An index into [`Cst`]'s token table.
pub(crate) type TokenId = u32;

/// An index into [`Cst`]'s trivia table.
pub(crate) type TriviaId = u32;

/// One production of §6.1, as a node of the tree (§6.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NodeKind {
    /// `file`: the root, holding every top-level section.
    File,
    /// `app`
    App,
    /// `entrypoint`
    Entrypoint,
    /// The `preload` directive of an `entrypoint python` block.
    Preload,
    /// `store`
    Store,
    /// `store_block`
    StoreBlock,
    /// `slot`
    Slot,
    /// `trait`
    Trait,
    /// `view`
    View,
    /// `view_header`: the methods and patterns written on the `view` line.
    ViewHeader,
    /// `endpoint`
    Endpoint,
    /// `endpoint_block`
    EndpointBlock,
    /// `body`
    Body,
    /// `use`
    Use,
    /// `egress`
    Egress,
    /// `prefork`
    Prefork,
    /// `store_grant`
    Grant,
    /// `grant_block`
    GrantBlock,
    /// `table_grant`
    TableGrant,
    /// `static`
    Static,
    /// `static_directive`
    StaticDirective,
    /// `pattern`
    Pattern,
    /// `query`
    Query,
}

/// One element of the arena (§14.1).
///
/// A node names the half-open range of its children; a token or trivia element
/// names its entry in the tree's own table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Element {
    /// A node: its kind and the range of its children in the arena.
    Node {
        /// The production this node is.
        kind: NodeKind,
        /// Index of the node's first child.
        first: u32,
        /// One past the index of the node's last child.
        last: u32,
    },
    /// A token, by index into the tree's token table.
    Token(TokenId),
    /// A trivia run, by index into the tree's trivia table.
    Trivia(TriviaId),
}

/// The one tree: an arena of elements, the source it was built from, and the
/// tokens and trivia the elements name (§14.1).
///
/// The source is owned, so [`Cst::text`] can hand back the exact bytes the
/// tree spans without a lifetime on the type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Cst {
    /// Every element, in source order.
    elements: Vec<Element>,
    /// Every token, in source order.
    tokens: Vec<Token>,
    /// Every trivia run, in source order.
    trivia: Vec<Trivia>,
    /// The `File` node.
    root: NodeId,
    /// The compiled form of every `pattern`, by its token (§8.1).
    ///
    /// A pattern is compiled while it is parsed, so a bad one is refused at its
    /// own `line:col` and `lower` reads the fragment instead of recompiling it.
    patterns: BTreeMap<TokenId, Compiled>,
    /// The source the spans index.
    text: String,
}

impl Cst {
    /// The source the tree was built from, byte for byte.
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// The `File` node.
    pub(crate) fn root(&self) -> NodeId {
        self.root
    }

    /// What a node is, or `None` when `node` names no node.
    pub(crate) fn kind(&self, node: NodeId) -> Option<NodeKind> {
        match self.elements.get(node as usize) {
            Some(Element::Node { kind, .. }) => Some(*kind),
            _ => None,
        }
    }

    /// The elements of `node`, in source order.
    pub(crate) fn children(&self, node: NodeId) -> impl Iterator<Item = Element> + '_ {
        self.elements
            .get(self.child_range(node))
            .unwrap_or(&[])
            .iter()
            .copied()
    }

    /// The child nodes of `node`, in source order.
    #[allow(dead_code)] // the editor walks it
    pub(crate) fn child_nodes(&self, node: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        let range = self.child_range(node);
        let start = range.start;
        self.elements
            .get(range)
            .unwrap_or(&[])
            .iter()
            .enumerate()
            .filter(|(_, element)| matches!(element, Element::Node { .. }))
            .map(move |(offset, _)| (start + offset) as u32)
    }

    /// The child nodes of `node` that are `kind`, in source order.
    pub(crate) fn of_kind(
        &self,
        node: NodeId,
        kind: NodeKind,
    ) -> impl Iterator<Item = NodeId> + '_ {
        let range = self.child_range(node);
        let start = range.start;
        self.elements
            .get(range)
            .unwrap_or(&[])
            .iter()
            .enumerate()
            .filter_map(move |(offset, element)| match element {
                Element::Node { kind: found, .. } if *found == kind => {
                    Some((start + offset) as u32)
                }
                _ => None,
            })
    }

    /// The first child node of `node` that is `kind`.
    pub(crate) fn first_of_kind(&self, node: NodeId, kind: NodeKind) -> Option<NodeId> {
        self.of_kind(node, kind).next()
    }

    /// The elements of `node` with their arena index, in source order.
    ///
    /// The index of an [`Element::Node`] is that node's id, so a walk that has
    /// to keep tokens and child nodes in one order — `lower`'s grant block, whose
    /// clause keyword and its items are siblings — can use it.
    pub(crate) fn child_elements(
        &self,
        node: NodeId,
    ) -> impl Iterator<Item = (NodeId, Element)> + '_ {
        let range = self.child_range(node);
        let start = range.start as u32;
        self.elements
            .get(range)
            .unwrap_or(&[])
            .iter()
            .enumerate()
            .map(move |(offset, element)| (start + offset as u32, *element))
    }

    /// The `app` section, when the file declares one.
    pub(crate) fn app(&self) -> Option<NodeId> {
        self.first_of_kind(self.root, NodeKind::App)
    }

    /// The `entrypoint` section, when the file declares one.
    pub(crate) fn entrypoint(&self) -> Option<NodeId> {
        self.first_of_kind(self.root, NodeKind::Entrypoint)
    }

    /// Every top-level `postgres` store, in source order.
    pub(crate) fn stores(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.of_kind(self.root, NodeKind::Store)
    }

    /// Every top-level `trait`, in source order.
    pub(crate) fn traits(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.of_kind(self.root, NodeKind::Trait)
    }

    /// Every top-level `view`, in source order.
    pub(crate) fn views(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.of_kind(self.root, NodeKind::View)
    }

    /// Every top-level `static` mount, in source order.
    pub(crate) fn statics(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.of_kind(self.root, NodeKind::Static)
    }

    /// The `slot` nodes of a store, in source order: none for the one-line
    /// `postgres $IDENT` form.
    pub(crate) fn store_slots(&self, store: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.first_of_kind(store, NodeKind::StoreBlock)
            .into_iter()
            .flat_map(move |block| self.of_kind(block, NodeKind::Slot))
    }

    /// The `endpoint` nodes of a view, in source order: none for the header
    /// form, whose endpoints are the header's patterns.
    pub(crate) fn endpoints(&self, view: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.of_kind(view, NodeKind::Endpoint)
    }

    /// The `pattern` nodes directly under `node`, in source order.
    pub(crate) fn patterns(&self, node: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.of_kind(node, NodeKind::Pattern)
    }

    /// The `store_grant` nodes directly under `node`, in source order.
    pub(crate) fn grants(&self, node: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.of_kind(node, NodeKind::Grant)
    }

    /// The child tokens of `node`, in source order.
    pub(crate) fn child_tokens(&self, node: NodeId) -> impl Iterator<Item = TokenId> + '_ {
        self.children(node).filter_map(|element| match element {
            Element::Token(id) => Some(id),
            _ => None,
        })
    }

    /// The `Word` tokens directly under `node`, as written.
    pub(crate) fn words(&self, node: NodeId) -> impl Iterator<Item = &str> + '_ {
        self.child_tokens(node)
            .filter(|token| matches!(self.token_kind(*token), Some(TokenKind::Word)))
            .map(|token| self.token_text(token))
    }

    /// The `Str` and `RawStr` tokens directly under `node`, quotes included.
    pub(crate) fn strings(&self, node: NodeId) -> impl Iterator<Item = &str> + '_ {
        self.child_tokens(node)
            .filter(|token| {
                matches!(
                    self.token_kind(*token),
                    Some(TokenKind::Str | TokenKind::RawStr)
                )
            })
            .map(|token| self.token_text(token))
    }

    /// The `Env` tokens directly under `node`, without their `$`.
    pub(crate) fn envs(&self, node: NodeId) -> impl Iterator<Item = &str> + '_ {
        self.child_tokens(node)
            .filter(|token| matches!(self.token_kind(*token), Some(TokenKind::Env)))
            .map(|token| self.token_text(token).trim_start_matches('$'))
    }

    /// What a token is, or `None` when `token` names no token.
    pub(crate) fn token_kind(&self, token: TokenId) -> Option<TokenKind> {
        self.tokens.get(token as usize).map(|token| token.kind)
    }

    /// What a trivia run is, or `None` when `trivia` names no run.
    pub(crate) fn trivia_kind(&self, trivia: TriviaId) -> Option<TriviaKind> {
        self.trivia.get(trivia as usize).map(|trivia| trivia.kind)
    }

    /// The source text a trivia run spans: the spaces, or the `#` and the rest
    /// of the comment line.
    pub(crate) fn trivia_text(&self, trivia: TriviaId) -> &str {
        self.trivia
            .get(trivia as usize)
            .and_then(|trivia| self.slice(trivia.span.clone()))
            .unwrap_or("")
    }

    /// The byte range a trivia run spans.
    pub(crate) fn trivia_span(&self, trivia: TriviaId) -> Option<Range<u32>> {
        self.trivia
            .get(trivia as usize)
            .map(|trivia| trivia.span.clone())
    }

    /// The compiled form of the `pattern` node `node` (§8.1).
    ///
    /// `None` when `node` is not a pattern node. The fragment is what a
    /// `Route.path` entry holds; the text is the value it was compiled from,
    /// decoded for `"…"` (§5.1), so a reader never re-decodes or recompiles.
    pub(crate) fn pattern(&self, node: NodeId) -> Option<&Compiled> {
        let token = self.child_tokens(node).find(|token| {
            matches!(
                self.token_kind(*token),
                Some(TokenKind::Str | TokenKind::RawStr)
            )
        })?;
        self.patterns.get(&token)
    }

    /// The source text a token spans.
    pub(crate) fn token_text(&self, token: TokenId) -> &str {
        self.tokens
            .get(token as usize)
            .and_then(|token| self.slice(token.span.clone()))
            .unwrap_or("")
    }

    /// The source text a token spans, as a byte range.
    pub(crate) fn token_span(&self, token: TokenId) -> Option<Range<u32>> {
        self.tokens
            .get(token as usize)
            .map(|token| token.span.clone())
    }

    /// The first token of `node`'s subtree.
    pub(crate) fn first_token(&self, node: NodeId) -> Option<TokenId> {
        let mut current = node as usize;
        loop {
            let mut next = None;
            for index in self.child_range(current as u32) {
                match self.elements.get(index)? {
                    Element::Token(id) => return Some(*id),
                    Element::Trivia(_) => {}
                    Element::Node { .. } => {
                        next = Some(index);
                        break;
                    }
                }
            }
            current = next?;
        }
    }

    /// The last token of `node`'s subtree.
    pub(crate) fn last_token(&self, node: NodeId) -> Option<TokenId> {
        let mut current = node as usize;
        loop {
            let mut next = None;
            for index in self.child_range(current as u32).rev() {
                match self.elements.get(index)? {
                    Element::Token(id) => return Some(*id),
                    Element::Trivia(_) => {}
                    Element::Node { .. } => {
                        next = Some(index);
                        break;
                    }
                }
            }
            current = next?;
        }
    }

    /// The source text `node`'s subtree spans.
    #[allow(dead_code)] // the editor rewrites a node in place
    pub(crate) fn node_text(&self, node: NodeId) -> &str {
        let start = self.first_token(node).and_then(|id| self.token_span(id));
        let end = self.last_token(node).and_then(|id| self.token_span(id));
        match (start, end) {
            (Some(start), Some(end)) => self.slice(start.start..end.end).unwrap_or(""),
            _ => "",
        }
    }

    /// The half-open range of `node`'s children, or an empty range.
    fn child_range(&self, node: NodeId) -> Range<usize> {
        match self.elements.get(node as usize) {
            Some(Element::Node { first, last, .. }) => *first as usize..*last as usize,
            _ => 0..0,
        }
    }

    /// The source text of a byte range, or `None` when it is not on a char
    /// boundary.
    fn slice(&self, span: Range<u32>) -> Option<&str> {
        self.text.get(span.start as usize..span.end as usize)
    }
}

/// Builds one [`Cst`] while the parser walks the token stream.
///
/// The builder keeps the tree as nodes with item lists, then seals it in two
/// passes: one run of children per node, laid out contiguously, so a node's
/// `first..last` is exactly its direct children and a walk from the root visits
/// every element once. Nodes are created in pre-order, so a parent's id is
/// always lower than its children's.
#[derive(Debug, Default)]
pub(crate) struct Builder {
    /// Every node, in creation order; node `0` is the root.
    nodes: Vec<TempNode>,
    /// The nodes currently open, innermost last.
    open: Vec<usize>,
}

/// One node before the arena is laid out.
#[derive(Debug)]
struct TempNode {
    /// The production this node is.
    kind: NodeKind,
    /// The node's children, in source order.
    items: Vec<Item>,
}

/// One child of a node before the arena is laid out.
#[derive(Clone, Copy, Debug)]
enum Item {
    /// A token, by index into the tree's token table.
    Token(TokenId),
    /// A trivia run, by index into the tree's trivia table.
    Trivia(TriviaId),
    /// A child node, by its index in [`Builder::nodes`].
    Node(usize),
}

impl Builder {
    /// A builder with an empty tree.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Opens a node; its children follow until [`Builder::finish`].
    pub(crate) fn start(&mut self, kind: NodeKind) {
        self.nodes.push(TempNode {
            kind,
            items: Vec::new(),
        });
        self.open.push(self.nodes.len().saturating_sub(1));
    }

    /// Closes the innermost open node, attaching it to its parent.
    pub(crate) fn finish(&mut self) {
        let Some(id) = self.open.pop() else {
            return;
        };
        if let Some(parent) = self.open.last().copied()
            && let Some(node) = self.nodes.get_mut(parent)
        {
            node.items.push(Item::Node(id));
        }
    }

    /// Appends one token element to the innermost open node.
    pub(crate) fn token(&mut self, token: TokenId) {
        if let Some(node) = self.open.last().and_then(|id| self.nodes.get_mut(*id)) {
            node.items.push(Item::Token(token));
        }
    }

    /// Appends one trivia element to the innermost open node.
    pub(crate) fn trivia(&mut self, trivia: TriviaId) {
        if let Some(node) = self.open.last().and_then(|id| self.nodes.get_mut(*id)) {
            node.items.push(Item::Trivia(trivia));
        }
    }

    /// Seals the tree: one contiguous child run per node, then the tables.
    pub(crate) fn build(
        self,
        tokens: Vec<Token>,
        trivia: Vec<Trivia>,
        patterns: BTreeMap<TokenId, Compiled>,
        text: &str,
    ) -> Cst {
        // Where each node's child run starts. Element 0 is the root itself, so
        // the first run starts at 1.
        let mut runs = Vec::with_capacity(self.nodes.len());
        let mut cursor = 1_usize;
        for node in &self.nodes {
            runs.push(cursor);
            cursor = cursor.saturating_add(node.items.len());
        }
        let mut elements: Vec<Element> = Vec::with_capacity(cursor);
        let root = self.nodes.first();
        let root_run = runs.first().copied().unwrap_or(0);
        elements.push(Element::Node {
            kind: root.map_or(NodeKind::File, |node| node.kind),
            first: root_run as u32,
            last: root_run.saturating_add(root.map_or(0, |node| node.items.len())) as u32,
        });
        for node in &self.nodes {
            for item in &node.items {
                let element = match item {
                    Item::Token(token) => Element::Token(*token),
                    Item::Trivia(trivia) => Element::Trivia(*trivia),
                    Item::Node(child) => {
                        let start = runs.get(*child).copied().unwrap_or(0);
                        let len = self.nodes.get(*child).map_or(0, |node| node.items.len());
                        Element::Node {
                            kind: self
                                .nodes
                                .get(*child)
                                .map_or(NodeKind::File, |node| node.kind),
                            first: start as u32,
                            last: start.saturating_add(len) as u32,
                        }
                    }
                };
                elements.push(element);
            }
        }
        Cst {
            elements,
            tokens,
            trivia,
            root: 0,
            patterns,
            text: text.to_owned(),
        }
    }
}

/// The source, rebuilt from the tree: every token's leading trivia, then the
/// token, in source order (P1, §14.3).
///
/// This is a byte-for-byte concatenation with no layout logic, so it proves the
/// tree lost nothing: `print_lossless(&parse(text)?) == text` for every
/// parseable text.
#[allow(dead_code)] // the P1 module tests are its only reader (§14.3)
pub(crate) fn print_lossless(cst: &Cst) -> String {
    let mut out = String::with_capacity(cst.text.len());
    let mut stack: Vec<Element> = cst.children(cst.root).collect();
    stack.reverse();
    while let Some(element) = stack.pop() {
        match element {
            Element::Token(id) => out.push_str(cst.token_text(id)),
            Element::Trivia(id) => {
                if let Some(trivia) = cst.trivia.get(id as usize)
                    && let Some(text) = cst.slice(trivia.span.clone())
                {
                    out.push_str(text);
                }
            }
            Element::Node { first, last, .. } => {
                for index in (first..last).rev() {
                    if let Some(child) = cst.elements.get(index as usize) {
                        stack.push(*child);
                    }
                }
            }
        }
    }
    out
}
