// SPDX-License-Identifier: Apache-2.0
//! Every serialized type of `cadmpeg-ir` and `cadmpeg-core` states the CADIR
//! v11 wire form (`docs/cadir-v11.md` §2.5) in its serde attributes.
//!
//! The census parses the two crates' source at run time. It finds each enum
//! that derives `Serialize` or `Deserialize` and computes the one tag form the
//! spec gives it from its variants. It finds each field, named or in a newtype,
//! and checks that an `Option` is omitted for `None` and refuses `null`, and
//! that no field other than an `Option` is ever left out. Test files and
//! `#[cfg(test)]` items are not wire types.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use proc_macro2::{Delimiter, TokenStream, TokenTree};
use syn::spanned::Spanned;
use syn::{Attribute, Fields, Item, Variant};

const WIRE_CRATES: [&str; 2] = ["crates/cadmpeg-ir/src", "crates/cadmpeg-core/src"];

const COLLECTIONS: [&str; 8] = [
    "Vec", "VecDeque", "BTreeMap", "HashMap", "BTreeSet", "HashSet", "IndexMap", "Map",
];

const MAPS: [&str; 3] = ["BTreeMap", "HashMap", "IndexMap"];

const OPTION_READER_MACRO: &str = "named_optional_field";

const OPTION_READER: &str = "named_present";

const TAG_ONLY_READER: &str = "::tag_only::read";

