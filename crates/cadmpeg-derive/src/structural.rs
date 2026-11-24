// SPDX-License-Identifier: Apache-2.0
//! `#[derive(StructuralOrd)]`: the derived structural order of
//! `docs/cadir-v11.md` §2.5, as `cadmpeg_ir::structural_order::StructuralOrd`.
//!
//! A struct compares its fields in declared order and an enum its variants in
//! declared order, then the payload's fields in declared order. A field that
//! serde never writes (`#[serde(skip)]`, `#[serde(skip_serializing)]`) is not
//! part of the value and is not compared. Each type parameter is bounded by
//! the trait.

use proc_macro::{Delimiter, TokenStream, TokenTree};

use super::tagged::{closing_angle, default_at, split, unwrapped};
use super::transparent;

const TRAIT: &str = "::cadmpeg_ir::structural_order::StructuralOrd";

struct Item {
    name: String,
    declared: String,
    arguments: String,
    clause: String,
    body: Body,
}

enum Body {
    Struct(Fields),
    Enum(Vec<(String, Fields)>),
}

enum Fields {
    Unit,
    /// Each field's name, `None` where serde skips it.
    Named(Vec<Option<String>>),
    /// Whether serde writes each position.
    Tuple(Vec<bool>),
}

/// The generated order of the type in `input`, or why the derive refuses it.
pub(super) fn derive(input: TokenStream) -> Result<String, String> {
    parse(input).map(|item| expand(&item))
}

fn parse(input: TokenStream) -> Result<Item, String> {
    let tokens: Vec<TokenTree> = input.into_iter().collect();
    let mut at = 0;
    skip_attributes(&tokens, &mut at)?;
    let keyword = tokens[at..].iter().position(|token| {
        matches!(unwrapped(token), TokenTree::Ident(ident)
            if ["enum", "struct", "union"].contains(&&*ident.to_string()))
    });
    let Some(offset) = keyword else {
        return Err("StructuralOrd derives the order of a struct or an enum".to_owned());
    };
    at += offset;
    let keyword = tokens[at].to_string();
    if keyword == "union" {
        return Err("StructuralOrd derives the order of a struct or an enum".to_owned());
    }
    at += 1;
    let name = match tokens.get(at).map(unwrapped) {
        Some(TokenTree::Ident(ident)) => ident.to_string(),
        _ => return Err("StructuralOrd cannot read the type's name".to_owned()),
    };
    at += 1;
    let (mut declared, mut arguments) = (String::new(), String::new());
    if matches!(tokens.get(at), Some(TokenTree::Punct(punct)) if punct.as_char() == '<') {
        let end = closing_angle(&tokens, at)
            .map_err(|_| format!("StructuralOrd cannot read the generic parameters of {name}"))?;
        (declared, arguments) = generics(&name, &tokens[at + 1..end])?;
        at = end + 1;
    }
    let rest = &tokens[at..];
    let Some((position, group)) =
        rest.iter()
            .enumerate()
            .find_map(|(position, token)| match token {
                TokenTree::Group(group)
                    if matches!(group.delimiter(), Delimiter::Brace | Delimiter::Parenthesis) =>
                {
                    Some((position, group))
                }
                _ => None,
            })
    else {
        let clause = clause(before_semicolon(rest));
        return Ok(Item {
            name,
            declared,
            arguments,
            clause,
            body: Body::Struct(Fields::Unit),
        });
    };
    let clause = if group.delimiter() == Delimiter::Brace {
        clause(&rest[..position])
    } else {
        clause(before_semicolon(&rest[position + 1..]))
    };
    let body = if keyword == "enum" {
        Body::Enum(variants(&name, group.stream())?)
    } else if group.delimiter() == Delimiter::Brace {
        Body::Struct(named(&name, group.stream())?)
    } else {
        Body::Struct(tuple(&name, group.stream())?)
    };
    Ok(Item {
        name,
        declared,
        arguments,
        clause,
        body,
    })
}

/// `tokens` without the `;` that ends a unit or tuple struct.
fn before_semicolon(tokens: &[TokenTree]) -> &[TokenTree] {
    tokens.split_last().map_or(tokens, |(_, head)| head)
}

/// The text of a where clause, without its keyword and with a trailing comma,
/// or the empty string.
fn clause(tokens: &[TokenTree]) -> String {
    match tokens.split_first() {
        Some((TokenTree::Ident(keyword), predicates)) if keyword.to_string() == "where" => {
            let text = predicates
                .iter()
                .cloned()
                .collect::<TokenStream>()
                .to_string();
            let text = text.trim().trim_end_matches(',').to_owned();
            if text.is_empty() {
                text
            } else {
                format!("{text},")
            }
        }
        _ => String::new(),
    }
}

