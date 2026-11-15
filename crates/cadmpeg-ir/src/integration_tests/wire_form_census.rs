// SPDX-License-Identifier: Apache-2.0
//! Every serialized type of `cadmpeg-ir` and `cadmpeg-core` states the CADIR
//! v11 wire form (`docs/cadir-v11.md` §2.5) in its serde attributes.
//!
//! The census parses the two crates' source at run time. It finds each enum
//! that derives `Serialize` or `Deserialize` and computes the one tag form the
//! spec gives it from its variants. It finds each field and checks that an
//! `Option` is omitted for `None` and refuses `null`, and that no field other
//! than an `Option` is ever left out. Test files and `#[cfg(test)]` items are
//! not wire types.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use proc_macro2::{Delimiter, TokenStream, TokenTree};
use syn::{Attribute, Fields, Item, Variant};

const WIRE_CRATES: [&str; 2] = ["crates/cadmpeg-ir/src", "crates/cadmpeg-core/src"];

const COLLECTIONS: [&str; 8] = [
    "Vec", "VecDeque", "BTreeMap", "HashMap", "BTreeSet", "HashSet", "IndexMap", "Map",
];

const MAPS: [&str; 3] = ["BTreeMap", "HashMap", "IndexMap"];

const OPTION_READER_MACRO: &str = "named_optional_field";

const OPTION_READER: &str = "named_present";

#[derive(Clone)]
struct Decl {
    file: String,
    line: usize,
    name: String,
    attrs: Vec<Attribute>,
    body: Body,
    generics: Vec<String>,
    generic_defaults: BTreeMap<String, syn::Type>,
}

#[derive(Clone)]
enum Body {
    Struct(Fields),
    Enum(Vec<Variant>),
}

#[derive(Default)]
struct Index {
    decls: BTreeMap<String, Vec<Decl>>,
    hand_serialize: BTreeMap<String, Vec<String>>,
    option_readers: BTreeSet<String>,
    macro_serde_attrs: Vec<(String, Vec<TokenTree>)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Form {
    Name,
    Internal,
    Adjacent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Payload {
    Struct { has_kind: bool },
    Value,
    Unresolved(String),
}

type SerdeMetas = Vec<(String, Option<String>)>;

fn is_test_path(relative: &str) -> bool {
    relative.ends_with("/tests.rs")
        || relative.ends_with("_tests.rs")
        || relative.contains("/tests/")
        || relative.contains("/test_support")
        || relative.contains("/integration_tests")
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            out.push(path);
        }
    }
}

fn flatten(tokens: &TokenStream, out: &mut Vec<String>) {
    for token in tokens.clone() {
        match token {
            TokenTree::Group(group) => flatten(&group.stream(), out),
            other => out.push(other.to_string()),
        }
    }
}

fn derives(attrs: &[Attribute], name: &str) -> bool {
    attrs.iter().any(|attr| {
        let is_cfg_attr = attr.path().is_ident("cfg_attr");
        if !attr.path().is_ident("derive") && !is_cfg_attr {
            return false;
        }
        let Ok(list) = attr.meta.require_list() else {
            return false;
        };
        let mut tokens = Vec::new();
        flatten(&list.tokens, &mut tokens);
        if is_cfg_attr && !tokens.iter().any(|token| token == "derive") {
            return false;
        }
        tokens.iter().any(|token| token == name)
    })
}

fn is_test_cfg(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if !attr.path().is_ident("cfg") {
            return false;
        }
        let Ok(list) = attr.meta.require_list() else {
            return false;
        };
        let text = list.tokens.to_string();
        text == "test"
            || (text.starts_with("all") && text.contains("test") && !text.contains("not"))
    })
}

fn impl_target(item: &syn::ItemImpl, trait_name: &str) -> Option<String> {
    let (path, _) = item.trait_.as_ref()?;
    if path.segments.last()?.ident != trait_name {
        return None;
    }
    match item.self_ty.as_ref() {
        syn::Type::Path(ty) => Some(ty.path.segments.last()?.ident.to_string()),
        _ => None,
    }
}