const VALUE_REFUSER: &str = "::tag_only::refuse_value";

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
    /// Type aliases by name: the declaring file and the aliased type.
    aliases: BTreeMap<String, Vec<(String, syn::Type)>>,
    /// The names each file imports, with the path each is imported from.
    imports: BTreeMap<String, Vec<(String, Vec<String>)>>,
    hand_serialize: BTreeMap<String, Vec<String>>,
    option_readers: BTreeSet<String>,
    /// Functions that forward to another helper, by name, with each helper a
    /// function of that name forwards to.
    option_forwards: BTreeMap<String, BTreeSet<String>>,
    macro_serde_attrs: Vec<(String, Vec<TokenTree>)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Form {
    Name,
    Internal,
    Adjacent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Holder {
    Struct,
    Variant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Payload {
    Struct {
        has_kind: bool,
    },
    Value,
    /// A generic parameter with no default: no declaration proves it a struct.
    Generic(String),
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
        Item::Type(item) => &item.attrs,
        Item::Use(item) => &item.attrs,
        _ => &[],
    }
}

/// Each name a `use` tree imports, with its full path. A renamed or glob
/// import names no declaration by its own name, so it is not recorded.
fn use_paths(tree: &syn::UseTree, prefix: &mut Vec<String>, out: &mut Vec<(String, Vec<String>)>) {
    match tree {
        syn::UseTree::Path(path) => {
            prefix.push(path.ident.to_string());
            use_paths(&path.tree, prefix, out);
            prefix.pop();
        }
        syn::UseTree::Name(name) if name.ident == "self" => {
            if let Some(last) = prefix.last() {
                out.push((last.clone(), prefix.clone()));
            }
        }
        syn::UseTree::Name(name) => {
            let mut path = prefix.clone();
            path.push(name.ident.to_string());
            out.push((name.ident.to_string(), path));
        }
        syn::UseTree::Group(group) => {
            for tree in &group.items {
                use_paths(tree, prefix, out);
            }
        }
        syn::UseTree::Rename(_) | syn::UseTree::Glob(_) => {}
    }
}

/// The name a wire crate's source directory is imported by.
fn crate_name(crate_dir: &str) -> String {
    crate_dir
        .trim_end_matches("/src")
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .replace('-', "_")
}

/// The crate and module path of a source file of a wire crate.
fn module_of(file: &str) -> Option<(String, Vec<String>)> {
    WIRE_CRATES.iter().find_map(|crate_dir| {
        let rest = file.strip_prefix(crate_dir)?.strip_prefix('/')?;
        let name = crate_name(crate_dir);
        let mut module: Vec<String> = rest
            .trim_end_matches(".rs")
            .split('/')
            .map(ToOwned::to_owned)
            .collect();
        if module.len() == 1 && module[0] == "lib"
            || module.last().is_some_and(|last| last == "mod")
        {
            module.pop();
        }
        Some((name, module))
    })
}

/// The crate and module a `use` path names its last segment in, read from a
/// file of module `module` in crate `krate`.
fn import_module(krate: &str, module: &[String], path: &[String]) -> (String, Vec<String>) {
    let segments = path.split_last().map_or(path, |(_, rest)| rest);
    let mut krate = krate.to_owned();
    let mut resolved = module.to_vec();
    let mut segments = segments.iter().peekable();
    match segments.peek().map(|segment| segment.as_str()) {
        Some("crate") => {
            resolved.clear();
            segments.next();
        }
        Some("self") => {
            segments.next();
        }
        Some(name) if WIRE_CRATES.iter().any(|dir| crate_name(dir) == name) => {
            name.clone_into(&mut krate);
            resolved.clear();
            segments.next();
        }
        _ => {}
    }
    while segments.peek().is_some_and(|segment| *segment == "super") {
        resolved.pop();
        segments.next();
    }
    resolved.extend(segments.cloned());
    (krate, resolved)
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

/// The helper `function` forwards to, when its body is one call of a helper
/// named without a path on its one parameter, followed by nothing but `.map`
/// calls. `Result::map` rewrites only `Ok`, so such a function refuses
/// whatever its callee refuses. The source census reads a forward by the same
/// rule.
fn forwarded_reader(function: &syn::ItemFn) -> Option<String> {
    let mut parameters = function.sig.inputs.iter();
    let (Some(syn::FnArg::Typed(parameter)), None) = (parameters.next(), parameters.next()) else {
        return None;
    };
    let syn::Pat::Ident(parameter) = parameter.pat.as_ref() else {
        return None;
    };
    let [syn::Stmt::Expr(body, None)] = function.block.stmts.as_slice() else {
        return None;
    };
    let mut expression = body;
    while let syn::Expr::MethodCall(call) = expression {
        if call.method != "map" || call.args.len() != 1 {
            return None;
        }
        expression = &call.receiver;
    }
    let syn::Expr::Call(call) = expression else {
        return None;
    };
    let syn::Expr::Path(callee) = call.func.as_ref() else {
        return None;
    };
    let callee = callee.path.get_ident()?;
    let mut arguments = call.args.iter();
    let (Some(syn::Expr::Path(argument)), None) = (arguments.next(), arguments.next()) else {
        return None;
    };
    (argument.path.get_ident()? == &parameter.ident).then(|| callee.to_string())
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
                Item::Fn(item) => {
                    if let Some(callee) = forwarded_reader(item) {
                        index
                            .option_forwards
                            .entry(item.sig.ident.to_string())
                            .or_default()
                            .insert(callee);
                    }
                    self.block(&item.block, index);
                }
                Item::Mod(item) => {
                    if let Some((_, nested)) = &item.content {
                        self.items(nested, index);
                    }
                }
                Item::Type(item) => {
                    index
                        .aliases
                        .entry(item.ident.to_string())
                        .or_default()
                        .push((self.file.to_owned(), (*item.ty).clone()));
                }
                Item::Use(item) => {
                    let imports = index.imports.entry(self.file.to_owned()).or_default();
                    use_paths(&item.tree, &mut Vec::new(), imports);
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

/// The generic parameter of `owner` whose associated type `ty` names, as in
/// `S::Curve` or `<S as Space>::Curve`. Its payload is whatever each
/// implementation chooses, so it is no declaration that shares its last name.
fn associated_parameter(ty: &syn::Type, owner: &Decl) -> Option<String> {
    let syn::Type::Path(path) = ty else {
        return None;
    };
    let parameter = match &path.qself {
        Some(qself) => last_segment(&qself.ty)?.0.to_string(),
        None if path.path.segments.len() > 1 => path.path.segments.first()?.ident.to_string(),
        None => return None,
    };
    owner.generics.contains(&parameter).then_some(parameter)
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
    /// Whether `name` is a `named_optional_field!` reader, or a function
    /// every definition of which forwards to one. A forward refuses whatever
    /// its callee refuses, so it reads a set through the set reader and still
    /// refuses `null` at its key.
    fn is_option_reader(&self, name: &str) -> bool {
        self.option_readers.contains(name)
            || self.option_forwards.get(name).is_some_and(|callees| {
                callees
                    .iter()
                    .all(|callee| self.option_readers.contains(callee))
            })
    }

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

    /// Of the files that declare a name, the one file `from` means by it: its
    /// own declaration, else the one it imports. `None` when neither picks
    /// exactly one.
    fn pick<'a>(&self, files: &[&'a str], name: &str, from: &str) -> Option<&'a str> {
        if let [file] = files {
            return Some(file);
        }
        self.in_scope(files, name, from)
    }

    /// Of the files that declare a name, the one whose declaration `from`
    /// brings into scope: `from` itself, else the one it imports. `None`
    /// when neither picks exactly one.
    fn in_scope<'a>(&self, files: &[&'a str], name: &str, from: &str) -> Option<&'a str> {
        if let Some(file) = files.iter().find(|file| **file == from) {
            return Some(file);
        }
        let (krate, module) = module_of(from)?;
        let targets: Vec<(String, Vec<String>)> = self
            .imports
            .get(from)?
            .iter()
            .filter(|(leaf, _)| leaf == name)
            .map(|(_, path)| import_module(&krate, &module, path))
            .collect();
        let mut matched = files.iter().filter(|file| {
            module_of(file).is_some_and(|(file_crate, file_module)| {
                targets.iter().any(|(target_crate, target_module)| {
                    *target_crate == file_crate && target_module.starts_with(&file_module)
                })
            })
        });
        match (matched.next(), matched.next()) {
            (Some(file), None) => Some(file),
            _ => None,
        }
    }

    /// The declaration a type named `name`, written in file `from`, refers to.
    fn resolve(&self, name: &str, from: &str) -> Option<&Decl> {
        if let Some(decl) = self.unique(name) {
            return Some(decl);
        }
        let decls = self.decls.get(name)?;
        let serialized: Vec<&Decl> = decls
            .iter()
            .filter(|decl| derives(&decl.attrs, "Serialize") || derives(&decl.attrs, "Deserialize"))
            .collect();
        let candidates = if serialized.is_empty() {
            decls.iter().collect()
        } else {
            serialized
        };
        let files: Vec<&str> = candidates.iter().map(|decl| decl.file.as_str()).collect();
        let file = self.pick(&files, name, from)?;
        let mut in_file = candidates.into_iter().filter(|decl| decl.file == file);
        match (in_file.next(), in_file.next()) {
            (Some(decl), None) => Some(decl),
            _ => None,
        }
    }

    /// The declaration a module-qualified type such as `mesh::Payload`,
    /// written in file `from`, refers to: the one in the module its qualifier
    /// names. `None` for an unqualified type or when no single one matches.
    fn qualified(&self, ty: &syn::Type, name: &str, from: &str) -> Option<&Decl> {
        let syn::Type::Path(ty) = ty else {
            return None;
        };
        if ty.path.segments.len() < 2 {
            return None;
        }
        let path: Vec<String> = ty
            .path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect();
        let (krate, module) = module_of(from)?;
        let target = import_module(&krate, &module, &path);
        let mut matched = self
            .decls
            .get(name)?
            .iter()
            .filter(|decl| module_of(&decl.file).as_ref() == Some(&target));
        match (matched.next(), matched.next()) {
            (Some(decl), None) => Some(decl),
            _ => None,
        }
    }

    /// The type an alias named `name`, written in file `from`, stands for,
    /// with the file that declares the alias.
    fn alias(&self, name: &str, from: &str) -> Option<(&str, &syn::Type)> {
        self.alias_in(name, from, Self::pick)
    }

    /// The alias named `name` that file `from` declares or imports, with the
    /// file that declares it. Such an alias is what `from` means by the name,
    /// whatever another module declares under it.
    fn scoped_alias(&self, name: &str, from: &str) -> Option<(&str, &syn::Type)> {
        self.alias_in(name, from, Self::in_scope)
    }

    fn alias_in<'a>(
        &'a self,
        name: &str,
        from: &str,
        choose: impl for<'f> Fn(&Self, &[&'f str], &str, &str) -> Option<&'f str>,
    ) -> Option<(&'a str, &'a syn::Type)> {
        let aliases = self.aliases.get(name)?;
        let files: Vec<&str> = aliases.iter().map(|(file, _)| file.as_str()).collect();
        let file = choose(self, &files, name, from)?;
        let mut in_file = aliases.iter().filter(|(alias_file, _)| alias_file == file);
        match (in_file.next(), in_file.next()) {
            (Some((file, ty)), None) => Some((file.as_str(), ty)),
            _ => None,
        }
    }

    /// The type a field of type `ty`, written in file `from`, holds: the
    /// target of a nongeneric alias that no declaration shares a name with,
    /// followed through further such aliases, or `ty` itself.
    fn field_type<'a>(&'a self, ty: &'a syn::Type, from: &'a str) -> &'a syn::Type {
        let (mut ty, mut from) = (ty, from);
        for _ in 0..6 {
            let Some((ident, syn::PathArguments::None)) = last_segment(ty) else {
                break;
            };
            let name = ident.to_string();
            if self.decls.contains_key(&name) {
                break;
            }
            let Some((file, aliased)) = self.alias(&name, from) else {
                break;
            };
            (ty, from) = (aliased, file);
        }
        ty
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

    /// Whether `ty` is a newtype struct over an `Option`, which writes `null`
    /// for `None` as the `Option` does.
    fn is_optional_newtype(&self, ty: &syn::Type) -> bool {
        let Some((ident, _)) = last_segment(ty) else {
            return false;
        };
        let Some(decl) = self.unique(&ident.to_string()) else {
            return false;
        };
        let Body::Struct(fields) = &decl.body else {
            return false;
        };
        let newtype = match fields {
            Fields::Unnamed(_) => true,
            Fields::Named(_) => has(&serde_metas(&decl.attrs), "transparent"),
            Fields::Unit => false,
        };
        let mut serialized = fields
            .iter()
            .filter(|field| !has(&serde_metas(&field.attrs), "skip"));
        let (Some(field), None) = (serialized.next(), serialized.next()) else {
            return false;
        };
        newtype && last_segment(&field.ty).is_some_and(|(ident, _)| ident == "Option")
    }

    /// The payload a type writes. `from` is the file the type is written in,
    /// which names the declaration a repeated name refers to.
    fn payload(&self, ty: &syn::Type, owner: Option<&Decl>, from: &str, depth: usize) -> Payload {
        let Some((ident, arguments)) = last_segment(ty) else {
            return Payload::Value;
        };
        let name = ident.to_string();
        if matches!(name.as_str(), "Box" | "Arc" | "Rc") {
            return first_type_argument(arguments).map_or(Payload::Value, |inner| {
                self.payload(inner, owner, from, depth)
            });
        }
        if let Some(parameter) = owner.and_then(|owner| associated_parameter(ty, owner)) {
            return Payload::Generic(format!("{parameter}::{name}"));
        }
        if let Some(owner) = owner.filter(|owner| owner.generics.contains(&name)) {
            return owner.generic_defaults.get(&name).map_or_else(
                || Payload::Generic(name.clone()),
                |default| self.payload(default, None, &owner.file, depth + 1),
            );
        }
        if depth > 6 {
            return Payload::Unresolved(format!("{name} nests too deep"));
        }
        if let Some((file, aliased)) = self.scoped_alias(&name, from) {
            return self.payload(aliased, None, file, depth + 1);
        }
        let Some(decl) = self
            .qualified(ty, &name, from)
            .or_else(|| self.resolve(&name, from))
        else {
            if self.decls.contains_key(&name) {
                return Payload::Unresolved(format!("{name} has more than one declaration"));
            }
            if let Some((file, aliased)) = self.alias(&name, from) {
                return self.payload(aliased, None, file, depth + 1);
            }
            if self.aliases.contains_key(&name) {
                return Payload::Unresolved(format!("{name} has more than one alias"));
            }
            return Payload::Value;
        };
        let metas = serde_metas(&decl.attrs);
        if let Some(target) = value(&metas, "into") {
            return self.payload(
                &syn::parse_str(target).unwrap(),
                owner,
                &decl.file,
                depth + 1,
            );
        }
        let Body::Struct(Fields::Named(fields)) = &decl.body else {
            return Payload::Value;
        };
        if has(&metas, "transparent") {
            return self.payload(&fields.named[0].ty, Some(decl), &decl.file, depth + 1);
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
                match self.payload(&field.ty, Some(decl), &decl.file, depth + 1) {
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
    /// when it delegates to a struct that derives `Serialize`; one that
    /// writes a sequence and opens neither writes a value.
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
        let writes_sequence = tokens
            .iter()
            .any(|token| matches!(token.as_str(), "collect_seq" | "serialize_seq"));
        if opens_struct || delegates {
            Payload::Struct { has_kind }
        } else if writes_sequence {
            Payload::Value
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
                Fields::Unnamed(fields) => {
                    match self.payload(&fields.unnamed[0].ty, Some(decl), &decl.file, 0) {
                        Payload::Struct { has_kind: false } => None,
                        Payload::Struct { has_kind: true } => Some(format!(
                            "{} wraps a struct with a field named kind",
                            variant.ident
                        )),
                        Payload::Value => Some(format!(
                            "{} wraps a value that is not a struct",
                            variant.ident
                        )),
                        Payload::Generic(parameter) => Some(format!(
                        "{} wraps the generic parameter {parameter}, which need not be a struct",
                        variant.ident
                    )),
                        Payload::Unresolved(why) => {
                            problems.push(format!(
                                "{}:{} {}::{}: payload unresolved: {why}",
                                decl.file, decl.line, decl.name, variant.ident
                            ));
                            None
                        }
                    }
                }
            };
            if let Some(reason) = reason {
                return (Form::Adjacent, reason);
            }
        }
        (Form::Internal, String::new())
    }
}

