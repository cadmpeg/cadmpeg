// SPDX-License-Identifier: Apache-2.0
//! Derive macros for the cadmpeg wire types.
//!
//! `#[derive(DeserializeName)]` takes the place of serde's derived
//! `Deserialize` on an enum whose variants hold no data. serde's derive reads
//! such an enum from the variant's name and also from a one-key object whose
//! value is `null` (`{"parent": null}` reads as `"parent"`), a second spelling
//! that `docs/cadir-v11.md` §2.5 refuses. The derived reader accepts the
//! variant's name as a JSON string and nothing else.
//!
//! The enum's `#[serde(...)]` attributes keep their meaning: the reader
//! resolves the name through a private enum with the same variants and the
//! same serde attributes, derived by serde. An enum with a tag, with
//! `untagged`, with a conversion, with a generic parameter, or with a variant
//! that holds data fails to compile.
//!
//! `#[derive(DeserializeTagged)]` takes the place of serde's derived
//! `Deserialize` on an enum tagged by `tag`, or by `tag` and `content`. It
//! accepts and refuses the documents that serde's derive does, and reads each
//! payload from JSON text through `cadmpeg_core::tagged`, so the payload's
//! types are compiled against the one deserializer that reads CADIR text.
//!
//! `#[derive(StructuralOrd)]` implements
//! `cadmpeg_ir::structural_order::StructuralOrd`, the derived structural
//! order of §2.5, from the type's declaration.

use proc_macro::{Delimiter, LexError, TokenStream, TokenTree};

mod structural;
mod tagged;

/// Derives a `Deserialize` that reads a data-free enum from its variant's
/// name alone.
#[proc_macro_derive(DeserializeName, attributes(serde))]
pub fn deserialize_name(input: TokenStream) -> TokenStream {
    let expanded = parse(input).map_or_else(
        |message| compile_error(&message),
        |enumeration| expand(&enumeration),
    );
    expanded
        .parse()
        .or_else(|error: LexError| compile_error(&error.to_string()).parse())
        .unwrap_or_default()
}

/// Derives a `Deserialize` for an enum tagged by serde's `tag`, or by `tag`
/// and `content`, that reads each payload from JSON text.
#[proc_macro_derive(DeserializeTagged, attributes(serde))]
pub fn deserialize_tagged(input: TokenStream) -> TokenStream {
    let expanded = tagged::derive(input).unwrap_or_else(|message| compile_error(&message));
    expanded
        .parse()
        .or_else(|error: LexError| compile_error(&error.to_string()).parse())
        .unwrap_or_default()
}

/// Derives the structural order of §2.5 from a struct's or an enum's
/// declaration.
#[proc_macro_derive(StructuralOrd, attributes(serde))]
pub fn structural_ord(input: TokenStream) -> TokenStream {
    let expanded = structural::derive(input).unwrap_or_else(|message| compile_error(&message));
    expanded
        .parse()
        .or_else(|error: LexError| compile_error(&error.to_string()).parse())
        .unwrap_or_default()
}

fn compile_error(message: &str) -> String {
    format!("::core::compile_error!({message:?});")
}

/// The container keys that give an enum another wire form than its
/// variant's name.
const REFUSED_KEYS: [&str; 9] = [
    "tag",
    "content",
    "untagged",
    "from",
    "try_from",
    "remote",
    "transparent",
    "variant_identifier",
    "field_identifier",
];

struct Enumeration {
    name: String,
    serde: Vec<String>,
    variants: Vec<Variant>,
}

struct Variant {
    name: String,
    serde: Vec<String>,
}

fn parse(input: TokenStream) -> Result<Enumeration, String> {
    let mut tokens = input.into_iter().peekable();
    let serde = attributes(&mut tokens)?;
    for attribute in &serde {
        if let Some(key) = REFUSED_KEYS.iter().find(|key| states_key(attribute, key)) {
            return Err(format!(
                "DeserializeName reads the variant's name; serde `{key}` gives the enum another wire form"
            ));
        }
    }
    let keyword = tokens.by_ref().find_map(|token| match token {
        TokenTree::Ident(ident) if ["enum", "struct", "union"].contains(&&*ident.to_string()) => {
            Some(ident.to_string())
        }
        _ => None,
    });
    if keyword.as_deref() != Some("enum") {
        return Err("DeserializeName derives the reader of an enum".to_owned());
    }
    let name = match tokens.next() {
        Some(TokenTree::Ident(ident)) => ident.to_string(),
        _ => return Err("DeserializeName derives the reader of an enum".to_owned()),
    };
    let body = match tokens.next() {
        Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Brace => group.stream(),
        _ => {
            return Err(format!(
                "DeserializeName reads an enum with no generic parameter or where clause; {name} states one"
            ))
        }
    };
    let variants = variants(&name, body)?;
    Ok(Enumeration {
        name,
        serde,
        variants,
    })
}

