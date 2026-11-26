// SPDX-License-Identifier: Apache-2.0
//! `#[derive(DeserializeTagged)]`: the reader of an enum tagged by serde's
//! `tag`, or by `tag` and `content`, that reads each payload from JSON text
//! through `cadmpeg_core::tagged`.
//!
//! The derive reads the enum's `#[serde(...)]` attributes. It names each
//! variant through a private enum that serde derives with
//! `#[serde(variant_identifier)]`, so the variant names follow serde's rules.
//! It reads a struct variant through a private struct that serde derives
//! with the variant's fields and their attributes. A newtype variant reads
//! its type, or its field's `deserialize_with` function. A unit variant reads
//! its `deserialize_with` function, where it states one.

use proc_macro::{Delimiter, Spacing, TokenStream, TokenTree};

use super::{names_serde, transparent};

struct Enumeration {
    name: String,
    params: Vec<Param>,
    tag: String,
    content: Option<String>,
    rename_all: Option<String>,
    deny_unknown_fields: bool,
    expecting: Option<String>,
    deserialize_name: Option<String>,
    variants: Vec<Variant>,
}

/// The bound each type parameter of an enum without `#[serde(bound = "")]`
/// takes.
const OWNED: &str = "::serde::de::DeserializeOwned";

struct Param {
    name: String,
    bounds: String,
}

struct Variant {
    name: String,
    names: Vec<String>,
    deserialize_with: Option<String>,
    shape: Shape,
}

enum Shape {
    Unit,
    Newtype { reader: Option<String> },
    Struct(Vec<Field>),
}

struct Field {
    name: String,
    serde: Vec<String>,
    ty: Vec<TokenTree>,
}

/// One argument of a `#[serde(...)]` attribute.
struct Item {
    key: String,
    text: String,
    value: Option<TokenTree>,
}

/// The generated reader of the enum in `input`, or why the derive refuses it.
pub(super) fn derive(input: TokenStream) -> Result<String, String> {
    parse(input).map(|enumeration| expand(&enumeration))
}

fn parse(input: TokenStream) -> Result<Enumeration, String> {
    let tokens: Vec<TokenTree> = input.into_iter().collect();
    let mut at = 0;
    let container = attributes(&tokens, &mut at)?;
    let keyword = tokens[at..].iter().position(|token| {
        matches!(token, TokenTree::Ident(ident)
            if ["enum", "struct", "union"].contains(&&*ident.to_string()))
    });
    let Some(offset) = keyword else {
        return Err("DeserializeTagged derives the reader of an enum".to_owned());
    };
    at += offset;
    if tokens[at].to_string() != "enum" {
        return Err("DeserializeTagged derives the reader of an enum".to_owned());
    }
    at += 1;
    let name = match tokens.get(at).map(unwrapped) {
        Some(TokenTree::Ident(ident)) => ident.to_string(),
        _ => return Err("DeserializeTagged cannot read the enum's name".to_owned()),
    };
    at += 1;
    let mut params = Vec::new();
    if matches!(tokens.get(at), Some(TokenTree::Punct(punct)) if punct.as_char() == '<') {
        let end = closing_angle(&tokens, at)?;
        params = generics(&name, &tokens[at + 1..end])?;
        at = end + 1;
    }
    let body = match tokens.get(at) {
        Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Brace => group.stream(),
        _ => {
            return Err(format!(
                "DeserializeTagged reads an enum with no where clause; {name} states one"
            ))
        }
    };
    let mut enumeration = Enumeration {
        name,
        params,
        tag: String::new(),
        content: None,
        rename_all: None,
        deny_unknown_fields: false,
        expecting: None,
        deserialize_name: None,
        variants: Vec::new(),
    };
    let mut stated_bound = None;
    for item in container.into_iter().flatten() {
        match item.key.as_str() {
            "tag" => enumeration.tag = literal(&item)?,
            "content" => enumeration.content = Some(literal(&item)?),
            "rename_all" => enumeration.rename_all = Some(item.text),
            "deny_unknown_fields" if item.value.is_none() => enumeration.deny_unknown_fields = true,
            "expecting" => enumeration.expecting = Some(literal(&item)?),
            "rename" => enumeration.deserialize_name = Some(literal(&item)?),
            "bound" => stated_bound = deserialize_bound(&item)?,
            key => {
                return Err(format!(
                    "DeserializeTagged reads {}; it does not read serde `{key}` on the enum",
                    enumeration.name
                ))
            }
        }
    }
    if enumeration.tag.is_empty() {
        return Err("DeserializeTagged reads an enum that states serde `tag`".to_owned());
    }
    bound_params(&mut enumeration, stated_bound.as_deref())?;
    enumeration.variants = variants(&enumeration.name, body)?;
    Ok(enumeration)
}