fn check_enum(
    index: &Index,
    decl: &Decl,
    variants: &[Variant],
    reads: bool,
    problems: &mut Vec<String>,
) {
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
    // serde reads an internally tagged unit variant by its tag and skips every
    // other key, `deny_unknown_fields` or not; it reads an adjacently tagged
    // unit variant's stated `value` as a unit, so `null` is a second spelling.
    let unit_reader = match actual {
        Some(Form::Internal) => Some(TAG_ONLY_READER),
        Some(Form::Adjacent) => Some(VALUE_REFUSER),
        Some(Form::Name) | None => None,
    };
    if let Some(expected) = unit_reader.filter(|_| reads) {
        for variant in &live {
            let metas = serde_metas(&variant.attrs);
            let reader = value(&metas, "deserialize_with");
            let refuses = reader.is_some_and(|reader| {
                reader
                    .strip_prefix("cadmpeg_core")
                    .or(reader.strip_prefix("crate"))
                    == Some(expected)
            });
            if matches!(variant.fields, Fields::Unit) && !refuses {
                problems.push(format!(
                    "{}:{} {}::{}: unit variant does not read through cadmpeg_core{expected}",
                    decl.file,
                    variant.ident.span().start().line,
                    decl.name,
                    variant.ident
                ));
            }
        }
    }
}

