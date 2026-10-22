// SPDX-License-Identifier: Apache-2.0
//! Module-rooted workspace loading, with test-only items removed.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use syn::spanned::Spanned;
use syn::visit::{self, Visit};

use super::bindings::Value;
use super::rules::{self, Aliases, Body};
use super::{absolute, module_scope, Function, Graph};

fn test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attribute| {
        if attribute.path().is_ident("test") {
            return true;
        }
        if !attribute.path().is_ident("cfg") {
            return false;
        }
        let mut required = false;
        attribute
            .parse_nested_meta(|meta| {
                required = super::super::cfg_requires_test(&meta)?;
                Ok(())
            })
            .expect("parse source cfg");
        required
    })
}

fn imports(items: &[syn::Item], inherited: &Aliases, scope: &[String]) -> Aliases {
    let inherits = scope.last().is_some_and(|part| part.starts_with('$')) || items.iter().any(|item| {
        matches!(item, syn::Item::Use(item) if matches!(&item.tree, syn::UseTree::Path(path) if path.ident == "super" && matches!(path.tree.as_ref(), syn::UseTree::Glob(_))))
    });
    let mut aliases = if inherits {
        inherited.clone()
    } else {
        Aliases::new()
    };
    for item in items {
        let name = match item {
            syn::Item::Fn(item) if !test_only(&item.attrs) => Some(&item.sig.ident),
            syn::Item::Struct(item) if !test_only(&item.attrs) => Some(&item.ident),
            syn::Item::Enum(item) if !test_only(&item.attrs) => Some(&item.ident),
            syn::Item::Trait(item) if !test_only(&item.attrs) => Some(&item.ident),
            syn::Item::Mod(item) if !test_only(&item.attrs) => Some(&item.ident),
            _ => None,
        };
        if let Some(name) = name {
            let mut path = scope.to_vec();
            path.push(name.to_string());
            aliases.insert(name.to_string(), path);
        }
    }
    for item in items {
        if let syn::Item::Use(item) = item {
            if !test_only(&item.attrs) {
                rules::import(&item.tree, &[], &mut aliases);
            }
        }
    }
    for _ in 0..items.len() {
        let previous = aliases.clone();
        for item in items {
            if let syn::Item::Type(item) = item {
                if test_only(&item.attrs) {
                    continue;
                }
                if let syn::Type::Path(ty) = item.ty.as_ref() {
                    aliases.insert(
                        item.ident.to_string(),
                        rules::path(&ty.path, &previous, None),
                    );
                }
            }
        }
        if aliases == previous {
            break;
        }
    }
    for value in aliases.values_mut() {
        *value = absolute(value, scope);
    }
    aliases
}

struct LocalImports(Aliases);
impl<'ast> Visit<'ast> for LocalImports {
    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        if !test_only(&item.attrs) {
            rules::import(&item.tree, &[], &mut self.0);
        }
    }
    fn visit_item_fn(&mut self, _item: &'ast syn::ItemFn) {}
}