/// The deserialize bound that a serde `bound` argument states: the string
/// of `bound = "..."`, or of `deserialize = "..."` in `bound(...)`.
fn deserialize_bound(item: &Item) -> Result<Option<String>, String> {
    if let Ok(text) = literal(item) {
        return Ok(Some(text));
    }
    let Some(TokenTree::Group(group)) = item.value.as_ref().map(unwrapped) else {
        return Err("DeserializeTagged cannot read serde `bound`".to_owned());
    };
    for inner in items(group.stream())? {
        if inner.key == "deserialize" {
            return literal(&inner).map(Some);
        }
    }
    Ok(None)
}

/// Bounds each type parameter of `enumeration` so that every payload reads
/// from owned text. With no deserialize bound stated, each parameter takes
/// `DeserializeOwned`, where serde infers `Deserialize<'de>`. A stated bound
/// takes the place of serde's, each `Deserialize<'de>` in it read as
/// `DeserializeOwned`; `#[serde(bound = "")]` leaves the parameters' own
/// bounds alone.
fn bound_params(enumeration: &mut Enumeration, stated: Option<&str>) -> Result<(), String> {
    let Some(stated) = stated else {
        for param in &mut enumeration.params {
            add_bound(param, OWNED);
        }
        return Ok(());
    };
    let owned = stated
        .replace("::serde::Deserialize<'de>", OWNED)
        .replace("serde::Deserialize<'de>", OWNED)
        .replace("Deserialize<'de>", OWNED);
    for predicate in split_predicates(&owned) {
        let Some((name, bounds)) = predicate.split_once(':') else {
            return Err(format!(
                "DeserializeTagged cannot read the bound `{predicate}` of {}",
                enumeration.name
            ));
        };
        let name = name.trim();
        let Some(param) = enumeration
            .params
            .iter_mut()
            .find(|param| param.name == name)
        else {
            return Err(format!(
                "DeserializeTagged reads a bound on a type parameter of {}; `{name}` is none",
                enumeration.name
            ));
        };
        add_bound(param, bounds.trim());
    }
    Ok(())
}

fn add_bound(param: &mut Param, bound: &str) {
    param.bounds = if param.bounds.is_empty() {
        bound.to_owned()
    } else {
        format!("{} + {bound}", param.bounds)
    };
}