fn check_fields(
    index: &Index,
    fields: &Fields,
    holder: Holder,
    at: &str,
    writes: bool,
    reads: bool,
    problems: &mut Vec<String>,
) {
    let newtype = matches!(fields, Fields::Unnamed(_));
    // `at` is `file:line name`: the file declares the fields.
    let from = at.split_once(':').map_or(at, |(file, _)| file);
    for (position, field) in fields.iter().enumerate() {
        let metas = serde_metas(&field.attrs);
        let ty = index.field_type(&field.ty, from);
        if has(&metas, "skip") {
            continue;
        }
        let at = match &field.ident {
            Some(ident) => format!("{at}.{ident}@{}", ident.span().start().line),
            None => format!("{at}.{position}@{}", field.ty.span().start().line),
        };
        let skip = value(&metas, "skip_serializing_if");
        let reader = value(&metas, "deserialize_with").or_else(|| value(&metas, "with"));
        if reader.is_some_and(|reader| reader.contains("nullable")) {
            problems.push(format!("{at}: reads null through a nullable reader"));
        }
        let named_reader = reader.is_some_and(|reader| {
            let name = reader.rsplit("::").next().unwrap_or(reader);
            let name = name.split('<').next().unwrap_or(name);
            name == OPTION_READER || index.is_option_reader(name)
        });
        if last_segment(ty).is_some_and(|(ident, _)| ident == "Option") {
            if newtype {
                // A derived newtype variant writes `value` for every payload,
                // so it writes `"value": null` for `None`. A newtype struct
                // writes its `Option` as itself, and the fields that hold it
                // omit `None`.
                if writes && holder == Holder::Variant {
                    problems.push(format!(
                        "{at}: newtype variant wraps an Option and writes null for None"
                    ));
                }
                if reads && !named_reader {
                    problems.push(format!(
                        "{at}: newtype Option is not read by a named_present reader"
                    ));
                }
                continue;
            }
            if writes && skip != Some("Option::is_none") && !has(&metas, "serialize_with") {
                problems.push(format!("{at}: Option field writes null for None"));
            }
            if reads && (!has(&metas, "default") || !named_reader) {
                problems.push(format!(
                    "{at}: Option field is not read by a default named_present reader"
                ));
            }
        } else if index.is_optional_newtype(ty) {
            if writes && skip.is_none() {
                problems.push(format!(
                    "{at}: field holding a newtype over an Option writes null for None"
                ));
            }
            if reads && !has(&metas, "default") {
                problems.push(format!(
                    "{at}: field holding a newtype over an Option refuses its omission"
                ));
            }
        } else {
            if let Some(skip) = skip {
                problems.push(format!("{at}: field is left out by {skip}"));
            }
            if reads && has(&metas, "default") && index.is_collection(ty, 0) {
                problems.push(format!(
                    "{at}: collection field reads its omission as empty"
                ));
            }
            let is_map = last_segment(ty)
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
        let mut sources = Vec::new();
        for path in &files {
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if !is_test_path(&relative) {
                sources.push((relative, std::fs::read_to_string(path).unwrap()));
            }
        }
        index_sources(&sources, &mut index);
    }
    index
}

/// Indexes one crate's non-test sources, given as `(relative path, text)`.
fn index_sources(sources: &[(String, String)], index: &mut Index) {
    let mut parsed = Vec::new();
    for (relative, text) in sources {
        let file = syn::parse_file(text).unwrap_or_else(|error| panic!("{relative}: {error}"));
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
            *relative == path || (path.ends_with('/') && relative.starts_with(path.as_str()))
        });
        if is_excluded {
            continue;
        }
        let mut walker = Walker {
            file: relative,
            text,
            test_modules: Vec::new(),
        };
        walker.items(&file.items, index);
    }
}