/// Reads the outer attributes at the head of `tokens` and returns the text
/// of each `#[serde(...)]` among them.
fn attributes(
    tokens: &mut std::iter::Peekable<proc_macro::token_stream::IntoIter>,
) -> Result<Vec<String>, String> {
    let mut serde = Vec::new();
    while matches!(tokens.peek(), Some(TokenTree::Punct(punct)) if punct.as_char() == '#') {
        tokens.next();
        let Some(TokenTree::Group(group)) = tokens.next() else {
            return Err("DeserializeName cannot read an attribute".to_owned());
        };
        let meta = transparent(group.stream());
        match meta.clone().into_iter().next() {
            Some(TokenTree::Ident(ident)) if ident.to_string() == "serde" => {
                serde.push(format!("[{meta}]"));
            }
            Some(TokenTree::Ident(ident))
                if ident.to_string() == "cfg_attr" && names_serde(&meta) =>
            {
                return Err(
                    "DeserializeName reads serde attributes stated without a condition".to_owned(),
                );
            }
            Some(TokenTree::Ident(ident)) if ident.to_string() == "cfg" => {
                return Err("DeserializeName reads variants stated without a condition".to_owned());
            }
            _ => {}
        }
    }
    Ok(serde)
}

/// `stream` without the invisible group a `macro_rules!` fragment arrives
/// in, where the stream is one such group.
fn transparent(stream: TokenStream) -> TokenStream {
    let mut tokens = stream.clone().into_iter();
    match (tokens.next(), tokens.next()) {
        (Some(TokenTree::Group(group)), None) if group.delimiter() == Delimiter::None => {
            transparent(group.stream())
        }
        _ => stream,
    }
}

fn names_serde(stream: &TokenStream) -> bool {
    stream.clone().into_iter().any(|token| match token {
        TokenTree::Ident(ident) => ident.to_string() == "serde",
        TokenTree::Group(group) => names_serde(&group.stream()),
        _ => false,
    })
}

/// Whether the attribute text `#[serde(...)]` states `key` among its
/// top-level arguments.
fn states_key(attribute: &str, key: &str) -> bool {
    let Ok(stream) = attribute.parse::<TokenStream>() else {
        return false;
    };
    let Some(TokenTree::Group(bracket)) = stream.into_iter().nth(1) else {
        return false;
    };
    let Some(TokenTree::Group(arguments)) = bracket.stream().into_iter().nth(1) else {
        return false;
    };
    let mut at_start = true;
    for token in arguments.stream() {
        match &token {
            TokenTree::Ident(ident) if at_start && ident.to_string() == key => return true,
            TokenTree::Punct(punct) if punct.as_char() == ',' => {
                at_start = true;
                continue;
            }
            _ => {}
        }
        at_start = false;
    }
    false
}

fn variants(owner: &str, body: TokenStream) -> Result<Vec<Variant>, String> {
    let mut found = Vec::new();
    let mut tokens = body.into_iter().peekable();
    while tokens.peek().is_some() {
        let serde = attributes(&mut tokens)?;
        let name = match tokens
            .next()
            .map(|token| transparent(token.into()).into_iter().collect::<Vec<_>>())
            .as_deref()
        {
            Some([TokenTree::Ident(ident)]) => ident.to_string(),
            _ => return Err(format!("DeserializeName cannot read a variant of {owner}")),
        };
        match tokens.peek() {
            Some(TokenTree::Group(_)) => {
                return Err(format!(
                    "DeserializeName reads a data-free enum; {owner}::{name} holds data"
                ));
            }
            Some(TokenTree::Punct(punct)) if punct.as_char() == '=' => {
                for token in tokens.by_ref() {
                    if matches!(&token, TokenTree::Punct(punct) if punct.as_char() == ',') {
                        break;
                    }
                }
            }
            Some(TokenTree::Punct(punct)) if punct.as_char() == ',' => {
                tokens.next();
            }
            None => {}
            Some(_) => return Err(format!("DeserializeName cannot read {owner}::{name}")),
        }
        found.push(Variant { name, serde });
    }
    if found.is_empty() {
        return Err(format!(
            "DeserializeName reads an enum with a variant; {owner} has none"
        ));
    }
    Ok(found)
}

fn expand(enumeration: &Enumeration) -> String {
    let Enumeration {
        name,
        serde,
        variants,
    } = enumeration;
    let mut container = String::new();
    for attribute in serde {
        container.push('#');
        container.push_str(attribute);
    }
    let mut shadow = String::new();
    let mut arms = String::new();
    for variant in variants {
        for attribute in &variant.serde {
            shadow.push('#');
            shadow.push_str(attribute);
        }
        shadow.push_str(&variant.name);
        shadow.push(',');
        for piece in ["__Name::", &variant.name, " => Self::", &variant.name, ","] {
            arms.push_str(piece);
        }
    }
    let expecting = format!("the name of a variant of {name}");
    format!(
        r"
#[automatically_derived]
impl<'de> ::serde::Deserialize<'de> for {name} {{
    fn deserialize<__D>(__deserializer: __D) -> ::core::result::Result<Self, __D::Error>
    where
        __D: ::serde::Deserializer<'de>,
    {{
        #[derive(::serde::Deserialize)]
        {container}
        enum __Name {{ {shadow} }}

        struct __Visitor;

        impl<'de> ::serde::de::Visitor<'de> for __Visitor {{
            type Value = __Name;

            fn expecting(
                &self,
                formatter: &mut ::core::fmt::Formatter<'_>,
            ) -> ::core::fmt::Result {{
                formatter.write_str({expecting:?})
            }}

            fn visit_str<__E>(self, value: &str) -> ::core::result::Result<__Name, __E>
            where
                __E: ::serde::de::Error,
            {{
                <__Name as ::serde::Deserialize>::deserialize(
                    ::serde::de::value::StrDeserializer::<__E>::new(value),
                )
            }}
        }}

        ::core::result::Result::Ok(match __deserializer.deserialize_str(__Visitor)? {{
            {arms}
        }})
    }}
}}
"
    )
}