/// `text` split at each comma outside angle brackets, without empty parts.
fn split_predicates(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0_usize;
    let mut start = 0;
    for (index, character) in text.char_indices() {
        match character {
            '<' => depth += 1,
            '>' if depth > 0 => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&text[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    parts.retain(|part| !part.trim().is_empty());
    parts
}

/// `token` without the invisible group a `macro_rules!` fragment arrives in.
pub(super) fn unwrapped(token: &TokenTree) -> TokenTree {
    let mut tokens = transparent(token.clone().into()).into_iter();
    match (tokens.next(), tokens.next()) {
        (Some(only), None) => only,
        _ => token.clone(),
    }
}

/// Reads the outer attributes at `tokens[*at..]`, advancing `at` past them,
/// and returns the arguments of each `#[serde(...)]` among them.
fn attributes(tokens: &[TokenTree], at: &mut usize) -> Result<Vec<Vec<Item>>, String> {
    let mut serde = Vec::new();
    while matches!(tokens.get(*at), Some(TokenTree::Punct(punct)) if punct.as_char() == '#') {
        let Some(TokenTree::Group(group)) = tokens.get(*at + 1) else {
            return Err("DeserializeTagged cannot read an attribute".to_owned());
        };
        *at += 2;
        let meta = transparent(group.stream());
        let mut parts = meta.clone().into_iter();
        let head = parts.next().as_ref().map(ToString::to_string);
        match head.as_deref() {
            Some("serde") => match parts.next() {
                Some(TokenTree::Group(arguments))
                    if arguments.delimiter() == Delimiter::Parenthesis =>
                {
                    serde.push(items(arguments.stream())?);
                }
                _ => return Err("DeserializeTagged cannot read a serde attribute".to_owned()),
            },
            Some("cfg_attr") if names_serde(&meta) => {
                return Err(
                    "DeserializeTagged reads serde attributes stated without a condition"
                        .to_owned(),
                )
            }
            Some("cfg") => {
                return Err("DeserializeTagged reads items stated without a condition".to_owned())
            }
            _ => {}
        }
    }
    Ok(serde)
}

/// The arguments of one `serde(...)` attribute.
fn items(stream: TokenStream) -> Result<Vec<Item>, String> {
    let tokens: Vec<TokenTree> = stream.into_iter().collect();
    let mut found = Vec::new();
    for part in split(&tokens, false) {
        let Some(TokenTree::Ident(key)) = part.first() else {
            return Err("DeserializeTagged cannot read a serde argument".to_owned());
        };
        let value = match part {
            [_] => None,
            [_, TokenTree::Punct(punct), value] if punct.as_char() == '=' => Some(value.clone()),
            [_, group @ TokenTree::Group(_)] => Some(group.clone()),
            _ => return Err(format!("DeserializeTagged cannot read serde `{key}`")),
        };
        found.push(Item {
            key: key.to_string(),
            text: part.iter().cloned().collect::<TokenStream>().to_string(),
            value,
        });
    }
    Ok(found)
}

/// The text of a string literal argument.
fn literal(item: &Item) -> Result<String, String> {
    let refused = || {
        format!(
            "DeserializeTagged reads serde `{}` as a plain string",
            item.key
        )
    };
    let Some(TokenTree::Literal(token)) = item.value.as_ref().map(unwrapped) else {
        return Err(refused());
    };
    let text = token.to_string();
    match text
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    {
        Some(inner) if !inner.contains('\\') => Ok(inner.to_owned()),
        _ => Err(refused()),
    }
}

/// Splits `tokens` at each comma outside angle brackets. With `angles`
/// false, angle brackets are not tracked.
pub(super) fn split(tokens: &[TokenTree], angles: bool) -> Vec<&[TokenTree]> {
    let mut parts = Vec::new();
    let mut depth = 0_usize;
    let mut start = 0;
    let mut arrow = false;
    for (index, token) in tokens.iter().enumerate() {
        if let TokenTree::Punct(punct) = token {
            match punct.as_char() {
                '<' if angles => depth += 1,
                '>' if angles && !arrow && depth > 0 => depth -= 1,
                ',' if depth == 0 => {
                    parts.push(&tokens[start..index]);
                    start = index + 1;
                }
                _ => {}
            }
            arrow = punct.as_char() == '-' && punct.spacing() == Spacing::Joint;
        } else {
            arrow = false;
        }
    }
    parts.push(&tokens[start..]);
    parts.retain(|part| !part.is_empty());
    parts
}

/// The index of the `>` that closes the `<` at `tokens[open]`.
pub(super) fn closing_angle(tokens: &[TokenTree], open: usize) -> Result<usize, String> {
    let mut depth = 0_usize;
    let mut arrow = false;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        if let TokenTree::Punct(punct) = token {
            match punct.as_char() {
                '<' => depth += 1,
                '>' if !arrow && depth > 0 => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(index);
                    }
                }
                _ => {}
            }
            arrow = punct.as_char() == '-' && punct.spacing() == Spacing::Joint;
        } else {
            arrow = false;
        }
    }
    Err("DeserializeTagged cannot read the enum's generic parameters".to_owned())
}