/// Every departure from the §2.5 wire form among the serialized types of the
/// wire crates under `root`.
pub(crate) fn census(root: &Path) -> Vec<String> {
    problems(&index(root))
}

fn problems(index: &Index) -> Vec<String> {
    let mut problems = Vec::new();
    for decl in index.decls.values().flatten() {
        let metas = serde_metas(&decl.attrs);
        let writes =
            derives(&decl.attrs, "Serialize") && !has(&metas, "into") && !has(&metas, "remote");
        let reads = (derives(&decl.attrs, "Deserialize")
            || derives(&decl.attrs, "DeserializeTagged"))
            && !has(&metas, "from")
            && !has(&metas, "try_from")
            && !has(&metas, "remote");
        if !writes && !reads {
            continue;
        }
        let at = format!("{}:{} {}", decl.file, decl.line, decl.name);
        match &decl.body {
            Body::Enum(variants) => {
                check_enum(index, decl, variants, reads, &mut problems);
                for variant in variants {
                    if !has(&serde_metas(&variant.attrs), "skip") {
                        let at = format!("{at}::{}", variant.ident);
                        check_fields(
                            index,
                            &variant.fields,
                            Holder::Variant,
                            &at,
                            writes,
                            reads,
                            &mut problems,
                        );
                    }
                }
            }
            Body::Struct(fields) => {
                if reads && has(&metas, "default") {
                    problems.push(format!(
                        "{at}: struct reads every omitted field as its default"
                    ));
                }
                check_fields(
                    index,
                    fields,
                    Holder::Struct,
                    &at,
                    writes,
                    reads,
                    &mut problems,
                );
            }
        }
    }
    check_macro_attrs(index, &mut problems);
    problems.sort();
    problems
}