fn item_attrs(item: &Item) -> &[Attribute] {
    match item {
        Item::Struct(item) => &item.attrs,
        Item::Enum(item) => &item.attrs,
        Item::Impl(item) => &item.attrs,
        Item::Fn(item) => &item.attrs,
        Item::Mod(item) => &item.attrs,
        Item::Macro(item) => &item.attrs,
        Item::Const(item) => &item.attrs,
        _ => &[],
    }
}

fn source_tokens(text: &str, first_line: usize, last_line: usize) -> Vec<String> {
    let slice: Vec<&str> = text
        .lines()
        .skip(first_line - 1)
        .take(last_line + 1 - first_line)
        .collect();
    let mut tokens = Vec::new();
    flatten(
        &TokenStream::from_str(&slice.join("\n")).unwrap(),
        &mut tokens,
    );
    tokens
}

fn serde_groups(tokens: &TokenStream, out: &mut Vec<Vec<TokenTree>>) {
    let tokens: Vec<TokenTree> = tokens.clone().into_iter().collect();
    for (at, token) in tokens.iter().enumerate() {
        let TokenTree::Group(group) = token else {
            continue;
        };
        let after_serde =
            at > 0 && matches!(&tokens[at - 1], TokenTree::Ident(ident) if ident == "serde");
        if after_serde && group.delimiter() == Delimiter::Parenthesis {
            out.push(group.stream().into_iter().collect());
        } else {
            serde_groups(&group.stream(), out);
        }
    }
}

fn macro_option_readers(tokens: &TokenStream, out: &mut BTreeSet<String>) {
    let tokens: Vec<TokenTree> = tokens.clone().into_iter().collect();
    for window in tokens.windows(3) {
        if let [TokenTree::Ident(name), TokenTree::Punct(bang), TokenTree::Group(args)] = window {
            if name == OPTION_READER_MACRO && bang.as_char() == '!' {
                if let Some(TokenTree::Ident(reader)) = args.stream().into_iter().next() {
                    out.insert(reader.to_string());
                }
            }
        }
    }
}

struct Walker<'a> {
    file: &'a str,
    text: &'a str,
    test_modules: Vec<String>,
}

impl Walker<'_> {
    fn decl(
        &self,
        name: &syn::Ident,
        attrs: &[Attribute],
        body: Body,
        generics: &syn::Generics,
    ) -> Decl {
        Decl {
            file: self.file.to_owned(),
            line: name.span().start().line,
            name: name.to_string(),
            attrs: attrs.to_vec(),
            body,
            generics: generics
                .type_params()
                .map(|param| param.ident.to_string())
                .collect(),
            generic_defaults: generics
                .type_params()
                .filter_map(|param| {
                    param
                        .default
                        .clone()
                        .map(|(_, ty)| (param.ident.to_string(), ty))
                })
                .collect(),
        }
    }

    fn block(&mut self, block: &syn::Block, index: &mut Index) {
        for statement in &block.stmts {
            match statement {
                syn::Stmt::Item(item) => self.items(std::slice::from_ref(item), index),
                syn::Stmt::Macro(statement) => self.invocation(&statement.mac, "statement", index),
                _ => {}
            }
        }
    }

    fn invocation(&self, mac: &syn::Macro, name: &str, index: &mut Index) {
        if mac
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == OPTION_READER_MACRO)
        {
            if let Some(TokenTree::Ident(reader)) = mac.tokens.clone().into_iter().next() {
                index.option_readers.insert(reader.to_string());
            }
        }
        self.macro_tokens(&mac.tokens, name, index);
    }

    fn macro_tokens(&self, tokens: &TokenStream, name: &str, index: &mut Index) {
        let mut groups = Vec::new();
        serde_groups(tokens, &mut groups);
        for group in groups {
            index
                .macro_serde_attrs
                .push((format!("{} macro {name}", self.file), group));
        }
        macro_option_readers(tokens, &mut index.option_readers);
    }

    fn items(&mut self, items: &[Item], index: &mut Index) {
        for item in items {
            if is_test_cfg(item_attrs(item)) {
                if let Item::Mod(module) = item {
                    if module.content.is_none() {
                        self.test_modules.push(module.ident.to_string());
                    }
                }
                continue;
            }
            match item {
                Item::Struct(item) => {
                    let decl = self.decl(
                        &item.ident,
                        &item.attrs,
                        Body::Struct(item.fields.clone()),
                        &item.generics,
                    );
                    index.decls.entry(decl.name.clone()).or_default().push(decl);
                }
                Item::Enum(item) => {
                    let body = Body::Enum(item.variants.iter().cloned().collect());
                    let decl = self.decl(&item.ident, &item.attrs, body, &item.generics);
                    index.decls.entry(decl.name.clone()).or_default().push(decl);
                }
                Item::Impl(item) => {
                    if let Some(target) = impl_target(item, "Serialize") {
                        let first = item.impl_token.span.start().line;
                        let last = item.brace_token.span.close().end().line;
                        index
                            .hand_serialize
                            .entry(target)
                            .or_default()
                            .extend(source_tokens(self.text, first, last));
                    }
                    for member in &item.items {
                        if let syn::ImplItem::Fn(function) = member {
                            if !is_test_cfg(&function.attrs) {
                                self.block(&function.block, index);
                            }
                        }
                    }
                }
                Item::Fn(item) => self.block(&item.block, index),
                Item::Mod(item) => {
                    if let Some((_, nested)) = &item.content {
                        self.items(nested, index);
                    }
                }
                Item::Macro(item) => {
                    let name = item
                        .ident
                        .as_ref()
                        .map(ToString::to_string)
                        .unwrap_or_default();
                    self.invocation(&item.mac, &name, index);
                }
                _ => {}
            }
        }
    }
}