/// The impl's parameter list, each type parameter bounded by the trait, and
/// the type's argument list.
fn generics(owner: &str, tokens: &[TokenTree]) -> Result<(String, String), String> {
    let mut declared = Vec::new();
    let mut arguments = Vec::new();
    for part in split(tokens, true) {
        let end = default_at(part).unwrap_or(part.len());
        let part = &part[..end];
        let text = part.iter().cloned().collect::<TokenStream>().to_string();
        match part.first().map(unwrapped) {
            Some(TokenTree::Punct(punct)) if punct.as_char() == '\'' => {
                let Some(TokenTree::Ident(lifetime)) = part.get(1).map(unwrapped) else {
                    return Err(format!("StructuralOrd cannot read a lifetime of {owner}"));
                };
                arguments.push(format!("'{lifetime}"));
                declared.push(text);
            }
            Some(TokenTree::Ident(keyword)) if keyword.to_string() == "const" => {
                let Some(TokenTree::Ident(constant)) = part.get(1).map(unwrapped) else {
                    return Err(format!(
                        "StructuralOrd cannot read a const parameter of {owner}"
                    ));
                };
                arguments.push(constant.to_string());
                declared.push(text);
            }
            Some(TokenTree::Ident(parameter)) => {
                arguments.push(parameter.to_string());
                if part.len() > 1 {
                    declared.push(format!("{text} + {TRAIT}"));
                } else {
                    declared.push(format!("{parameter}: {TRAIT}"));
                }
            }
            _ => {
                return Err(format!(
                    "StructuralOrd cannot read a generic parameter of {owner}"
                ))
            }
        }
    }
    Ok((
        format!("<{}>", declared.join(", ")),
        format!("<{}>", arguments.join(", ")),
    ))
}

/// Advances `at` past the outer attributes at `tokens[*at..]` and returns
/// whether one of them is a serde argument that leaves the item unwritten.
fn skip_attributes(tokens: &[TokenTree], at: &mut usize) -> Result<bool, String> {
    let mut skipped = false;
    while matches!(tokens.get(*at), Some(TokenTree::Punct(punct)) if punct.as_char() == '#') {
        let Some(TokenTree::Group(group)) = tokens.get(*at + 1) else {
            return Err("StructuralOrd cannot read an attribute".to_owned());
        };
        *at += 2;
        let meta: Vec<TokenTree> = transparent(group.stream()).into_iter().collect();
        match meta.first().map(ToString::to_string).as_deref() {
            Some("serde") => {
                if let Some(TokenTree::Group(arguments)) = meta.get(1) {
                    let arguments: Vec<TokenTree> = arguments.stream().into_iter().collect();
                    skipped |= split(&arguments, false).iter().any(|argument| {
                        matches!(argument, [TokenTree::Ident(key)]
                            if ["skip", "skip_serializing"].contains(&&*key.to_string()))
                    });
                }
            }
            Some("cfg") => {
                return Err("StructuralOrd reads items stated without a condition".to_owned())
            }
            _ => {}
        }
    }
    Ok(skipped)
}

/// Advances `at` past a visibility.
fn skip_visibility(tokens: &[TokenTree], at: &mut usize) {
    if matches!(tokens.get(*at).map(unwrapped), Some(TokenTree::Ident(ident)) if ident.to_string() == "pub")
    {
        *at += 1;
        if matches!(tokens.get(*at), Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Parenthesis)
        {
            *at += 1;
        }
    }
}

fn named(owner: &str, stream: TokenStream) -> Result<Fields, String> {
    let tokens: Vec<TokenTree> = stream.into_iter().collect();
    let mut found = Vec::new();
    for part in split(&tokens, true) {
        let mut at = 0;
        let skipped = skip_attributes(part, &mut at)?;
        skip_visibility(part, &mut at);
        let name = match part.get(at).map(unwrapped) {
            Some(TokenTree::Ident(ident)) => ident.to_string(),
            _ => return Err(format!("StructuralOrd cannot read a field of {owner}")),
        };
        if !matches!(part.get(at + 1), Some(TokenTree::Punct(punct)) if punct.as_char() == ':') {
            return Err(format!("StructuralOrd cannot read {owner}::{name}"));
        }
        found.push((!skipped).then_some(name));
    }
    Ok(Fields::Named(found))
}

fn tuple(owner: &str, stream: TokenStream) -> Result<Fields, String> {
    let tokens: Vec<TokenTree> = stream.into_iter().collect();
    let mut found = Vec::new();
    for part in split(&tokens, true) {
        let mut at = 0;
        let skipped = skip_attributes(part, &mut at)?;
        skip_visibility(part, &mut at);
        if at >= part.len() {
            return Err(format!("StructuralOrd cannot read a field of {owner}"));
        }
        found.push(!skipped);
    }
    Ok(Fields::Tuple(found))
}

