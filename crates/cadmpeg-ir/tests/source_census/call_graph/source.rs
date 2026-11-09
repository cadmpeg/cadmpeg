// SPDX-License-Identifier: Apache-2.0
//! Module-rooted workspace loading, with test-only items removed.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use syn::spanned::Spanned;
use syn::visit::Visit;

use super::bindings::Value;
use super::body::{self, Aliases, Body};
use super::{absolute, module_scope, Function, Graph, SelfReturn};

fn returns_self(signature: &syn::Signature) -> Option<SelfReturn> {
    let syn::ReturnType::Type(_, ty) = &signature.output else {
        return None;
    };
    let syn::Type::Path(ty) = ty.as_ref() else {
        return None;
    };
    let segment = ty.path.segments.last()?;
    if segment.ident == "Self" {
        return Some(SelfReturn::Direct);
    }
    if segment.ident != "Result" && segment.ident != "Option" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return None;
    };
    matches!(arguments.args.first(), Some(syn::GenericArgument::Type(syn::Type::Path(inner))) if inner.qself.is_none() && inner.path.is_ident("Self")).then_some(SelfReturn::Wrapped)
}

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
                body::import(&item.tree, &[], &mut aliases);
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
                        body::path(&ty.path, &previous, None),
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

fn globs(tree: &syn::UseTree, prefix: &[String], found: &mut Vec<Vec<String>>) {
    match tree {
        syn::UseTree::Path(part) => {
            let mut path = prefix.to_vec();
            path.push(part.ident.to_string());
            globs(&part.tree, &path, found);
        }
        syn::UseTree::Group(group) => {
            for item in &group.items {
                globs(item, prefix, found);
            }
        }
        syn::UseTree::Glob(_) => found.push(prefix.to_vec()),
        syn::UseTree::Name(_) | syn::UseTree::Rename(_) => {}
    }
}

struct LocalImports(Aliases);
impl<'ast> Visit<'ast> for LocalImports {
    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        if !test_only(&item.attrs) {
            body::import(&item.tree, &[], &mut self.0);
        }
    }
    fn visit_item_fn(&mut self, _item: &'ast syn::ItemFn) {}
}

struct Loader {
    seen: BTreeSet<(PathBuf, Vec<String>)>,
    graph: Graph,
}
impl Loader {
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
            let syn::Item::Use(item) = item else { continue };
            if test_only(&item.attrs) {
                continue;
            }
            let mut found = Vec::new();
            globs(&item.tree, &[], &mut found);
            for glob in found {
                let target = absolute(&body::resolve_names(glob, &aliases, None), scope);
                if target != scope {
                    self.graph
                        .globs
                        .entry(scope.to_vec())
                        .or_default()
                        .insert(target);
                }
            }
            let mut declared = Aliases::new();
            body::import(&item.tree, &[], &mut declared);
            for (name, target) in declared {
                let mut binding = scope.to_vec();
                binding.push(name.clone());
                let mut referenced = aliases.clone();
                referenced.remove(&name);
                let target = absolute(&body::resolve_names(target, &referenced, None), scope);
                if target != binding {
                    self.graph
                        .aliases
                        .entry(binding)
                        .or_default()
                        .insert(target);
                }
            }
        }
        for item in items {
            if let syn::Item::Type(item) = item {
                if !test_only(&item.attrs) {
                    if let syn::Type::Path(ty) = item.ty.as_ref() {
                        let mut binding = scope.to_vec();
                        binding.push(item.ident.to_string());
                        let target = absolute(&body::path(&ty.path, &aliases, None), scope);
                        if target != binding {
                            self.graph
                                .aliases
                                .entry(binding)
                                .or_default()
                                .insert(target);
                        }
                    }
                }
            }
        }
        for item in items {
            match item {
                syn::Item::Fn(item) if !test_only(&item.attrs) => {
                    self.function(
                        file,
                        scope,
                        (None, false, None),
                        &item.sig,
                        &item.block,
                        &aliases,
                    );
                }
                syn::Item::Impl(item) if !test_only(&item.attrs) => {
                    let owner = match item.self_ty.as_ref() {
                        syn::Type::Path(ty) => {
                            Some(owner_path(body::path(&ty.path, &aliases, None), scope))
                        }
                        _ => None,
                    };
                    let root_trait = item
                        .trait_
                        .as_ref()
                        .map(|(path, _)| body::path(path, &aliases, None));
                    let root = root_trait.as_ref().is_some_and(|path| {
                        path.last().is_some_and(|name| name == "EncoderBackend")
                    });
                    for method in &item.items {
                        if let syn::ImplItem::Fn(method) = method {
                            if !test_only(&method.attrs) {
                                self.function(
                                    file,
                                    scope,
                                    (owner.clone(), root, root_trait.clone()),
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
                                            None,
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
                syn::Item::Macro(item) if !test_only(&item.attrs) => {
                    if let Some(name) = &item.ident {
                        let mut scan = Body::new(aliases.clone(), None);
                        scan.tokens(item.mac.tokens.clone());
                        self.graph.functions.push(Function {
                            scope: scope.to_vec(),
                            owner: None,
                            name: name.to_string(),
                            line: item.span().start().line,
                            root: false,
                            root_trait: None,
                            is_macro: true,
                            returns_self: None,
                            calls: scan.calls,
                            ctor: scan.ctor,
                        });
                    }
                }
                _ => {}
            }
        }
    }

    fn function(
        &mut self,
        file: &Path,
        scope: &[String],
        (owner, root, root_trait): (Option<Vec<String>>, bool, Option<Vec<String>>),
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
            scope: scope.to_vec(),
            owner: owner.clone(),
            name: name.to_string(),
            line: span.start().line,
            root,
            root_trait,
            is_macro: false,
            returns_self: returns_self(signature),
            calls: scan.calls,
            ctor: scan.ctor,
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
pub(crate) fn workspace(root: &Path) -> Graph {
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).expect("workspace manifest");
    let mut loader = Loader {
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
pub(crate) fn fixture(text: &str) -> Graph {
    let parsed = syn::parse_file(text).expect("synthetic Rust source");
    let mut loader = Loader {
        seen: BTreeSet::new(),
        graph: Graph::default(),
    };
    loader.items(
        &parsed.items,
        Path::new("./fixture.rs"),
        Path::new("."),
        &["fixture".into()],
        &Aliases::new(),
    );
    loader.graph
}
