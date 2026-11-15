// SPDX-License-Identifier: Apache-2.0
//! What a source states in tokens the item walk does not read: the token trees
//! macros read, every trait impl header wherever it sits, and every `use`
//! rename.
//!
//! The census expands no macro. A token tree that names a trait or a generic
//! declaration can expand to an impl or an instantiation no parsed item states,
//! so the census reads the trees themselves and fails a proof that one of them
//! could change.

use proc_macro2::{Delimiter, Spacing, TokenStream, TokenTree};

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

    /// The first of `names` the tree spells with generic arguments, as
    /// `Name<..>` or `Name::<..>`.
    pub(super) fn instantiates<'a>(&self, names: &'a [String]) -> Option<&'a String> {
        self.tokens.iter().enumerate().find_map(|(index, token)| {
            let name = names.iter().find(|name| *name == token)?;
            let next = |offset: usize| self.tokens.get(index + offset).map(String::as_str);
            let generic = next(1) == Some("<")
                || (next(1) == Some(":") && next(2) == Some(":") && next(3) == Some("<"));
            generic.then_some(name)
        })
    }

    /// Whether the tree's own tokens spell a type declaration: `struct`,
    /// `enum`, `union` or `type` that is no metavariable.
    pub(super) fn spells_declaration(&self) -> bool {
        self.tokens.iter().enumerate().any(|(index, token)| {
            matches!(token.as_str(), "struct" | "enum" | "union" | "type")
                && (index == 0 || self.tokens[index - 1] != "$")
        })
    }
}

/// What one source states in tokens.
#[derive(Default)]
pub(super) struct FileTokens {
    pub(super) trees: Vec<MacroTree>,
    /// Each trait impl header outside a token tree, as the last segment of its
    /// trait path and the line of its `impl`.
    pub(super) impl_headers: Vec<(String, usize)>,
    /// Each `original as alias` a `use` declaration states.
    pub(super) renames: Vec<(String, String)>,
}

/// Reads `text` as tokens, skipping every token on a line of `excluded`, the
/// inclusive line ranges of its test modules. `None` when it does not lex.
pub(super) fn scan(text: &str, excluded: &[(usize, usize)]) -> Option<FileTokens> {
    let stream: TokenStream = text.parse().ok()?;
    let mut found = FileTokens::default();
    walk(stream, excluded, &mut found);
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

fn walk(stream: TokenStream, excluded: &[(usize, usize)], found: &mut FileTokens) {
    let trees: Vec<TokenTree> = stream.into_iter().collect();
    let mut index = 0;
    while index < trees.len() {
        let line = trees[index].span().start().line;
        if excluded
            .iter()
            .any(|(first, last)| (*first..=*last).contains(&line))
        {
            index += 1;
            continue;
        }
        if let Some((kind, name, stream, width)) = tree_at(&trees, index) {
            let mut tokens = Vec::new();
            flatten(stream.clone(), &mut tokens);
            let mut calls = Vec::new();
            called_macros(stream, &mut calls);
            found.trees.push(MacroTree {
                kind,
                name,
                line,
                tokens,
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
                if let Some(name) = impl_trait(&trees[index + 1..]) {
                    found.impl_headers.push((name, line));
                }
            }
            TokenTree::Group(nested) => walk(nested.stream(), excluded, found),
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

/// The last segment of the trait an impl header names, when the tokens after
/// its `impl` keyword are a trait impl header.
fn impl_trait(after: &[TokenTree]) -> Option<String> {
    let mut index = 0;
    if is_punct(after.first(), '<') {
        index = skip_generics(after, 0)?;
    }
    if is_punct(after.get(index), '!') {
        index += 1;
    }
    let mut last = None;
    while let Some(tree) = after.get(index) {
        match tree {
            TokenTree::Ident(ident) if ident == "for" => return last,
            TokenTree::Ident(ident) => last = Some(ident.to_string()),
            TokenTree::Punct(punct) if punct.as_char() == '<' => {
                index = skip_generics(after, index)?;
                continue;
            }
            // A path separator, or the `-> R` of a `Fn(A) -> R` trait.
            TokenTree::Punct(punct) if matches!(punct.as_char(), ':' | '-' | '>') => {}
            TokenTree::Group(group) if group.delimiter() == Delimiter::Parenthesis => {}
            _ => return None,
        }
        index += 1;
    }
    None
}

/// The index after the `>` that closes the `<` at `start`.
fn skip_generics(trees: &[TokenTree], start: usize) -> Option<usize> {
    let mut depth = 0_usize;
    let mut arrow = false;
    for (index, tree) in trees.iter().enumerate().skip(start) {
        let TokenTree::Punct(punct) = tree else {
            arrow = false;
            continue;
        };
        match punct.as_char() {
            '<' => depth += 1,
            '>' if !arrow => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index + 1);
                }
            }
            _ => {}
        }
        arrow = punct.as_char() == '-' && punct.spacing() == Spacing::Joint;
    }
    None
}

fn is_punct(tree: Option<&TokenTree>, wanted: char) -> bool {
    matches!(tree, Some(TokenTree::Punct(punct)) if punct.as_char() == wanted)
}

/// Whether a call to the macro `name` can write a type declaration.
///
/// `definitions` holds every wire-crate `macro_rules!` tree. A macro with no
/// definition there, other than a standard expression macro, can write
/// anything; one whose body spells a type declaration, or calls a macro that
/// can, writes one. A call through a metavariable can reach any macro.
pub(super) fn declaring_macro(
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
        tree.spells_declaration()
            || tree
                .calls
                .iter()
                .any(|called| !seen.contains(called) && declaring_macro(called, definitions, seen))
    })
}