const PROBE: &str = "crates/cadmpeg-ir/src/probe.rs";

const DEPARTING_PROBE: &str = r#"#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", deny_unknown_fields)]
enum Section {
    Unnamed,
    Unresolved(#[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")] Option<String>),
}
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
struct State(#[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")] Option<u32>);
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Holder {
    state: State,
}
"#;

const STATING_PROBE: &str = r#"#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", deny_unknown_fields, into = "SectionWire")]
enum Section {
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Unnamed,
    Unresolved(#[serde(deserialize_with = "deserialize_native")] Option<String>),
}
cadmpeg_core::named_optional_field!(deserialize_native, String, "value");
#[derive(Serialize)]
#[serde(tag = "kind", content = "value")]
enum SectionWire {
    Unnamed,
    Unresolved(String),
}
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
struct State(#[serde(deserialize_with = "deserialize_state")] Option<u32>);
cadmpeg_core::named_optional_field!(deserialize_state, u32, "state");
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Holder {
    #[serde(default = "State::unstated", skip_serializing_if = "State::is_unstated")]
    state: State,
}
"#;

fn probe_problems(source: &str) -> Vec<String> {
    let mut index = Index::default();
    index_sources(&[(PROBE.to_owned(), source.to_owned())], &mut index);
    problems(&index)
}

#[test]
fn the_census_reads_newtype_fields_and_adjacent_unit_variants() {
    let mut expected = vec![
        format!("{PROBE}:4 Section::Unnamed: unit variant does not read through cadmpeg_core{VALUE_REFUSER}"),
        format!("{PROBE}:3 Section::Unresolved.0@5: reads null through a nullable reader"),
        format!("{PROBE}:3 Section::Unresolved.0@5: newtype variant wraps an Option and writes null for None"),
        format!("{PROBE}:3 Section::Unresolved.0@5: newtype Option is not read by a named_present reader"),
        format!("{PROBE}:9 State.0@9: reads null through a nullable reader"),
        format!("{PROBE}:9 State.0@9: newtype Option is not read by a named_present reader"),
        format!("{PROBE}:12 Holder.state@13: field holding a newtype over an Option writes null for None"),
        format!("{PROBE}:12 Holder.state@13: field holding a newtype over an Option refuses its omission"),
    ];
    expected.sort();
    assert_eq!(probe_problems(DEPARTING_PROBE), expected);
    assert_eq!(probe_problems(STATING_PROBE), Vec::<String>::new());
}

const FORWARDING_PROBE: &str = r#"#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Holder {
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "deserialize_set")]
    set: Option<BTreeSet<u32>>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "deserialize_loose")]
    loose: Option<BTreeSet<u32>>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "deserialize_lenient")]
    lenient: Option<BTreeSet<u32>>,
}
fn deserialize_set<'de, D: Deserializer<'de>>(d: D) -> Result<Option<BTreeSet<u32>>, D::Error> {
    deserialize_members(d).map(|set| set.map(|Members(set)| set))
}
fn deserialize_loose<'de, D: Deserializer<'de>>(d: D) -> Result<Option<BTreeSet<u32>>, D::Error> {
    read_members(d).map(|set| set.map(|Members(set)| set))
}
fn deserialize_lenient<'de, D: Deserializer<'de>>(d: D) -> Result<Option<BTreeSet<u32>>, D::Error> {
    deserialize_members(d).or_else(|_| Ok(None))
}
cadmpeg_core::named_optional_field!(deserialize_members, Members, "set");
"#;

