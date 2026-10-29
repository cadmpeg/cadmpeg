// SPDX-License-Identifier: Apache-2.0
//! Module-rooted workspace loading, with test-only items removed.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use syn::spanned::Spanned;
use syn::visit::{self, Visit};

use super::bindings::Value;
use super::resolve::Item;
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
            rules::import(&item.tree, &[], &mut self.0);
        }
    }
    fn visit_item_fn(&mut self, _item: &'ast syn::ItemFn) {}
}

struct Loader<'a> {
    root: &'a Path,
    seen: BTreeSet<(PathBuf, Vec<String>)>,
    graph: Graph,
    /// Type parameters of the `impl` or `trait` whose methods are loading.
    outer_generics: BTreeSet<String>,
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
        if module_scope(scope).len() == scope.len() {
            self.graph.modules.insert(scope.to_vec());
        }
        self.register(items, scope);
        for item in items {
            let syn::Item::Use(item) = item else { continue };
            if test_only(&item.attrs) {
                continue;
            }
            let mut found = Vec::new();
            globs(&item.tree, &[], &mut found);
            for glob in found {
                let target = absolute(&rules::resolve_names(glob, &aliases, None), scope);
                if target != scope {
                    self.graph
                        .globs
                        .entry(scope.to_vec())
                        .or_default()
                        .insert(target);
                }
            }
            let mut declared = Aliases::new();
            rules::import(&item.tree, &[], &mut declared);
            for (name, target) in declared {
                let mut binding = scope.to_vec();
                binding.push(name.clone());
                let mut referenced = aliases.clone();
                referenced.remove(&name);
                let target = absolute(&rules::resolve_names(target, &referenced, None), scope);
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
                        let target = absolute(&rules::path(&ty.path, &aliases, None), scope);
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
                            Some(owner_path(rules::path(&ty.path, &aliases, None), scope))
                        }
                        _ => None,
                    };
                    let root_trait = item
                        .trait_
                        .as_ref()
                        .map(|(path, _)| rules::path(path, &aliases, None));
                    let root = root_trait.as_ref().is_some_and(|path| {
                        path.last().is_some_and(|name| name == "EncoderBackend")
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
                    for member in &item.items {
                        match member {
                            syn::ImplItem::Const(constant) if !test_only(&constant.attrs) => {
                                if let Some(owner) = &owner {
                                    let mut path = owner.clone();
                                    path.push(constant.ident.to_string());
                                    self.graph.items.insert(path, Item::Value);
                                }
                            }
                            syn::ImplItem::Macro(invocation) if !test_only(&invocation.attrs) => {
                                if let Some(generator) = invocation.mac.path.segments.last() {
                                    self.generated(&generator.ident, invocation.mac.tokens.clone());
                                }
                            }
                            _ => {}
                        }
                    }
                    self.outer_generics = type_parameters(&item.generics);
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
                    self.outer_generics.clear();
                }
                syn::Item::Trait(item) if !test_only(&item.attrs) => {
                    self.outer_generics = type_parameters(&item.generics);
                    for member in &item.items {
                        if let syn::TraitItem::Const(constant) = member {
                            let mut path = owner_path(vec![item.ident.to_string()], scope);
                            path.push(constant.ident.to_string());
                            self.graph.items.insert(path, Item::Value);
                        }
                    }
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
                    self.outer_generics.clear();
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
                    let generator = item
                        .ident
                        .as_ref()
                        .or_else(|| item.mac.path.segments.last().map(|part| &part.ident));
                    if let Some(generator) = generator {
                        self.generated(generator, item.mac.tokens.clone());
                    }
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
                            root_trait: None,
                            is_macro: true,
                            generics: BTreeSet::new(),
                            calls: scan.calls,
                            sites: scan.sites,
                            path_checks: scan.path_checks,
                        });
                    }
                }
                _ => {}
            }
        }
    }

    /// Maps each identifier in a macro definition or item-level invocation to
    /// the macro, so a call to an item the macro generates reaches its body.
    fn generated(&mut self, generator: &syn::Ident, tokens: proc_macro2::TokenStream) {
        for token in tokens {
            match token {
                proc_macro2::TokenTree::Ident(name) => {
                    self.graph
                        .generated
                        .entry(name.to_string())
                        .or_default()
                        .insert(generator.to_string());
                }
                proc_macro2::TokenTree::Group(group) => self.generated(generator, group.stream()),
                _ => {}
            }
        }
    }

    fn register(&mut self, items: &[syn::Item], scope: &[String]) {
        let items_found = &mut self.graph.items;
        let mut add = |names: &[&syn::Ident], kind: Item| {
            let mut path = scope.to_vec();
            path.extend(names.iter().map(ToString::to_string));
            items_found.insert(path, kind);
        };
        for item in items {
            match item {
                syn::Item::Struct(item) if !test_only(&item.attrs) => {
                    add(&[&item.ident], Item::Type);
                }
                syn::Item::Union(item) if !test_only(&item.attrs) => {
                    add(&[&item.ident], Item::Type);
                }
                syn::Item::Type(item) if !test_only(&item.attrs) => add(&[&item.ident], Item::Type),
                syn::Item::Trait(item) if !test_only(&item.attrs) => {
                    self.outer_generics = type_parameters(&item.generics);
                    add(&[&item.ident], Item::Trait);
                }
                syn::Item::Const(item) if !test_only(&item.attrs) => {
                    add(&[&item.ident], Item::Value);
                }
                syn::Item::Static(item) if !test_only(&item.attrs) => {
                    add(&[&item.ident], Item::Value);
                }
                syn::Item::Enum(item) if !test_only(&item.attrs) => {
                    add(&[&item.ident], Item::Type);
                    for variant in &item.variants {
                        add(&[&item.ident, &variant.ident], Item::Value);
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
        let mut generics = type_parameters(&signature.generics);
        generics.extend(self.outer_generics.iter().cloned());
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
            root_trait,
            is_macro: false,
            generics,
            calls: scan.calls,
            sites: scan.sites,
            path_checks: scan.path_checks,
        });
        let outer = std::mem::take(&mut self.outer_generics);
        for item in nested.0 {
            self.items(
                &[item],
                file,
                file.parent().expect("source directory"),
                &nested_scope,
                aliases,
            );
        }
        self.outer_generics = outer;
    }
}

fn type_parameters(generics: &syn::Generics) -> BTreeSet<String> {
    generics
        .type_params()
        .map(|parameter| parameter.ident.to_string())
        .collect()
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
        outer_generics: BTreeSet::new(),
    };
    loader.graph.externals = external_crates(&manifest);
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

/// The `[workspace.dependencies]` crates that are not workspace members.
fn external_crates(manifest: &str) -> BTreeSet<String> {
    let Some((_, section)) = manifest.split_once("[workspace.dependencies]") else {
        return BTreeSet::new();
    };
    section
        .lines()
        .map(str::trim)
        .take_while(|line| !line.starts_with('['))
        .filter(|line| !line.starts_with('#') && !line.contains("path ="))
        .filter_map(|line| line.split_once('='))
        .map(|(name, _)| name.trim().replace('-', "_"))
        .filter(|name| !name.is_empty())
        .collect()
}

#[cfg(test)]
pub(super) fn fixture(text: &str) -> Graph {
    let parsed = syn::parse_file(text).expect("synthetic Rust source");
    let root = Path::new(".");
    let mut loader = Loader {
        root,
        seen: BTreeSet::new(),
        graph: Graph::default(),
        outer_generics: BTreeSet::new(),
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
