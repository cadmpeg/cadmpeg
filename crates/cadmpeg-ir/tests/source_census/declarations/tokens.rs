// SPDX-License-Identifier: Apache-2.0
//! What a source states in tokens the item walk does not read: the token trees
//! macros read, every trait impl header wherever it sits, and every `use`
//! rename.
//!
//! The census expands no macro. A token tree that names a trait or a generic
//! declaration can expand to an impl or an instantiation no parsed item states,
//! so the census reads the trees themselves and fails a proof that one of them
//! could change.

use proc_macro2::{Delimiter, TokenStream, TokenTree};

/// Keywords that can stand before a `!` that negates a parenthesized operand.
const NEGATING_KEYWORDS: &[&str] = &[
    "if", "while", "match", "return", "in", "else", "let", "mut", "ref", "move", "as", "break",
    "yield", "where", "for", "loop", "unsafe",
];

/// Standard macros that expand to an expression and declare no item.
const EXPRESSION_MACROS: &[&str] = &[
    "assert",
    "assert_eq",
    "assert_ne",
    "cfg",
    "column",
    "compile_error",
    "concat",
    "dbg",
    "debug_assert",
    "debug_assert_eq",
    "debug_assert_ne",
    "env",
    "eprint",
    "eprintln",
    "file",
    "format",
    "format_args",
    "line",
    "matches",
    "module_path",
    "option_env",
    "panic",
    "print",
    "println",
    "stringify",
    "todo",
    "unimplemented",
    "unreachable",
    "vec",
    "write",
    "writeln",
];

/// What reads a token tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TreeKind {
    /// A `macro_rules!` body.
    Definition,
    /// The input of a macro call.
    Call,
    /// An attribute, which a derive or attribute macro reads.
    Attribute,
}

/// One token tree a macro reads.
pub(super) struct MacroTree {
    pub(super) kind: TreeKind,
    /// The defined or called macro; a call through a metavariable keeps its
    /// `$`.
    pub(super) name: String,
    pub(super) line: usize,
    /// Every token of the tree, groups flattened.
    pub(super) tokens: Vec<String>,
    /// The tree itself, delimiters kept.
    pub(super) stream: TokenStream,
    /// Whether the tree sits in a function body.
    pub(super) in_function: bool,
    /// The macros called inside the tree.
    pub(super) calls: Vec<String>,
}

impl MacroTree {
    /// Where the tree is, for an offender line.
    pub(super) fn describe(&self, relative: &str) -> String {
        match self.kind {
            TreeKind::Definition => {
                format!("the macro_rules! {} at {relative}:{}", self.name, self.line)
            }
            TreeKind::Call => format!("the call to {}! at {relative}:{}", self.name, self.line),
            TreeKind::Attribute => format!("the attribute at {relative}:{}", self.line),
        }
    }

    /// The first of `names` the tree spells as a token.
    pub(super) fn names<'a>(&self, names: &'a [String]) -> Option<&'a String> {
        names
            .iter()
            .find(|name| self.tokens.iter().any(|token| token == *name))
    }

    /// Whether the tree's own tokens spell a keyword that binds a name a type
    /// argument can spell: `struct`, `enum`, `union`, `type`, `use`, `mod`,
    /// `extern` or `trait` that is no metavariable.
    pub(super) fn spells_binding(&self) -> bool {
        self.tokens.iter().enumerate().any(|(index, token)| {
            matches!(
                token.as_str(),
                "struct" | "enum" | "union" | "type" | "use" | "mod" | "extern" | "trait"
            ) && (index == 0 || self.tokens[index - 1] != "$")
        })
    }
}

/// A position in a source, as (line, column).
pub(super) type Position = (usize, usize);

/// Where a token starts.
pub(super) fn position(span: proc_macro2::Span) -> Position {
    let start = span.start();
    (start.line, start.column)
}

/// One trait impl header outside a token tree: an `impl` whose tokens up to
/// its body spell `for`.
pub(super) struct ImplHeader {
    /// Every identifier its tokens spell, groups included.
    pub(super) names: Vec<String>,
    /// Where its `impl` keyword starts.
    pub(super) at: Position,
}

impl ImplHeader {
    /// The first of `names` the header spells.
    pub(super) fn names<'a>(&self, names: &'a [String]) -> Option<&'a String> {
        names.iter().find(|name| self.names.contains(name))
    }
}

/// What one source states in tokens.
#[derive(Default)]
pub(super) struct FileTokens {
    pub(super) trees: Vec<MacroTree>,
    pub(super) impl_headers: Vec<ImplHeader>,
    /// Each `original as alias` a `use` declaration states.
    pub(super) renames: Vec<(String, String)>,
}