/// A helper that only forwards to a `named_optional_field!` reader is one;
/// a forward to another helper, or a call that rewrites the refusal, is not.
#[test]
fn a_forward_to_an_option_reader_reads_as_one() {
    let unread = |line: usize, field: &str| {
        format!("{PROBE}:3 Holder.{field}@{line}: Option field is not read by a default named_present reader")
    };
    assert_eq!(
        probe_problems(FORWARDING_PROBE),
        vec![unread(9, "lenient"), unread(7, "loose")]
    );
}

#[test]
fn a_field_is_read_through_a_nongeneric_alias_of_its_type() {
    let source = |attrs: &str| {
        format!(
            "pub type Maybe = Option<u32>;\n\
             pub type Again = Maybe;\n\
             cadmpeg_core::named_optional_field!(deserialize_maybe, u32, \"maybe\");\n\
             #[derive(Serialize, Deserialize)]\n\
             #[serde(deny_unknown_fields)]\n\
             struct Holder {{\n\
                 #[serde({attrs})]\n\
                 maybe: Again,\n\
             }}\n"
        )
    };
    let stated = source(
        "default, skip_serializing_if = \"Option::is_none\", deserialize_with = \"deserialize_maybe\"",
    );
    assert_eq!(probe_problems(&stated), Vec::<String>::new());
    assert_eq!(
        probe_problems(&source("skip_serializing_if = \"Option::is_none\"")),
        vec![format!(
            "{PROBE}:6 Holder.maybe@8: Option field is not read by a default named_present reader"
        )]
    );
    let shadowed = format!("{stated}#[derive(Serialize, Deserialize)] pub struct Again(u32);\n");
    assert_eq!(
        probe_problems(&shadowed),
        vec![format!(
            "{PROBE}:6 Holder.maybe@8: field is left out by Option::is_none"
        )]
    );
}