fn variants(owner: &str, body: TokenStream) -> Result<Vec<(String, Fields)>, String> {
    let tokens: Vec<TokenTree> = body.into_iter().collect();
    let mut found = Vec::new();
    for part in split(&tokens, false) {
        let mut at = 0;
        skip_attributes(part, &mut at)?;
        let name = match part.get(at).map(unwrapped) {
            Some(TokenTree::Ident(ident)) => ident.to_string(),
            _ => return Err(format!("StructuralOrd cannot read a variant of {owner}")),
        };
        let fields = match part.get(at + 1) {
            Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Brace => {
                named(owner, group.stream())?
            }
            Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Parenthesis => {
                tuple(owner, group.stream())?
            }
            _ => Fields::Unit,
        };
        found.push((name, fields));
    }
    Ok(found)
}

/// The comparison of the bindings `left` and `right` of each compared field,
/// in order.
fn chain(pairs: &[(String, String)]) -> String {
    let links: Vec<String> = pairs
        .iter()
        .map(|(left, right)| format!(".then_with(|| {TRAIT}::structural_cmp({left}, {right}))"))
        .collect();
    format!("::core::cmp::Ordering::Equal{}", links.concat())
}

/// A pattern for `path` that binds each compared field with `prefix`, and
/// the binding names.
fn pattern(path: &str, fields: &Fields, prefix: &str) -> (String, Vec<String>) {
    match fields {
        Fields::Unit => (path.to_owned(), Vec::new()),
        Fields::Named(names) => {
            let mut bound = Vec::new();
            let mut parts = Vec::new();
            for (index, name) in names.iter().enumerate() {
                if let Some(name) = name {
                    let binding = format!("{prefix}{index}");
                    parts.push(format!("{name}: {binding}"));
                    bound.push(binding);
                }
            }
            parts.push("..".to_owned());
            (format!("{path} {{ {} }}", parts.join(", ")), bound)
        }
        Fields::Tuple(written) => {
            let mut bound = Vec::new();
            let mut parts = Vec::new();
            for (index, written) in written.iter().enumerate() {
                if *written {
                    let binding = format!("{prefix}{index}");
                    parts.push(binding.clone());
                    bound.push(binding);
                } else {
                    parts.push("_".to_owned());
                }
            }
            (format!("{path}({})", parts.join(", ")), bound)
        }
    }
}

fn wildcard(path: &str, fields: &Fields) -> String {
    match fields {
        Fields::Unit => path.to_owned(),
        Fields::Named(_) => format!("{path} {{ .. }}"),
        Fields::Tuple(_) => format!("{path}(..)"),
    }
}

fn expand(item: &Item) -> String {
    let Item {
        name,
        declared,
        arguments,
        clause,
        body,
    } = item;
    let (other, comparison) = match body {
        Body::Struct(fields) => {
            let pairs: Vec<(String, String)> = match fields {
                Fields::Unit => Vec::new(),
                Fields::Named(names) => names
                    .iter()
                    .flatten()
                    .map(|name| (format!("&self.{name}"), format!("&__other.{name}")))
                    .collect(),
                Fields::Tuple(written) => written
                    .iter()
                    .enumerate()
                    .filter(|(_, written)| **written)
                    .map(|(index, _)| (format!("&self.{index}"), format!("&__other.{index}")))
                    .collect(),
            };
            let other = if pairs.is_empty() { "_" } else { "__other" };
            (other, chain(&pairs))
        }
        Body::Enum(variants) => {
            let mut arms: Vec<String> = variants
                .iter()
                .map(|(variant, fields)| {
                    let path = format!("Self::{variant}");
                    let (left, bound) = pattern(&path, fields, "__left_");
                    let (right, _) = pattern(&path, fields, "__right_");
                    let pairs: Vec<(String, String)> = bound
                        .iter()
                        .map(|left| (left.clone(), left.replacen("__left_", "__right_", 1)))
                        .collect();
                    format!("({left}, {right}) => {},", chain(&pairs))
                })
                .collect();
            if variants.len() > 1 {
                let ranks: Vec<String> = variants
                    .iter()
                    .enumerate()
                    .map(|(rank, (variant, fields))| {
                        format!(
                            "{} => {rank}_usize,",
                            wildcard(&format!("Self::{variant}"), fields)
                        )
                    })
                    .collect();
                arms.push(format!(
                    "_ => {{ let __rank = |__value: &Self| -> usize {{ match __value {{ {} }} }}; \
                     __rank(self).cmp(&__rank(__other)) }}",
                    ranks.concat()
                ));
            }
            (
                "__other",
                format!("match (self, __other) {{ {} }}", arms.concat()),
            )
        }
    };
    let clause = if clause.is_empty() {
        String::new()
    } else {
        format!("where {clause}")
    };
    format!(
        r"
#[automatically_derived]
impl{declared} {TRAIT} for {name}{arguments} {clause} {{
    fn structural_cmp(&self, {other}: &Self) -> ::core::cmp::Ordering {{
        {comparison}
    }}
}}
"
    )
}