/// Reads `text` as tokens, skipping every token that starts in one of
/// `excluded`, the half-open spans of its test modules. `None` when it does
/// not lex.
pub(super) fn scan(text: &str, excluded: &[(Position, Position)]) -> Option<FileTokens> {
    let stream: TokenStream = text.parse().ok()?;
    let mut found = FileTokens::default();
    walk(stream, excluded, false, &mut found);
    Some(found)
}

/// The macro tree that starts at `trees[index]`, as its kind, its name, its
/// tokens and how many trees it spans.
fn tree_at(trees: &[TokenTree], index: usize) -> Option<(TreeKind, String, TokenStream, usize)> {
    let group = |offset: usize| match trees.get(index + offset) {
        Some(TokenTree::Group(group)) => Some(group.stream()),
        _ => None,
    };
    match &trees[index] {
        TokenTree::Ident(called) if is_punct(trees.get(index + 1), '!') => {
            if called == "macro_rules" {
                let Some(TokenTree::Ident(defined)) = trees.get(index + 2) else {
                    return None;
                };
                return Some((TreeKind::Definition, defined.to_string(), group(3)?, 4));
            }
            let called = called.to_string();
            if NEGATING_KEYWORDS.contains(&called.as_str()) {
                return None;
            }
            let input = group(2)?;
            let name = if index > 0 && is_punct(trees.get(index - 1), '$') {
                format!("${called}")
            } else {
                called
            };
            Some((TreeKind::Call, name, input, 3))
        }
        TokenTree::Punct(hash) if hash.as_char() == '#' => {
            let inner = usize::from(is_punct(trees.get(index + 1), '!'));
            match trees.get(index + 1 + inner) {
                Some(TokenTree::Group(attribute))
                    if attribute.delimiter() == Delimiter::Bracket =>
                {
                    Some((
                        TreeKind::Attribute,
                        String::new(),
                        attribute.stream(),
                        2 + inner,
                    ))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Appends the name of every macro called in `stream`, at any depth.
fn called_macros(stream: TokenStream, calls: &mut Vec<String>) {
    let trees: Vec<TokenTree> = stream.into_iter().collect();
    for (index, tree) in trees.iter().enumerate() {
        if let Some((TreeKind::Call, name, _, _)) = tree_at(&trees, index) {
            calls.push(name);
        }
        if let TokenTree::Group(group) = tree {
            called_macros(group.stream(), calls);
        }
    }
}

fn walk(
    stream: TokenStream,
    excluded: &[(Position, Position)],
    in_function: bool,
    found: &mut FileTokens,
) {
    let trees: Vec<TokenTree> = stream.into_iter().collect();
    let mut index = 0;
    // Whether a `fn` keyword opened a signature whose body is the next brace.
    let mut signature = false;
    while index < trees.len() {
        let at = position(trees[index].span());
        let line = at.0;
        if excluded
            .iter()
            .any(|(start, end)| (start..end).contains(&&at))
        {
            index += 1;
            continue;
        }
        if let Some((kind, name, stream, width)) = tree_at(&trees, index) {
            let mut tokens = Vec::new();
            flatten(stream.clone(), &mut tokens);
            let mut calls = Vec::new();
            called_macros(stream.clone(), &mut calls);
            found.trees.push(MacroTree {
                kind,
                name,
                line,
                tokens,
                stream,
                in_function,
                calls,
            });
            index += width;
            continue;
        }
        match &trees[index] {
            TokenTree::Ident(keyword) if keyword == "use" => {
                renames(&trees[index + 1..], &mut found.renames);
            }
            TokenTree::Ident(keyword) if keyword == "impl" => {
                if let Some(names) = impl_header_names(&trees[index + 1..]) {
                    found.impl_headers.push(ImplHeader { names, at });
                }
            }
            TokenTree::Ident(keyword) if keyword == "fn" => signature = true,
            TokenTree::Punct(punct) if punct.as_char() == ';' => signature = false,
            TokenTree::Group(nested) => {
                let body = signature && nested.delimiter() == Delimiter::Brace;
                if body {
                    signature = false;
                }
                walk(nested.stream(), excluded, in_function || body, found);
            }
            _ => {}
        }
        index += 1;
    }
}

/// Appends every token of `stream`, descending into groups.
fn flatten(stream: TokenStream, tokens: &mut Vec<String>) {
    for tree in stream {
        match tree {
            TokenTree::Group(group) => flatten(group.stream(), tokens),
            other => tokens.push(other.to_string()),
        }
    }
}

/// Records each `original as alias` of the `use` tree that `after` starts.
fn renames(after: &[TokenTree], found: &mut Vec<(String, String)>) {
    let mut tokens = Vec::new();
    for tree in after {
        if matches!(tree, TokenTree::Punct(punct) if punct.as_char() == ';') {
            break;
        }
        flatten(tree.clone().into(), &mut tokens);
    }
    for window in tokens.windows(3) {
        if window[1] == "as" && window[2] != "_" {
            found.push((window[0].clone(), window[2].clone()));
        }
    }
}

/// Every identifier the tokens after an `impl` keyword spell up to its body,
/// when they spell `for`, as every trait impl header does; `None` otherwise.
///
/// The body is the first brace group; a `;` or the end of the enclosing
/// group ends a header that has none. No other shape is required, so a
/// header no parser reads is still a header.
fn impl_header_names(after: &[TokenTree]) -> Option<Vec<String>> {
    let mut header = Vec::new();
    let mut spells_for = false;
    for tree in after {
        match tree {
            TokenTree::Group(group) if group.delimiter() == Delimiter::Brace => break,
            TokenTree::Punct(punct) if punct.as_char() == ';' => break,
            TokenTree::Ident(ident) if ident == "for" => spells_for = true,
            _ => {}
        }
        flatten(tree.clone().into(), &mut header);
    }
    spells_for.then(|| {
        header
            .into_iter()
            .filter(|token| token.starts_with(|c: char| c.is_alphabetic() || c == '_'))
            .collect()
    })
}

fn is_punct(tree: Option<&TokenTree>, wanted: char) -> bool {
    matches!(tree, Some(TokenTree::Punct(punct)) if punct.as_char() == wanted)
}

/// Whether a call to the macro `name` can bind a name a type argument can
/// spell.
///
/// `definitions` holds every wire-crate `macro_rules!` tree. A macro with no
/// definition there, other than a standard expression macro, can write
/// anything; one whose body spells a binding keyword, or calls a macro that
/// can bind, binds. A call through a metavariable can reach any macro.
pub(super) fn binding_macro(
    name: &str,
    definitions: &[&MacroTree],
    seen: &mut Vec<String>,
) -> bool {
    if name.starts_with('$') {
        return true;
    }
    let defined: Vec<&&MacroTree> = definitions
        .iter()
        .filter(|tree| tree.name == name)
        .collect();
    if defined.is_empty() {
        return !EXPRESSION_MACROS.contains(&name);
    }
    seen.push(name.to_owned());
    defined.iter().any(|tree| {
        tree.spells_binding()
            || tree
                .calls
                .iter()
                .any(|called| !seen.contains(called) && binding_macro(called, definitions, seen))
    })
}

/// Each `Name<..>` with `Name` one of `names` that `stream` spells, at any
/// depth, as the type the name and its arguments parse as, or `Err` with
/// the name when the arguments do not parse as a type.
pub(super) fn spelled_uses(
    stream: &TokenStream,
    names: &[String],
) -> Vec<Result<syn::Type, String>> {
    let trees: Vec<TokenTree> = stream.clone().into_iter().collect();
    let mut found = Vec::new();
    for (index, tree) in trees.iter().enumerate() {
        if let TokenTree::Group(group) = tree {
            found.extend(spelled_uses(&group.stream(), names));
            continue;
        }
        let TokenTree::Ident(ident) = tree else {
            continue;
        };
        let name = ident.to_string();
        if !names.contains(&name) {
            continue;
        }
        let mut open = index + 1;
        if is_punct(trees.get(open), ':') && is_punct(trees.get(open + 1), ':') {
            open += 2;
        }
        if !is_punct(trees.get(open), '<') {
            continue;
        }
        let mut depth = 0_usize;
        let mut close = None;
        for (at, tree) in trees.iter().enumerate().skip(open) {
            let TokenTree::Punct(punct) = tree else {
                continue;
            };
            match punct.as_char() {
                '<' => depth += 1,
                '>' if !is_punct(trees.get(at - 1), '-') => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(at);
                        break;
                    }
                }
                _ => {}
            }
        }
        let parsed = close.and_then(|close| {
            let spelled: TokenStream = std::iter::once(tree.clone())
                .chain(trees[open..=close].iter().cloned())
                .collect();
            syn::parse2::<syn::Type>(spelled).ok()
        });
        found.push(parsed.ok_or(name));
    }
    found
}