fn serde_metas(attrs: &[Attribute]) -> SerdeMetas {
    let mut out = Vec::new();
    for attr in attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            let key = meta
                .path
                .get_ident()
                .map(ToString::to_string)
                .unwrap_or_default();
            if meta.input.peek(syn::Token![=]) {
                let value = match meta.value()?.parse()? {
                    syn::Lit::Str(text) => Some(text.value()),
                    _ => None,
                };
                out.push((key, value));
            } else if meta.input.peek(syn::token::Paren) {
                let content;
                syn::parenthesized!(content in meta.input);
                content.parse::<TokenStream>()?;
                out.push((key, None));
            } else {
                out.push((key, None));
            }
            Ok(())
        })
        .unwrap();
    }
    out
}

fn has(metas: &SerdeMetas, key: &str) -> bool {
    metas.iter().any(|(name, _)| name == key)
}

fn value<'a>(metas: &'a SerdeMetas, key: &str) -> Option<&'a str> {
    metas
        .iter()
        .find(|(name, _)| name == key)
        .and_then(|(_, value)| value.as_deref())
}

fn last_segment(ty: &syn::Type) -> Option<(&syn::Ident, &syn::PathArguments)> {
    match ty {
        syn::Type::Path(ty) => ty
            .path
            .segments
            .last()
            .map(|segment| (&segment.ident, &segment.arguments)),
        syn::Type::Reference(ty) => last_segment(&ty.elem),
        syn::Type::Group(ty) => last_segment(&ty.elem),
        _ => None,
    }
}

fn first_type_argument(arguments: &syn::PathArguments) -> Option<&syn::Type> {
    let syn::PathArguments::AngleBracketed(arguments) = arguments else {
        return None;
    };
    arguments.args.iter().find_map(|argument| match argument {
        syn::GenericArgument::Type(ty) => Some(ty),
        _ => None,
    })
}

fn field_name(field: &syn::Field) -> String {
    let metas = serde_metas(&field.attrs);
    value(&metas, "rename").map_or_else(
        || field.ident.as_ref().unwrap().to_string(),
        ToOwned::to_owned,
    )
}

impl Index {
    fn unique(&self, name: &str) -> Option<&Decl> {
        let decls = self.decls.get(name)?;
        let serialized: Vec<&Decl> = decls
            .iter()
            .filter(|decl| derives(&decl.attrs, "Serialize") || derives(&decl.attrs, "Deserialize"))
            .collect();
        match (serialized.as_slice(), decls.as_slice()) {
            ([decl], _) => Some(decl),
            (_, [decl]) => Some(decl),
            _ => None,
        }
    }