#[test]
fn a_payload_resolves_through_imports_aliases_and_generic_parameters() {
    let root =
        std::env::temp_dir().join(format!("cadmpeg-wire-form-census-{}", std::process::id()));
    let sources = [
        ("crates/cadmpeg-core/src/lib.rs", ""),
        (
            "crates/cadmpeg-ir/src/one.rs",
            "#[derive(Serialize)] pub struct Payload { pub a: u32 }\n\
             pub type Alias = Payload;\n\
             pub type Shadowed = Payload;\n",
        ),
        (
            "crates/cadmpeg-ir/src/two.rs",
            "#[derive(Serialize)] #[serde(tag = \"kind\", content = \"value\")]\n\
             pub enum Payload { A(u32) }\n\
             #[derive(Serialize)] #[serde(tag = \"kind\", content = \"value\")]\n\
             pub enum Shadowed { A(u32) }\n",
        ),
        (
            "crates/cadmpeg-ir/src/user.rs",
            "use crate::one::{Alias, Payload, Shadowed};\n\
             #[derive(Serialize)] #[serde(tag = \"kind\", content = \"value\")]\n\
             pub enum Imported { Item(Payload) }\n\
             #[derive(Serialize)] #[serde(tag = \"kind\", content = \"value\")]\n\
             pub enum Aliased { Item(Alias) }\n\
             #[derive(Serialize)] #[serde(tag = \"kind\", content = \"value\")]\n\
             pub enum ImportedAlias { Item(Shadowed) }\n\
             #[derive(Serialize)] #[serde(tag = \"kind\")]\n\
             pub enum Generic<T> { Item(T) }\n",
        ),
    ];
    for (path, text) in sources {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let problems = census(&root);
    std::fs::remove_dir_all(&root).unwrap();
    for (name, form) in [
        ("Imported", "Internal"),
        ("Aliased", "Internal"),
        ("ImportedAlias", "Internal"),
        ("Generic", "Adjacent"),
    ] {
        assert!(
            problems
                .iter()
                .any(|problem| problem.contains(&format!(" {name}: enum must take form {form}"))),
            "{name}: {problems:#?}"
        );
    }
    assert_eq!(problems.len(), 4, "{problems:#?}");
}

const ASSOCIATED_PROBE: &str = r#"#[derive(Serialize)]
pub struct Curve { pub a: u32 }
pub trait Space { type Curve; }
#[derive(Serialize)]
#[serde(tag = "kind")]
pub enum Internal<S: Space> { Item(S::Curve) }
#[derive(Serialize)]
#[serde(tag = "kind", content = "value")]
pub enum Adjacent<S: Space> { Item(S::Curve), Qualified(<S as Space>::Curve) }
"#;

#[test]
fn an_associated_type_of_a_generic_parameter_need_not_be_a_struct() {
    let problems = probe_problems(ASSOCIATED_PROBE);
    assert_eq!(problems.len(), 1, "{problems:#?}");
    assert!(
        problems[0].contains(" Internal: enum must take form Adjacent"),
        "{problems:#?}"
    );
}

const SEQUENCE_PROBE: &str = r#"pub struct Items { items: Vec<u32> }
impl Serialize for Items {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(&self.items)
    }
}
#[derive(Serialize)]
#[serde(tag = "kind")]
pub enum Internal { Item(Items) }
#[derive(Serialize)]
#[serde(tag = "kind", content = "value")]
pub enum Adjacent { Item(Items) }
"#;

#[test]
fn a_hand_serialize_that_writes_a_sequence_is_a_value_payload() {
    let problems = probe_problems(SEQUENCE_PROBE);
    assert_eq!(problems.len(), 1, "{problems:#?}");
    assert!(
        problems[0].contains(" Internal: enum must take form Adjacent"),
        "{problems:#?}"
    );
}

#[test]
fn a_module_qualified_payload_resolves_in_the_module_it_names() {
    let root = std::env::temp_dir().join(format!(
        "cadmpeg-wire-form-census-qualified-{}",
        std::process::id()
    ));
    let sources = [
        ("crates/cadmpeg-core/src/lib.rs", ""),
        (
            "crates/cadmpeg-ir/src/user/inner.rs",
            "#[derive(Serialize)] pub struct Payload { pub a: u32 }\n",
        ),
        (
            "crates/cadmpeg-ir/src/other.rs",
            "#[derive(Serialize)] #[serde(tag = \"kind\", content = \"value\")]\n\
             pub enum Payload { A(u32) }\n",
        ),
        (
            "crates/cadmpeg-ir/src/user.rs",
            "#[derive(Serialize)] #[serde(tag = \"kind\", content = \"value\")]\n\
             pub enum Qualified { Item(inner::Payload) }\n\
             #[derive(Serialize)] #[serde(tag = \"kind\", content = \"value\")]\n\
             pub enum Bare { Item(Payload) }\n",
        ),
    ];
    for (path, text) in sources {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let problems = census(&root);
    std::fs::remove_dir_all(&root).unwrap();
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains(" Qualified: enum must take form Internal")),
        "{problems:#?}"
    );
    assert!(
        problems.iter().any(|problem| problem
            .contains("Bare::Item: payload unresolved: Payload has more than one declaration")),
        "{problems:#?}"
    );
    assert_eq!(problems.len(), 3, "{problems:#?}");
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