fn generics(owner: &str, tokens: &[TokenTree]) -> Result<Vec<Param>, String> {
    let mut params = Vec::new();
    for part in split(tokens, true) {
        let name = match part.first().map(unwrapped) {
            Some(TokenTree::Ident(ident)) if ident.to_string() != "const" => ident.to_string(),
            _ => {
                return Err(format!(
                    "DeserializeTagged reads an enum whose generic parameters are types; {owner} states another"
                ))
            }
        };
        let bounds = match part.get(1) {
            Some(TokenTree::Punct(punct)) if punct.as_char() == ':' => {
                let rest = &part[2..];
                let end = default_at(rest).unwrap_or(rest.len());
                rest[..end]
                    .iter()
                    .cloned()
                    .collect::<TokenStream>()
                    .to_string()
            }
            _ => String::new(),
        };
        params.push(Param { name, bounds });
    }
    Ok(params)
}

/// The index of the `=` that starts a parameter's default, outside angle
/// brackets.
pub(super) fn default_at(tokens: &[TokenTree]) -> Option<usize> {
    let mut depth = 0_usize;
    for (index, token) in tokens.iter().enumerate() {
        if let TokenTree::Punct(punct) = token {
            match punct.as_char() {
                '<' => depth += 1,
                '>' if depth > 0 => depth -= 1,
                '=' if depth == 0 => return Some(index),
                _ => {}
            }
        }
    }
    None
}

fn variants(owner: &str, body: TokenStream) -> Result<Vec<Variant>, String> {
    let tokens: Vec<TokenTree> = body.into_iter().collect();
    let mut found = Vec::new();
    for part in split(&tokens, false) {
        let mut at = 0;
        let serde = attributes(part, &mut at)?;
        let name = match part.get(at).map(unwrapped) {
            Some(TokenTree::Ident(ident)) => ident.to_string(),
            _ => {
                return Err(format!(
                    "DeserializeTagged cannot read a variant of {owner}"
                ))
            }
        };
        let shape = match &part[at + 1..] {
            [] => Shape::Unit,
            [TokenTree::Group(group)] if group.delimiter() == Delimiter::Parenthesis => {
                newtype(owner, &name, group.stream())?
            }
            [TokenTree::Group(group)] if group.delimiter() == Delimiter::Brace => {
                Shape::Struct(fields(owner, &name, group.stream())?)
            }
            _ => {
                return Err(format!(
                    "DeserializeTagged reads a unit, newtype, or struct variant; {owner}::{name} is another"
                ))
            }
        };
        let mut variant = Variant {
            name,
            names: Vec::new(),
            deserialize_with: None,
            shape,
        };
        for item in serde.into_iter().flatten() {
            match item.key.as_str() {
                "rename" | "alias" => variant.names.push(item.text),
                "deserialize_with" if matches!(variant.shape, Shape::Unit) => {
                    variant.deserialize_with = Some(literal(&item)?);
                }
                key => {
                    return Err(format!(
                        "DeserializeTagged does not read serde `{key}` on {owner}::{}",
                        variant.name
                    ))
                }
            }
        }
        found.push(variant);
    }
    if found.is_empty() {
        return Err(format!(
            "DeserializeTagged reads an enum with a variant; {owner} has none"
        ));
    }
    Ok(found)
}

fn newtype(owner: &str, variant: &str, stream: TokenStream) -> Result<Shape, String> {
    let tokens: Vec<TokenTree> = stream.into_iter().collect();
    let parts = split(&tokens, true);
    let [part] = parts.as_slice() else {
        return Err(format!(
            "DeserializeTagged reads a unit, newtype, or struct variant; {owner}::{variant} is a tuple"
        ));
    };
    let mut at = 0;
    let mut reader = None;
    for item in attributes(part, &mut at)?.into_iter().flatten() {
        match item.key.as_str() {
            "deserialize_with" => reader = Some(literal(&item)?),
            "with" => reader = Some(format!("{}::deserialize", literal(&item)?)),
            "serialize_with" | "skip_serializing_if" => {}
            key => {
                return Err(format!(
                "DeserializeTagged does not read serde `{key}` on the field of {owner}::{variant}"
            ))
            }
        }
    }
    Ok(Shape::Newtype { reader })
}