    fn is_collection(&self, ty: &syn::Type, depth: usize) -> bool {
        let Some((ident, arguments)) = last_segment(ty) else {
            return matches!(ty, syn::Type::Slice(_));
        };
        let name = ident.to_string();
        if COLLECTIONS.contains(&name.as_str()) {
            return true;
        }
        if name == "Box" {
            return first_type_argument(arguments)
                .is_some_and(|inner| self.is_collection(inner, depth));
        }
        if depth > 4 {
            return false;
        }
        let Some(decl) = self.unique(&name) else {
            return false;
        };
        match &decl.body {
            Body::Struct(Fields::Unnamed(fields)) if fields.unnamed.len() == 1 => {
                self.is_collection(&fields.unnamed[0].ty, depth + 1)
            }
            _ => false,
        }
    }

    fn payload(&self, ty: &syn::Type, owner: Option<&Decl>, depth: usize) -> Payload {
        let Some((ident, arguments)) = last_segment(ty) else {
            return Payload::Value;
        };
        let name = ident.to_string();
        if matches!(name.as_str(), "Box" | "Arc" | "Rc") {
            return first_type_argument(arguments)
                .map_or(Payload::Value, |inner| self.payload(inner, owner, depth));
        }
        if let Some(owner) = owner.filter(|owner| owner.generics.contains(&name)) {
            return owner.generic_defaults.get(&name).map_or_else(
                || Payload::Unresolved(format!("generic parameter {name} has no default")),
                |default| self.payload(default, None, depth + 1),
            );
        }
        if depth > 6 {
            return Payload::Unresolved(format!("{name} nests too deep"));
        }
        let Some(decl) = self.unique(&name) else {
            if self.decls.contains_key(&name) {
                return Payload::Unresolved(format!("{name} has more than one declaration"));
            }
            return Payload::Value;
        };
        let metas = serde_metas(&decl.attrs);
        if let Some(target) = value(&metas, "into") {
            return self.payload(&syn::parse_str(target).unwrap(), owner, depth + 1);
        }
        let Body::Struct(Fields::Named(fields)) = &decl.body else {
            return Payload::Value;
        };
        if has(&metas, "transparent") {
            return self.payload(&fields.named[0].ty, Some(decl), depth + 1);
        }
        if !derives(&decl.attrs, "Serialize") {
            if let Some(tokens) = self.hand_serialize.get(&name) {
                return self.hand_payload(&name, tokens);
            }
        }
        let mut has_kind = false;
        for field in &fields.named {
            let field_metas = serde_metas(&field.attrs);
            if has(&field_metas, "skip") || has(&field_metas, "skip_serializing") {
                continue;
            }
            if has(&field_metas, "flatten") {
                match self.payload(&field.ty, Some(decl), depth + 1) {
                    Payload::Struct {
                        has_kind: flattened,
                    } => has_kind |= flattened,
                    other => return Payload::Unresolved(format!("{name} flattens {other:?}")),
                }
            }
            has_kind |= field_name(field) == "kind";
        }
        Payload::Struct { has_kind }
    }

    /// A hand `Serialize` writes a struct when it opens a struct or a map, or
    /// when it delegates to a struct that derives `Serialize`.
    fn hand_payload(&self, name: &str, tokens: &[String]) -> Payload {
        let has_kind = tokens.iter().any(|token| token == "\"kind\"");
        let opens_struct = tokens.iter().any(|token| {
            matches!(
                token.as_str(),
                "serialize_struct" | "serialize_map" | "struct"
            )
        });
        let delegates = tokens.iter().any(|token| {
            token != name
                && self.unique(token).is_some_and(|decl| {
                    matches!(decl.body, Body::Struct(Fields::Named(_)))
                        && derives(&decl.attrs, "Serialize")
                })
        });
        if opens_struct || delegates {
            Payload::Struct { has_kind }
        } else {
            Payload::Unresolved(format!("hand Serialize of {name} writes no struct"))
        }
    }