struct Loader<'a> {
    root: &'a Path,
    seen: BTreeSet<(PathBuf, Vec<String>)>,
    graph: Graph,
}
impl Loader<'_> {
    fn file(&mut self, file: &Path, scope: &[String], inherited: &Aliases) {
        if !self.seen.insert((file.to_owned(), scope.to_vec())) {
            return;
        }
        let text = std::fs::read_to_string(file)
            .unwrap_or_else(|error| panic!("{}: {error}", file.display()));
        let parsed =
            syn::parse_file(&text).unwrap_or_else(|error| panic!("{}: {error}", file.display()));
        if test_only(&parsed.attrs) {
            return;
        }
        let parent = file.parent().expect("source parent");
        let module_dir = if matches!(
            file.file_name().and_then(|name| name.to_str()),
            Some("lib.rs" | "main.rs" | "mod.rs")
        ) {
            parent.to_owned()
        } else {
            parent.join(file.file_stem().expect("module filename"))
        };
        self.items(&parsed.items, file, &module_dir, scope, inherited);
    }

    fn items(
        &mut self,
        items: &[syn::Item],
        file: &Path,
        directory: &Path,
        scope: &[String],
        inherited: &Aliases,
    ) {
        let aliases = imports(items, inherited, scope);
        for item in items {
            match item {
                syn::Item::Fn(item) if !test_only(&item.attrs) => {
                    self.function(file, scope, (None, false), &item.sig, &item.block, &aliases);
                }
                syn::Item::Impl(item) if !test_only(&item.attrs) => {
                    let owner = match item.self_ty.as_ref() {
                        syn::Type::Path(ty) => {
                            Some(owner_path(rules::path(&ty.path, &aliases, None), scope))
                        }
                        _ => None,
                    };
                    let root = item.trait_.as_ref().is_some_and(|(path, _)| {
                        rules::path(path, &aliases, None)
                            .last()
                            .is_some_and(|name| name == "EncoderBackend")
                    });
                    if owner
                        .as_ref()
                        .and_then(|owner| owner.last())
                        .is_some_and(|name| name == "EncodeError")
                    {
                        if let Some((path, _)) = &item.trait_ {
                            if path
                                .segments
                                .last()
                                .is_some_and(|part| part.ident == "From")
                            {
                                if let syn::PathArguments::AngleBracketed(arguments) =
                                    &path.segments.last().expect("From segment").arguments
                                {
                                    if arguments.args.iter().any(|argument| matches!(argument, syn::GenericArgument::Type(syn::Type::Path(ty)) if rules::path(&ty.path, &aliases, None).last().is_some_and(|name| name == "CodecError"))) {
                                        self.graph.global.push(format!("{}:{}: forbidden From<CodecError> for EncodeError", self.relative(file), item.span().start().line));
                                    }
                                }
                            }
                        }
                    }
                    for method in &item.items {
                        if let syn::ImplItem::Fn(method) = method {
                            if !test_only(&method.attrs) {
                                self.function(
                                    file,
                                    scope,
                                    (owner.clone(), root),
                                    &method.sig,
                                    &method.block,
                                    &aliases,
                                );
                            }
                        }
                    }
                }
                syn::Item::Trait(item) if !test_only(&item.attrs) => {
                    for method in &item.items {
                        if let syn::TraitItem::Fn(method) = method {
                            if !test_only(&method.attrs) {
                                if let Some(block) = &method.default {
                                    self.function(
                                        file,
                                        scope,
                                        (
                                            Some(owner_path(vec![item.ident.to_string()], scope)),
                                            false,
                                        ),
                                        &method.sig,
                                        block,
                                        &aliases,
                                    );
                                }
                            }
                        }
                    }
                }
                syn::Item::Mod(item) if !test_only(&item.attrs) => {
                    let mut nested = scope.to_vec();
                    nested.push(item.ident.to_string());
                    if let Some((_, items)) = &item.content {
                        self.items(
                            items,
                            file,
                            &directory.join(item.ident.to_string()),
                            &nested,
                            &aliases,
                        );
                    } else {
                        let explicit = item.attrs.iter().find_map(|attribute| {
                            if !attribute.path().is_ident("path") {
                                return None;
                            }
                            if let syn::Meta::NameValue(value) = &attribute.meta {
                                if let syn::Expr::Lit(value) = &value.value {
                                    if let syn::Lit::Str(path) = &value.lit {
                                        return Some(path.value());
                                    }
                                }
                            }
                            None
                        });
                        let source = explicit.map_or_else(
                            || {
                                let flat = directory.join(format!("{}.rs", item.ident));
                                if flat.is_file() {
                                    flat
                                } else {
                                    directory.join(item.ident.to_string()).join("mod.rs")
                                }
                            },
                            |path| directory.join(path),
                        );
                        self.file(&source, &nested, &aliases);
                    }
                }
                syn::Item::Const(item)
                    if item.ident == "CAPACITY_FIELDS" && !test_only(&item.attrs) =>
                {
                    let mut rows = CapacityRows(Vec::new());
                    rows.visit_expr(&item.expr);
                    for helper in rows.0 {
                        self.graph.capacities.push((scope[0].clone(), helper));
                    }
                }
                syn::Item::Macro(item) if !test_only(&item.attrs) => {
                    if let Some(name) = &item.ident {
                        let mut scan = Body::new(aliases.clone(), None);
                        scan.tokens(item.mac.tokens.clone());
                        self.graph.functions.push(Function {
                            file: self.relative(file),
                            scope: scope.to_vec(),
                            owner: None,
                            name: name.to_string(),
                            line: item.span().start().line,
                            root: false,
                            is_macro: true,
                            calls: scan.calls,
                            sites: scan.sites,
                        });
                    }
                }
                _ => {}
            }
        }
    }

    fn relative(&self, file: &Path) -> String {
        file.strip_prefix(self.root)
            .expect("workspace source")
            .to_string_lossy()
            .replace('\\', "/")
    }

    fn function(
        &mut self,
        file: &Path,
        scope: &[String],
        (owner, root): (Option<Vec<String>>, bool),
        signature: &syn::Signature,
        block: &syn::Block,
        aliases: &Aliases,
    ) {
        let name = &signature.ident;
        let span = signature.span();
        let mut nested = Nested(Vec::new());
        nested.visit_block(block);
        let mut nested_scope = scope.to_vec();
        let owner_name = owner
            .as_ref()
            .map_or(String::new(), |owner| owner.join("::") + "::");
        nested_scope.push(format!("${owner_name}{name}@{}", span.start().line));
        let mut local = LocalImports(aliases.clone());
        local.visit_block(block);
        for item in &nested.0 {
            if let syn::Item::Fn(item) = item {
                if !test_only(&item.attrs) {
                    let mut path = nested_scope.clone();
                    path.push(item.sig.ident.to_string());
                    local.0.insert(item.sig.ident.to_string(), path);
                }
            }
        }
        for value in local.0.values_mut() {
            *value = absolute(value, scope);
        }
        let mut scan = Body::new(local.0, owner.as_deref());
        scan.bindings.push();
        for argument in &signature.inputs {
            if let syn::FnArg::Typed(argument) = argument {
                scan.bindings.bind(&argument.pat, Value::Unknown);
            }
        }
        scan.visit_block(block);
        self.graph.functions.push(Function {
            file: self.relative(file),
            scope: scope.to_vec(),
            owner: owner.clone(),
            name: name.to_string(),
            line: span.start().line,
            root,
            is_macro: false,
            calls: scan.calls,
            sites: scan.sites,
        });
        for item in nested.0 {
            self.items(
                &[item],
                file,
                file.parent().expect("source directory"),
                &nested_scope,
                aliases,
            );
        }
    }
}