fn fields(owner: &str, variant: &str, stream: TokenStream) -> Result<Vec<Field>, String> {
    let tokens: Vec<TokenTree> = stream.into_iter().collect();
    let mut found = Vec::new();
    for part in split(&tokens, true) {
        let mut at = 0;
        let mut serde = Vec::new();
        while matches!(part.get(at), Some(TokenTree::Punct(punct)) if punct.as_char() == '#') {
            let Some(TokenTree::Group(group)) = part.get(at + 1) else {
                return Err(format!(
                    "DeserializeTagged cannot read a field of {owner}::{variant}"
                ));
            };
            let meta = transparent(group.stream());
            let head = meta
                .clone()
                .into_iter()
                .next()
                .as_ref()
                .map(ToString::to_string);
            match head.as_deref() {
                Some("serde") => serde.push(format!("#[{meta}]")),
                Some("cfg_attr") if names_serde(&meta) => {
                    return Err(
                        "DeserializeTagged reads serde attributes stated without a condition"
                            .to_owned(),
                    )
                }
                Some("cfg") => {
                    return Err(
                        "DeserializeTagged reads items stated without a condition".to_owned()
                    )
                }
                _ => {}
            }
            at += 2;
        }
        let name = match part.get(at).map(unwrapped) {
            Some(TokenTree::Ident(ident)) => ident.to_string(),
            _ => {
                return Err(format!(
                    "DeserializeTagged cannot read a field of {owner}::{variant}"
                ))
            }
        };
        match part.get(at + 1) {
            Some(TokenTree::Punct(punct)) if punct.as_char() == ':' => {}
            _ => {
                return Err(format!(
                    "DeserializeTagged cannot read {owner}::{variant}::{name}"
                ))
            }
        }
        found.push(Field {
            name,
            serde,
            ty: part[at + 2..].to_vec(),
        });
    }
    Ok(found)
}

/// Whether `tokens` name the identifier `name`.
fn mentions(tokens: &[TokenTree], name: &str) -> bool {
    tokens.iter().any(|token| match token {
        TokenTree::Ident(ident) => ident.to_string() == name,
        TokenTree::Group(group) => mentions(&group.stream().into_iter().collect::<Vec<_>>(), name),
        _ => false,
    })
}

fn declared(params: &[&Param]) -> String {
    if params.is_empty() {
        return String::new();
    }
    let list: Vec<String> = params
        .iter()
        .map(|param| {
            if param.bounds.is_empty() {
                param.name.clone()
            } else {
                format!("{}: {}", param.name, param.bounds)
            }
        })
        .collect();
    format!("<{}>", list.join(", "))
}

fn arguments(params: &[&Param]) -> String {
    if params.is_empty() {
        return String::new();
    }
    let names: Vec<&str> = params.iter().map(|param| param.name.as_str()).collect();
    format!("<{}>", names.join(", "))
}

/// The expression that reads a payload through the function at `path`.
fn reader(path: &str) -> String {
    format!(
        "({{ let mut __d = ::cadmpeg_core::tagged::payload(__text); \
         ::cadmpeg_core::tagged::end({path}(&mut __d), __d) }})"
    )
}