    /// The tag form §2.5 gives an enum with these variants.
    fn expected_form(
        &self,
        decl: &Decl,
        variants: &[&Variant],
        problems: &mut Vec<String>,
    ) -> (Form, String) {
        if variants
            .iter()
            .all(|variant| matches!(variant.fields, Fields::Unit))
        {
            return (Form::Name, String::new());
        }
        for variant in variants {
            let reason = match &variant.fields {
                Fields::Unit => None,
                Fields::Named(fields) => fields
                    .named
                    .iter()
                    .any(|field| field_name(field) == "kind")
                    .then(|| format!("{} has a field named kind", variant.ident)),
                Fields::Unnamed(fields) if fields.unnamed.len() != 1 => {
                    Some(format!("{} holds a tuple", variant.ident))
                }
                Fields::Unnamed(fields) => match self.payload(&fields.unnamed[0].ty, Some(decl), 0)
                {
                    Payload::Struct { has_kind: false } => None,
                    Payload::Struct { has_kind: true } => Some(format!(
                        "{} wraps a struct with a field named kind",
                        variant.ident
                    )),
                    Payload::Value => Some(format!(
                        "{} wraps a value that is not a struct",
                        variant.ident
                    )),
                    Payload::Unresolved(why) => {
                        problems.push(format!(
                            "{}:{} {}::{}: payload unresolved: {why}",
                            decl.file, decl.line, decl.name, variant.ident
                        ));
                        None
                    }
                },
            };
            if let Some(reason) = reason {
                return (Form::Adjacent, reason);
            }
        }
        (Form::Internal, String::new())
    }
}

fn check_enum(index: &Index, decl: &Decl, variants: &[Variant], problems: &mut Vec<String>) {
    let live: Vec<&Variant> = variants
        .iter()
        .filter(|variant| !has(&serde_metas(&variant.attrs), "skip"))
        .collect();
    let (expected, reason) = index.expected_form(decl, &live, problems);
    let metas = serde_metas(&decl.attrs);
    let untagged = has(&metas, "untagged")
        || live
            .iter()
            .any(|variant| has(&serde_metas(&variant.attrs), "untagged"));
    let actual = match (value(&metas, "tag"), value(&metas, "content"), untagged) {
        (None, None, false) => Some(Form::Name),
        (Some("kind"), None, false) => Some(Form::Internal),
        (Some("kind"), Some("value"), false) => Some(Form::Adjacent),
        _ => None,
    };
    if actual != Some(expected) {
        problems.push(format!(
            "{}:{} {}: enum must take form {expected:?} ({reason}), states tag {:?}, content {:?}, untagged {untagged}",
            decl.file,
            decl.line,
            decl.name,
            value(&metas, "tag"),
            value(&metas, "content"),
        ));
    }
}

fn check_fields(
    index: &Index,
    fields: &Fields,
    at: &str,
    writes: bool,
    reads: bool,
    problems: &mut Vec<String>,
) {
    let Fields::Named(fields) = fields else {
        return;
    };
    for field in &fields.named {
        let metas = serde_metas(&field.attrs);
        if has(&metas, "skip") {
            continue;
        }
        let ident = field.ident.as_ref().unwrap();
        let at = format!("{at}.{ident}@{}", ident.span().start().line);
        let skip = value(&metas, "skip_serializing_if");
        let reader = value(&metas, "deserialize_with").or_else(|| value(&metas, "with"));
        if reader.is_some_and(|reader| reader.contains("nullable")) {
            problems.push(format!("{at}: reads null through a nullable reader"));
        }
        if last_segment(&field.ty).is_some_and(|(ident, _)| ident == "Option") {
            if writes && skip != Some("Option::is_none") && !has(&metas, "serialize_with") {
                problems.push(format!("{at}: Option field writes null for None"));
            }
            if reads {
                let named_reader = reader.is_some_and(|reader| {
                    let name = reader.rsplit("::").next().unwrap_or(reader);
                    let name = name.split('<').next().unwrap_or(name);
                    name == OPTION_READER || index.option_readers.contains(name)
                });
                if !has(&metas, "default") || !named_reader {
                    problems.push(format!(
                        "{at}: Option field is not read by a default named_present reader"
                    ));
                }
            }
        } else {
            if let Some(skip) = skip {
                problems.push(format!("{at}: field is left out by {skip}"));
            }
            if reads && has(&metas, "default") && index.is_collection(&field.ty, 0) {
                problems.push(format!(
                    "{at}: collection field reads its omission as empty"
                ));
            }
            let is_map = last_segment(&field.ty)
                .is_some_and(|(ident, _)| MAPS.contains(&ident.to_string().as_str()));
            if reads && is_map && !reader.is_some_and(|reader| reader.contains("distinct_keys")) {
                problems.push(format!("{at}: map field reads a repeated key"));
            }
        }
    }
}