fn owner_path(path: Vec<String>, scope: &[String]) -> Vec<String> {
    if matches!(
        path.first().map(String::as_str),
        Some("crate" | "self" | "super")
    ) {
        return absolute(&path, scope);
    }
    if path.first().is_some_and(|name| {
        scope.first() == Some(name)
            || name.starts_with("cadmpeg_")
            || matches!(name.as_str(), "std" | "core" | "alloc")
    }) {
        return path;
    }
    let mut result = module_scope(scope);
    result.extend(path);
    result
}

struct Nested(Vec<syn::Item>);
impl<'ast> Visit<'ast> for Nested {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        self.0.push(item.clone());
    }
}
struct CapacityRows(Vec<String>);
impl<'ast> Visit<'ast> for CapacityRows {
    fn visit_expr_struct(&mut self, value: &'ast syn::ExprStruct) {
        if value
            .path
            .segments
            .last()
            .is_some_and(|name| name.ident == "CapacityField")
        {
            for field in &value.fields {
                if matches!(&field.member, syn::Member::Named(name) if name == "helper") {
                    if let syn::Expr::Lit(value) = &field.expr {
                        if let syn::Lit::Str(value) = &value.lit {
                            self.0.push(value.value());
                        }
                    }
                }
            }
        }
        visit::visit_expr_struct(self, value);
    }
}

pub(super) fn workspace(root: &Path) -> Graph {
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).expect("workspace manifest");
    let mut loader = Loader {
        root,
        seen: BTreeSet::new(),
        graph: Graph::default(),
    };
    let members = manifest
        .split_once("members = [")
        .expect("explicit workspace members")
        .1
        .split_once(']')
        .expect("workspace member array")
        .0;
    for line in members
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let member: String = serde_json::from_str(line.trim_end_matches(','))
            .expect("literal workspace member path");
        let package = member
            .rsplit('/')
            .next()
            .expect("package basename")
            .replace('-', "_");
        for name in ["lib.rs", "main.rs"] {
            let source = root.join(&member).join("src").join(name);
            if source.is_file() {
                loader.file(&source, std::slice::from_ref(&package), &Aliases::new());
            }
        }
    }
    loader.graph
}

#[cfg(test)]
pub(super) fn fixture(text: &str) -> Graph {
    let parsed = syn::parse_file(text).expect("synthetic Rust source");
    let root = Path::new(".");
    let mut loader = Loader {
        root,
        seen: BTreeSet::new(),
        graph: Graph::default(),
    };
    loader.items(
        &parsed.items,
        Path::new("./fixture.rs"),
        root,
        &["fixture".into()],
        &Aliases::new(),
    );
    loader.graph
}