fn expand(enumeration: &Enumeration) -> String {
    let Enumeration {
        name,
        params,
        tag,
        content,
        rename_all,
        deny_unknown_fields,
        expecting,
        deserialize_name,
        variants,
    } = enumeration;
    let all: Vec<&Param> = params.iter().collect();
    let declared_all = declared(&all);
    let arguments_all = arguments(&all);
    let impl_params = if params.is_empty() {
        "<'de>".to_owned()
    } else {
        format!("<'de, {}", &declared_all[1..])
    };
    let turbofish = if params.is_empty() {
        String::new()
    } else {
        format!("::{arguments_all}")
    };
    let this = format!("{name}{arguments_all}");

    let mut items =
        vec!["#[derive(::serde::Deserialize)]\n#[serde(variant_identifier)]\n".to_owned()];
    if let Some(rename_all) = rename_all {
        items.push(format!("#[serde({rename_all})]\n"));
    }
    items.push("enum __Tag {\n".to_owned());
    for variant in variants {
        if !variant.names.is_empty() {
            items.push(format!("#[serde({})]\n", variant.names.join(", ")));
        }
        items.push(format!("{},\n", variant.name));
    }
    items.push("}\n".to_owned());

    let mut arms: Vec<String> = Vec::new();
    for (index, variant) in variants.iter().enumerate() {
        let variant_name = &variant.name;
        let path = format!("{name}::{variant_name}");
        let shadow = format!("__Payload{index}");
        let mut shadow_arguments = String::new();
        let mut destructure = String::new();
        if let Shape::Struct(fields) = &variant.shape {
            let used: Vec<&Param> = params
                .iter()
                .filter(|param| fields.iter().any(|field| mentions(&field.ty, &param.name)))
                .collect();
            let struct_expecting = expecting
                .clone()
                .unwrap_or_else(|| format!("struct variant {name}::{variant_name}"));
            items.push("#[derive(::serde::Deserialize)]\n".to_owned());
            items.push(format!("#[serde(expecting = {struct_expecting:?})]\n"));
            if *deny_unknown_fields {
                items.push("#[serde(deny_unknown_fields)]\n".to_owned());
            }
            if !used.is_empty() {
                items.push("#[serde(bound = \"\")]\n".to_owned());
            }
            items.push(format!("struct {shadow}{} {{\n", declared(&used)));
            for field in fields {
                for attribute in &field.serde {
                    items.push(attribute.clone());
                    items.push("\n".to_owned());
                }
                let ty: TokenStream = field.ty.iter().cloned().collect();
                items.push(format!("{}: {ty},\n", field.name));
            }
            items.push("}\n".to_owned());
            shadow_arguments = arguments(&used);
            let names: Vec<&str> = fields.iter().map(|field| field.name.as_str()).collect();
            destructure = format!(
                "|{shadow} {{ {list} }}| {path} {{ {list} }}",
                list = names.join(", ")
            );
        }
        let typed_shadow = if shadow_arguments.is_empty() {
            shadow.clone()
        } else {
            format!("{shadow}::{shadow_arguments}")
        };
        let unit_reader = variant
            .deserialize_with
            .as_deref()
            .map(|with| format!("{}.map(|()| {path})", reader(with)));
        let newtype_reader = |field_reader: Option<&str>| match field_reader {
            Some(with) => format!("{}.map({path})", reader(with)),
            None => format!("::cadmpeg_core::tagged::read(__text).map({path})"),
        };
        match content {
            None => {
                let arm = match &variant.shape {
                    Shape::Unit => {
                        unit_reader.unwrap_or_else(|| format!("::core::result::Result::Ok({path})"))
                    }
                    Shape::Newtype { reader } => newtype_reader(reader.as_deref()),
                    Shape::Struct(_) => format!(
                        "::cadmpeg_core::tagged::read::<{typed_shadow}>(__text).map({destructure})"
                    ),
                };
                arms.push(format!("__Tag::{variant_name} => {arm},\n"));
            }
            Some(content) => {
                let (missing, present) = match &variant.shape {
                    Shape::Unit => (
                        format!("::core::result::Result::Ok({path})"),
                        unit_reader.unwrap_or_else(|| {
                            let unit_expecting = format!("unit variant {name}::{variant_name}");
                            format!(
                                "::cadmpeg_core::tagged::read_unit(__text, {unit_expecting:?}).map(|()| {path})"
                            )
                        }),
                    ),
                    Shape::Newtype { reader } => (
                        format!("::cadmpeg_core::tagged::missing({content:?}).map({path})"),
                        newtype_reader(reader.as_deref()),
                    ),
                    Shape::Struct(_) => {
                        let struct_expecting = expecting
                            .clone()
                            .unwrap_or_else(|| format!("struct variant {name}::{variant_name}"));
                        (
                            format!(
                                "::core::result::Result::Err(::cadmpeg_core::tagged::missing_field({content:?}))"
                            ),
                            format!(
                                "::cadmpeg_core::tagged::read_object::<{typed_shadow}>(__text, {struct_expecting:?}).map({destructure})"
                            ),
                        )
                    }
                };
                arms.push(format!(
                    "(__Tag::{variant_name}, ::core::option::Option::None) => {missing},\n\
                     (__Tag::{variant_name}, ::core::option::Option::Some(__text)) => {present},\n"
                ));
            }
        }
    }

    let items = items.concat();
    let arms = arms.concat();
    let read_tag = "__name.read::<__Tag>()?";
    if params.is_empty() {
        return match content {
            None => {
                let enum_expecting = expecting
                    .clone()
                    .unwrap_or_else(|| format!("internally tagged enum {name}"));
                format!(
                    r"
const _: () = {{
    {items}

    fn __read_tag(
        __slot: &mut ::core::option::Option<__Tag>,
        __name: ::cadmpeg_core::tagged::Tag<'_>,
    ) -> ::core::result::Result<(), ::std::string::String> {{
        *__slot = ::core::option::Option::Some({read_tag});
        ::core::result::Result::Ok(())
    }}

    fn __payload(__tag: __Tag, __text: &str) -> ::core::result::Result<{this}, ::std::string::String> {{
        match __tag {{
            {arms}
        }}
    }}

    #[automatically_derived]
    impl<'de> ::serde::Deserialize<'de> for {this} {{
        fn deserialize<__D>(__deserializer: __D) -> ::core::result::Result<Self, __D::Error>
        where
            __D: ::serde::Deserializer<'de>,
        {{
            let mut __tag: ::core::option::Option<__Tag> = ::core::option::Option::None;
            let __rest = ::cadmpeg_core::tagged::internally(
                __deserializer,
                {tag:?},
                {enum_expecting:?},
                &mut |__name| __read_tag(&mut __tag, __name),
            )?;
            match __tag {{
                ::core::option::Option::Some(__read) => __payload(__read, &__rest)
                    .map_err(<__D::Error as ::serde::de::Error>::custom),
                ::core::option::Option::None => ::core::result::Result::Err(
                    <__D::Error as ::serde::de::Error>::missing_field({tag:?}),
                ),
            }}
        }}
    }}
}};
"
                )
            }
            Some(content) => {
                let enum_expecting = expecting
                    .clone()
                    .unwrap_or_else(|| format!("adjacently tagged enum {name}"));
                let struct_name = deserialize_name.clone().unwrap_or_else(|| name.clone());
                format!(
                    r"
const _: () = {{
    {items}

    fn __payload(__tag: __Tag, __content: ::core::option::Option<&str>) -> ::core::result::Result<{this}, ::std::string::String> {{
        match (__tag, __content) {{
            {arms}
        }}
    }}

    fn __step(
        __tag: &mut ::core::option::Option<__Tag>,
        __value: &mut ::core::option::Option<{this}>,
        __event: ::cadmpeg_core::tagged::Event<'_>,
    ) -> ::core::result::Result<(), ::std::string::String> {{
        match __event {{
            ::cadmpeg_core::tagged::Event::Tag(__name) => {{
                *__tag = ::core::option::Option::Some({read_tag});
            }}
            ::cadmpeg_core::tagged::Event::Content(__content) => {{
                let ::core::option::Option::Some(__read) = __tag.take() else {{
                    return ::core::result::Result::Err(
                        ::cadmpeg_core::tagged::missing_field({tag:?}),
                    );
                }};
                *__value = ::core::option::Option::Some(__payload(__read, __content)?);
            }}
        }}
        ::core::result::Result::Ok(())
    }}

    #[automatically_derived]
    impl<'de> ::serde::Deserialize<'de> for {this} {{
        fn deserialize<__D>(__deserializer: __D) -> ::core::result::Result<Self, __D::Error>
        where
            __D: ::serde::Deserializer<'de>,
        {{
            let mut __tag: ::core::option::Option<__Tag> = ::core::option::Option::None;
            let mut __value: ::core::option::Option<Self> = ::core::option::Option::None;
            ::cadmpeg_core::tagged::adjacently(
                __deserializer,
                &::cadmpeg_core::tagged::Adjacent {{
                    rust_name: {name:?},
                    name: {struct_name:?},
                    tag: {tag:?},
                    content: {content:?},
                    fields: &[{tag:?}, {content:?}],
                    deny_unknown_fields: {deny_unknown_fields},
                    expecting: {enum_expecting:?},
                }},
                &mut |__event| __step(&mut __tag, &mut __value, __event),
            )?;
            __value.ok_or_else(|| <__D::Error as ::serde::de::Error>::missing_field({tag:?}))
        }}
    }}
}};
"
                )
            }
        };
    }
    let body = match content {
        None => {
            let enum_expecting = expecting
                .clone()
                .unwrap_or_else(|| format!("internally tagged enum {name}"));
            format!(
                r"
fn __payload{declared_all}(__tag: __Tag, __text: &str) -> ::core::result::Result<{this}, ::std::string::String> {{
    match __tag {{
        {arms}
    }}
}}

let mut __tag: ::core::option::Option<__Tag> = ::core::option::Option::None;
let __rest = ::cadmpeg_core::tagged::internally(
    __deserializer,
    {tag:?},
    {enum_expecting:?},
    &mut |__name: ::cadmpeg_core::tagged::Tag<'_>| {{
        __tag = ::core::option::Option::Some({read_tag});
        ::core::result::Result::Ok(())
    }},
)?;
match __tag {{
    ::core::option::Option::Some(__read) => __payload{turbofish}(__read, &__rest)
        .map_err(<__D::Error as ::serde::de::Error>::custom),
    ::core::option::Option::None => ::core::result::Result::Err(
        <__D::Error as ::serde::de::Error>::missing_field({tag:?}),
    ),
}}
"
            )
        }
        Some(content) => {
            let enum_expecting = expecting
                .clone()
                .unwrap_or_else(|| format!("adjacently tagged enum {name}"));
            let struct_name = deserialize_name.clone().unwrap_or_else(|| name.clone());
            format!(
                r"
fn __payload{declared_all}(__tag: __Tag, __content: ::core::option::Option<&str>) -> ::core::result::Result<{this}, ::std::string::String> {{
    match (__tag, __content) {{
        {arms}
    }}
}}

let mut __tag: ::core::option::Option<__Tag> = ::core::option::Option::None;
let mut __value: ::core::option::Option<Self> = ::core::option::Option::None;
::cadmpeg_core::tagged::adjacently(
    __deserializer,
    &::cadmpeg_core::tagged::Adjacent {{
        rust_name: {name:?},
        name: {struct_name:?},
        tag: {tag:?},
        content: {content:?},
        fields: &[{tag:?}, {content:?}],
        deny_unknown_fields: {deny_unknown_fields},
        expecting: {enum_expecting:?},
    }},
    &mut |__event: ::cadmpeg_core::tagged::Event<'_>| {{
        match __event {{
            ::cadmpeg_core::tagged::Event::Tag(__name) => {{
                __tag = ::core::option::Option::Some({read_tag});
            }}
            ::cadmpeg_core::tagged::Event::Content(__content) => {{
                let ::core::option::Option::Some(__read) = __tag.take() else {{
                    return ::core::result::Result::Err(
                        ::cadmpeg_core::tagged::missing_field({tag:?}),
                    );
                }};
                __value = ::core::option::Option::Some(__payload{turbofish}(__read, __content)?);
            }}
        }}
        ::core::result::Result::Ok(())
    }},
)?;
__value.ok_or_else(|| <__D::Error as ::serde::de::Error>::missing_field({tag:?}))
"
            )
        }
    };

    format!(
        r"
#[automatically_derived]
impl{impl_params} ::serde::Deserialize<'de> for {this} {{
    fn deserialize<__D>(__deserializer: __D) -> ::core::result::Result<Self, __D::Error>
    where
        __D: ::serde::Deserializer<'de>,
    {{
        {items}
        {body}
    }}
}}
"
    )
}