fn check_macro_attrs(index: &Index, problems: &mut Vec<String>) {
    for (at, tokens) in &index.macro_serde_attrs {
        for token in tokens {
            if matches!(token, TokenTree::Ident(ident) if ident == "untagged") {
                problems.push(format!("{at}: states untagged"));
            }
        }
        for window in tokens.windows(3) {
            let [TokenTree::Ident(key), TokenTree::Punct(eq), TokenTree::Literal(literal)] = window
            else {
                continue;
            };
            if eq.as_char() != '=' {
                continue;
            }
            let literal = literal.to_string();
            let refused = match key.to_string().as_str() {
                "skip_serializing_if" => literal != "\"Option::is_none\"",
                "tag" => literal != "\"kind\"",
                "content" => literal != "\"value\"",
                "deserialize_with" | "with" => literal.contains("nullable"),
                _ => false,
            };
            if refused {
                problems.push(format!("{at}: states {key} = {literal}"));
            }
        }
    }
}

fn index(root: &Path) -> Index {
    let mut index = Index::default();
    for crate_dir in WIRE_CRATES {
        let mut files = Vec::new();
        rust_files(&root.join(crate_dir), &mut files);
        files.sort();
        let mut parsed = Vec::new();
        for path in &files {
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if is_test_path(&relative) {
                continue;
            }
            let text = std::fs::read_to_string(path).unwrap();
            let file = syn::parse_file(&text).unwrap_or_else(|error| panic!("{relative}: {error}"));
            parsed.push((relative, text, file));
        }
        let mut excluded = Vec::new();
        for (relative, text, file) in &parsed {
            let mut walker = Walker {
                file: relative,
                text,
                test_modules: Vec::new(),
            };
            walker.items(&file.items, &mut Index::default());
            let dir = if relative.ends_with("/lib.rs") || relative.ends_with("/mod.rs") {
                relative.rsplit_once('/').unwrap().0.to_owned()
            } else {
                relative.trim_end_matches(".rs").to_owned()
            };
            for module in walker.test_modules {
                excluded.push(format!("{dir}/{module}.rs"));
                excluded.push(format!("{dir}/{module}/"));
            }
        }
        for (relative, text, file) in &parsed {
            let is_excluded = excluded.iter().any(|path| {
                relative == path || (path.ends_with('/') && relative.starts_with(path.as_str()))
            });
            if is_excluded {
                continue;
            }
            let mut walker = Walker {
                file: relative,
                text,
                test_modules: Vec::new(),
            };
            walker.items(&file.items, &mut index);
        }
    }
    index
}

/// Every departure from the §2.5 wire form among the serialized types of the
/// wire crates under `root`.
pub(crate) fn census(root: &Path) -> Vec<String> {
    let index = index(root);
    let mut problems = Vec::new();
    for decl in index.decls.values().flatten() {
        let metas = serde_metas(&decl.attrs);
        let writes =
            derives(&decl.attrs, "Serialize") && !has(&metas, "into") && !has(&metas, "remote");
        let reads = derives(&decl.attrs, "Deserialize")
            && !has(&metas, "from")
            && !has(&metas, "try_from")
            && !has(&metas, "remote");
        if !writes && !reads {
            continue;
        }
        let at = format!("{}:{} {}", decl.file, decl.line, decl.name);
        match &decl.body {
            Body::Enum(variants) => {
                check_enum(&index, decl, variants, &mut problems);
                for variant in variants {
                    if !has(&serde_metas(&variant.attrs), "skip") {
                        let at = format!("{at}::{}", variant.ident);
                        check_fields(&index, &variant.fields, &at, writes, reads, &mut problems);
                    }
                }
            }
            Body::Struct(fields) => {
                if reads && has(&metas, "default") {
                    problems.push(format!(
                        "{at}: struct reads every omitted field as its default"
                    ));
                }
                check_fields(&index, fields, &at, writes, reads, &mut problems);
            }
        }
    }
    check_macro_attrs(&index, &mut problems);
    problems.sort();
    problems
}

#[test]
fn every_wire_type_states_the_v11_wire_form() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let problems = census(&root);
    assert!(
        problems.is_empty(),
        "{} serialized types depart from the docs/cadir-v11.md §2.5 wire form:\n{}",
        problems.len(),
        problems.join("\n")
    );
}
