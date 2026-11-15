// SPDX-License-Identifier: Apache-2.0
//! Wire-crate declarations addressed by module path, and the associated types
//! their trait impls state.
//!
//! A name is no address: two modules may each declare a `Sketch`, and each is
//! its own declaration with its own keys. A [`Declaration`] is the file, the
//! module path and the name, and a path written in source is resolved from
//! the module that writes it.
//!
//! A field typed `P::Name` for a type parameter `P` holds whatever the impl
//! filling `P` states for `Name`. Every impl of the bound trait in the wire
//! crates is read; a bound, trait or impl the census cannot read is an error,
//! never a field taken to hold no `Option`.

use std::cell::{OnceCell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use syn::spanned::Spanned;

use crate::reader_routes::DeclarationResolver;
use crate::{collect_rust_sources, flatten_tokens, is_option_type, is_test_module, is_test_path};

/// The source roots of the three wire crates.
const WIRE_ROOTS: [&str; 3] = [
    "crates/cadmpeg-ir/src",
    "crates/cadmpeg-core/src",
    "crates/cadmpeg-asm/src",
];

/// Standard and serde traits declare no associated type a field can name. A
/// bound that resolves to no wire-crate trait and is not one of these may
/// declare one the census cannot read.
const TRAITS_WITHOUT_ASSOCIATED_TYPES: &[&str] = &[
    "Clone",
    "Copy",
    "Debug",
    "Display",
    "Default",
    "PartialEq",
    "Eq",
    "PartialOrd",
    "Ord",
    "Hash",
    "Send",
    "Sync",
    "Sized",
    "Unpin",
    "Serialize",
    "Deserialize",
    "DeserializeOwned",
    "JsonSchema",
];

/// One struct, enum, alias or trait of the wire crates.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Declaration {
    /// The repository-relative file that declares it.
    pub(crate) path: String,
    /// Its module path from the crate root, inline modules included.
    pub(crate) scope: Vec<String>,
    pub(crate) name: String,
}

impl fmt::Display for Declaration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let krate = self.path.split('/').nth(1).unwrap_or("crate");
        write!(formatter, "{}", krate.replace('-', "_"))?;
        for segment in &self.scope {
            write!(formatter, "::{segment}")?;
        }
        write!(formatter, "::{}", self.name)
    }
}

/// The module path of a repository-relative source file from its crate root.
pub(crate) fn module_scope(relative: &str) -> Vec<String> {
    let Some((_, inner)) = relative.split_once("/src/") else {
        return Vec::new();
    };
    let mut parts: Vec<String> = inner.split('/').map(str::to_owned).collect();
    let file = parts.pop().unwrap_or_default();
    let stem = file.strip_suffix(".rs").unwrap_or(&file);
    if !matches!(stem, "lib" | "main" | "mod") {
        parts.push(stem.to_owned());
    }
    parts
}

/// What a trait states for one of its associated types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AssociatedDeclaration {
    /// `type Name;` or `type Name: Bounds;`: every impl states the type.
    Required,
    /// `type Name = Default;`: an impl may state nothing.
    Defaulted,
    /// `type Name<..>;`: the type takes arguments the census does not read.
    Generic,
}

/// One `impl Trait for Type` block of the wire crates.
struct TraitImpl {
    path: String,
    scope: Vec<String>,
    line: usize,
    trait_path: syn::Path,
    self_type: String,
    /// The impl's own type parameters.
    parameters: Vec<String>,
    /// Associated type name -> each type the impl states, or why the census
    /// cannot read it.
    types: BTreeMap<String, Vec<Result<syn::Type, String>>>,
}

/// A type alias of the wire crates: its parameters and the type it names.
struct Alias {
    generics: syn::Generics,
    ty: syn::Type,
}

impl TraitImpl {
    fn describe(&self) -> String {
        format!(
            "impl {} for {} at {}:{}",
            path_text(&self.trait_path),
            self.self_type,
            self.path,
            self.line
        )
    }
}

/// The declarations of the wire crates, their traits, and every trait impl.
///
/// Each index is read from the source on first use.
pub(crate) struct Declarations {
    root: PathBuf,
    resolver: OnceCell<DeclarationResolver>,
    traits: OnceCell<TraitIndex>,
    /// Per trait name, an impl of a trait of that name outside the wire
    /// crates.
    foreign: RefCell<BTreeMap<String, Option<String>>>,
}

/// The traits and trait impls of the wire crates.
#[derive(Default)]
struct TraitIndex {
    traits: BTreeMap<Declaration, BTreeMap<String, AssociatedDeclaration>>,
    impls: Vec<TraitImpl>,
    aliases: BTreeMap<Declaration, Alias>,
    /// Each `macro_rules!` body or item macro call that spells `impl`, as its
    /// location and tokens: an impl it expands to is not parsed here.
    macros: Vec<(String, usize, Vec<String>)>,
    /// The names of the `macro_rules!` definitions whose body spells `impl`.
    implementing: BTreeSet<String>,
    /// Each item macro call, as its location, the macro's name and its
    /// tokens: a call to an implementing macro can name the trait it
    /// implements.
    calls: Vec<(String, usize, String, Vec<String>)>,
}

impl Declarations {
    pub(crate) fn read(root: &Path) -> Self {
        Self {
            root: root.to_owned(),
            resolver: OnceCell::new(),
            traits: OnceCell::new(),
            foreign: RefCell::new(BTreeMap::new()),
        }
    }

    fn resolver(&self) -> &DeclarationResolver {
        self.resolver
            .get_or_init(|| DeclarationResolver::read(&self.root))
    }

    fn trait_index(&self) -> &TraitIndex {
        self.traits.get_or_init(|| TraitIndex::read(&self.root))
    }
}

impl TraitIndex {
    fn read(root: &Path) -> Self {
        let mut index = Self::default();
        for source in WIRE_ROOTS {
            let mut files = Vec::new();
            collect_rust_sources(&root.join(source), &mut files)
                .unwrap_or_else(|error| panic!("cannot collect {source}: {error}"));
            files.sort();
            for file in files {
                let relative = relative_path(root, &file);
                if is_test_path(&relative) {
                    continue;
                }
                let text = std::fs::read_to_string(&file).expect("read source");
                let parsed = syn::parse_file(&text)
                    .map_err(|error| format!("{relative} does not parse: {error}"))
                    .expect("every source file in the wire crates parses");
                index.collect(&parsed.items, &relative, &module_scope(&relative));
            }
        }
        index
    }

    fn collect(&mut self, items: &[syn::Item], relative: &str, scope: &[String]) {
        for item in items {
            match item {
                syn::Item::Trait(declaration) => {
                    let mut types = BTreeMap::new();
                    for member in &declaration.items {
                        let syn::TraitItem::Type(associated) = member else {
                            continue;
                        };
                        let stated = if !associated.generics.params.is_empty() {
                            AssociatedDeclaration::Generic
                        } else if associated.default.is_some() {
                            AssociatedDeclaration::Defaulted
                        } else {
                            AssociatedDeclaration::Required
                        };
                        types.insert(associated.ident.to_string(), stated);
                    }
                    self.traits.insert(
                        Declaration {
                            path: relative.to_owned(),
                            scope: scope.to_owned(),
                            name: declaration.ident.to_string(),
                        },
                        types,
                    );
                }
                syn::Item::Impl(implementation) => {
                    self.collect_impl(implementation, relative, scope);
                }
                syn::Item::Type(alias) => {
                    self.aliases.insert(
                        Declaration {
                            path: relative.to_owned(),
                            scope: scope.to_owned(),
                            name: alias.ident.to_string(),
                        },
                        Alias {
                            generics: alias.generics.clone(),
                            ty: (*alias.ty).clone(),
                        },
                    );
                }
                syn::Item::Fn(function) => {
                    let (nested, calls) = function_body_items(function);
                    self.collect(&nested, relative, scope);
                    for invocation in calls {
                        self.record_call(&invocation.mac, relative);
                    }
                }
                syn::Item::Mod(module) => {
                    if is_test_module(module) {
                        continue;
                    }
                    if let Some((_, nested)) = &module.content {
                        let mut nested_scope = scope.to_owned();
                        nested_scope.push(module.ident.to_string());
                        self.collect(nested, relative, &nested_scope);
                    }
                }
                syn::Item::Macro(invocation) => match &invocation.ident {
                    Some(defined) => self.record_definition(&invocation.mac, defined, relative),
                    None => self.record_call(&invocation.mac, relative),
                },
                _ => {}
            }
        }
    }

    /// Records a `macro_rules!` definition named `defined`.
    fn record_definition(&mut self, mac: &syn::Macro, defined: &syn::Ident, relative: &str) {
        let mut tokens = Vec::new();
        flatten_tokens(&mac.tokens, &mut tokens);
        if tokens.iter().any(|token| token == "impl") {
            self.implementing.insert(defined.to_string());
            self.macros
                .push((relative.to_owned(), mac.path.span().start().line, tokens));
        }
    }

    /// Records a macro call, or a `macro_rules!` definition parsed as a
    /// statement, whose first token is the macro's name.
    fn record_call(&mut self, mac: &syn::Macro, relative: &str) {
        let mut tokens = Vec::new();
        flatten_tokens(&mac.tokens, &mut tokens);
        let line = mac.path.span().start().line;
        let spells_impl = tokens.iter().any(|token| token == "impl");
        let Some(called) = mac.path.segments.last() else {
            return;
        };
        if called.ident == "macro_rules" {
            if let (true, Some(defined)) = (spells_impl, tokens.first()) {
                self.implementing.insert(defined.clone());
            }
        } else {
            self.calls.push((
                relative.to_owned(),
                line,
                called.ident.to_string(),
                tokens.clone(),
            ));
        }
        if spells_impl {
            self.macros.push((relative.to_owned(), line, tokens));
        }
    }

    fn collect_impl(&mut self, implementation: &syn::ItemImpl, relative: &str, scope: &[String]) {
        let Some((trait_path, _)) = &implementation.trait_ else {
            return;
        };
        if implementation.modifiers.polarity.is_some() {
            // A negative impl states no associated type.
            return;
        }
        let parameters: Vec<String> = implementation
            .generics
            .params
            .iter()
            .filter_map(|parameter| match parameter {
                syn::GenericParam::Type(declared) => Some(declared.ident.to_string()),
                _ => None,
            })
            .collect();
        let mut types: BTreeMap<String, Vec<Result<syn::Type, String>>> = BTreeMap::new();
        for member in &implementation.items {
            let syn::ImplItem::Type(associated) = member else {
                continue;
            };
            let stated = if associated.generics.params.is_empty() {
                Ok(associated.ty.clone())
            } else {
                Err(format!(
                    "type {} takes arguments the census does not read",
                    associated.ident
                ))
            };
            types
                .entry(associated.ident.to_string())
                .or_default()
                .push(stated);
        }
        self.impls.push(TraitImpl {
            path: relative.to_owned(),
            scope: scope.to_owned(),
            line: implementation.self_ty.span().start().line,
            trait_path: trait_path.clone(),
            self_type: type_name(&implementation.self_ty),
            parameters,
            types,
        });
    }
}

/// The items a function body declares, and the macros it calls in
/// statement position, where a call can expand to an impl.
fn function_body_items(function: &syn::ItemFn) -> (Vec<syn::Item>, Vec<syn::StmtMacro>) {
    let mut items = Vec::new();
    let mut calls = Vec::new();
    for statement in &function.block.stmts {
        match statement {
            syn::Stmt::Item(item) => items.push(item.clone()),
            syn::Stmt::Macro(call) => calls.push(call.clone()),
            _ => {}
        }
    }
    (items, calls)
}

impl Declarations {
    /// The declaration `path` names from module `scope` of `file`.
    pub(crate) fn resolve(
        &self,
        file: &str,
        scope: &[String],
        path: &syn::Path,
    ) -> Option<Declaration> {
        self.resolver()
            .resolve(file, scope, path)
            .map(|(path, scope, name)| Declaration { path, scope, name })
    }

    /// The declaration a serde attribute's type string names from `from`'s
    /// own module.
    pub(crate) fn resolve_named(
        &self,
        from: &Declaration,
        spelled: &str,
    ) -> Result<Declaration, String> {
        syn::parse_str::<syn::TypePath>(spelled)
            .ok()
            .filter(|typed| typed.qself.is_none())
            .and_then(|typed| self.resolve(&from.path, &from.scope, &typed.path))
            .ok_or_else(|| format!("{spelled}, named from {from}, resolves to no one declaration"))
    }

    /// Whether a field of type `ty`, declared with `generics` in module
    /// `scope` of `file`, holds an `Option`.
    ///
    /// A field spelled `Option<..>` does. A field spelled `P::Name` or
    /// `<P as Trait>::Name` for a type parameter `P` of `generics` holds the
    /// type each impl of the one bound trait that declares `Name` states, so
    /// it holds an `Option` when any impl's does. A field spelled by a type
    /// alias holds what the alias names, read where the alias is declared;
    /// an alias that names one of its own parameters holds the argument the
    /// field states for it.
    pub(crate) fn is_optional(
        &self,
        file: &str,
        scope: &[String],
        generics: &syn::Generics,
        ty: &syn::Type,
    ) -> Result<bool, String> {
        let site = Site {
            file,
            scope,
            generics,
            impl_parameters: None,
        };
        self.holds_option(&site, ty, 0)
    }

    fn holds_option(&self, site: &Site<'_>, ty: &syn::Type, depth: usize) -> Result<bool, String> {
        if depth > MAX_TYPE_DEPTH {
            return Err(format!(
                "the aliases and impls reached from {} name each other past {MAX_TYPE_DEPTH} steps",
                site.file
            ));
        }
        if let syn::Type::Reference(borrowed) = ty {
            return self.holds_option(site, &borrowed.elem, depth);
        }
        if is_option_type(ty) {
            return Ok(true);
        }
        if let (Some(parameters), syn::Type::Path(typed)) = (site.impl_parameters, ty) {
            let first = typed
                .path
                .segments
                .first()
                .map(|segment| segment.ident.to_string())
                .unwrap_or_default();
            if typed.qself.is_some()
                || (first == "Self" && typed.path.segments.len() > 1)
                || parameters.contains(&first)
            {
                return Err(format!(
                    "the type {} depends on what fills the impl, which the census does not read",
                    path_text(&typed.path)
                ));
            }
        }
        if let Some((parameter, named_trait, name)) = associated_type(ty, site.generics) {
            return self.associated_holds_option(site, &parameter, named_trait, &name, depth);
        }
        self.alias_holds_option(site, ty, depth)
    }

    /// Whether the alias `ty` names, if it names one, holds an `Option`.
    fn alias_holds_option(
        &self,
        site: &Site<'_>,
        ty: &syn::Type,
        depth: usize,
    ) -> Result<bool, String> {
        let syn::Type::Path(typed) = ty else {
            return Ok(false);
        };
        let Some(last) = typed.path.segments.last() else {
            return Ok(false);
        };
        let index = self.trait_index();
        if typed.qself.is_some()
            || !index
                .aliases
                .keys()
                .any(|declared| last.ident == declared.name)
            || (typed.path.segments.len() == 1
                && site
                    .generics
                    .type_params()
                    .any(|declared| declared.ident == last.ident))
        {
            return Ok(false);
        }
        let Some(declaration) = self.resolve(site.file, site.scope, &typed.path) else {
            return Ok(false);
        };
        let Some(alias) = index.aliases.get(&declaration) else {
            return Ok(false);
        };
        let alias_site = Site {
            file: &declaration.path,
            scope: &declaration.scope,
            generics: &alias.generics,
            impl_parameters: None,
        };
        let parameters: Vec<&syn::TypeParam> = alias.generics.type_params().collect();
        let names: Vec<&syn::Ident> = parameters.iter().map(|declared| &declared.ident).collect();
        let mut body = &alias.ty;
        while let syn::Type::Reference(borrowed) = body {
            body = &borrowed.elem;
        }
        let named = match body {
            syn::Type::Path(named) if named.qself.is_none() => named.path.get_ident(),
            _ => None,
        };
        if let Some(position) = named.and_then(|ident| names.iter().position(|name| *name == ident))
        {
            let arguments: Vec<&syn::Type> = match &last.arguments {
                syn::PathArguments::AngleBracketed(arguments) => arguments
                    .args
                    .iter()
                    .filter_map(|argument| match argument {
                        syn::GenericArgument::Type(argument) => Some(argument),
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            };
            return match (arguments.get(position), &parameters[position].default) {
                (Some(argument), _) => self.holds_option(site, argument, depth + 1),
                (None, Some((_, default))) => self.holds_option(&alias_site, default, depth + 1),
                (None, None) => Err(format!(
                    "{declaration} names its parameter {}, for which the use states no argument",
                    names[position]
                )),
            };
        }
        if mentions_any(body, &names)
            && type_head(body)
                .is_some_and(|head| index.aliases.keys().any(|declared| head == declared.name))
        {
            return Err(format!(
                "{declaration} passes its parameters on to another alias, which the census does not read"
            ));
        }
        self.holds_option(&alias_site, &alias.ty, depth + 1)
    }

    /// Whether `parameter::name` holds an `Option`: whether any impl of the
    /// one bound trait that declares `name` states one there.
    fn associated_holds_option(
        &self,
        site: &Site<'_>,
        parameter: &str,
        named_trait: Option<syn::Path>,
        name: &str,
        depth: usize,
    ) -> Result<bool, String> {
        let (file, scope, generics) = (site.file, site.scope, site.generics);
        let index = self.trait_index();
        let label = format!("{parameter}::{name}");
        let bounds = match named_trait {
            Some(path) => vec![path],
            None => parameter_bounds(generics, parameter),
        };
        if bounds.is_empty() {
            return Err(format!(
                "{label} names a type parameter with no trait bound that declares {name}"
            ));
        }
        let mut declaring = Vec::new();
        for bound in &bounds {
            let Some(declaration) = self.resolve(file, scope, bound) else {
                let base = bound
                    .segments
                    .last()
                    .map(|segment| segment.ident.to_string())
                    .unwrap_or_default();
                if TRAITS_WITHOUT_ASSOCIATED_TYPES.contains(&base.as_str())
                    && !index.traits.keys().any(|declared| declared.name == base)
                {
                    continue;
                }
                return Err(format!(
                    "the bound {} of {parameter} is no wire-crate trait the census can read, so {label} is unread",
                    path_text(bound)
                ));
            };
            let Some(types) = index.traits.get(&declaration) else {
                return Err(format!(
                    "the bound {} of {parameter} resolves to {declaration}, which is no trait",
                    path_text(bound)
                ));
            };
            match types.get(name) {
                Some(AssociatedDeclaration::Required) => declaring.push(declaration),
                Some(AssociatedDeclaration::Defaulted) => {
                    return Err(format!("{declaration}::{name} states a default"));
                }
                Some(AssociatedDeclaration::Generic) => {
                    return Err(format!(
                        "{declaration}::{name} is a generic associated type"
                    ));
                }
                None => {}
            }
        }
        let [trait_declaration] = declaring.as_slice() else {
            return Err(format!(
                "{} bounds of {parameter} declare {name}, so {label} names no one trait",
                declaring.len()
            ));
        };
        if let Some(location) = self.foreign_impl(&trait_declaration.name) {
            return Err(format!(
                "{location} is outside the wire crates, so its {name} is unread"
            ));
        }
        if let Some((path, line, _)) = index
            .macros
            .iter()
            .find(|(_, _, tokens)| tokens.contains(&trait_declaration.name))
        {
            return Err(format!(
                "the macro at {path}:{line} spells impl and {}, so an impl it expands is unread",
                trait_declaration.name
            ));
        }
        if let Some((path, line, called, _)) = index.calls.iter().find(|(_, _, called, tokens)| {
            index.implementing.contains(called) && tokens.contains(&trait_declaration.name)
        }) {
            return Err(format!(
                "the call to {called}! at {path}:{line} names {} and the macro spells impl, so an impl it expands is unread",
                trait_declaration.name
            ));
        }
        let mut optional = false;
        let mut found = false;
        // A type an impl states names no parameter a declaration instantiates.
        let no_generics = syn::Generics::default();
        for implementation in &index.impls {
            if implementation
                .trait_path
                .segments
                .last()
                .is_none_or(|segment| segment.ident != trait_declaration.name)
            {
                continue;
            }
            let resolved = self.resolve(
                &implementation.path,
                &implementation.scope,
                &implementation.trait_path,
            );
            let Some(resolved) = resolved else {
                return Err(format!(
                    "the trait of {} resolves to no one declaration",
                    implementation.describe()
                ));
            };
            if resolved != *trait_declaration {
                continue;
            }
            found = true;
            let Some(stated) = implementation.types.get(name) else {
                return Err(format!(
                    "{} states no type {name}",
                    implementation.describe()
                ));
            };
            let impl_site = Site {
                file: &implementation.path,
                scope: &implementation.scope,
                generics: &no_generics,
                impl_parameters: Some(&implementation.parameters),
            };
            for value in stated {
                match value
                    .as_ref()
                    .map_err(String::clone)
                    .and_then(|value| self.holds_option(&impl_site, value, depth + 1))
                {
                    Ok(holds_option) => optional |= holds_option,
                    Err(reason) => {
                        return Err(format!("{}: {reason}", implementation.describe()));
                    }
                }
            }
        }
        if !found {
            return Err(format!(
                "{trait_declaration} has no impl in the wire crates, so {label} is unread"
            ));
        }
        Ok(optional)
    }

    /// An impl of a trait named `name` anywhere in the workspace outside the
    /// wire crates' sources, or a call there naming `name` to a macro that
    /// spells `impl`.
    ///
    /// Only an impl in a wire crate's `src` is read; one in another crate,
    /// in `examples/`, in `benches/` or in a build script states a type the
    /// census does not read.
    fn foreign_impl(&self, name: &str) -> Option<String> {
        if let Some(found) = self.foreign.borrow().get(name) {
            return found.clone();
        }
        let mut implementing = self.trait_index().implementing.clone();
        let mut calls = Vec::new();
        let mut found = None;
        for file in workspace_sources(&self.root) {
            let relative = relative_path(&self.root, &file);
            // The leading separator makes a test tree at the workspace root
            // one too.
            if is_test_path(&format!("/{relative}"))
                || WIRE_ROOTS
                    .iter()
                    .any(|root| relative.starts_with(&format!("{root}/")))
            {
                continue;
            }
            let text = std::fs::read_to_string(&file).expect("read source");
            let names = text.contains(name);
            if !names && !text.contains("macro_rules") {
                continue;
            }
            let Ok(parsed) = syn::parse_file(&text) else {
                if names {
                    found = Some(format!(
                        "{relative}, which names {name} and does not parse,"
                    ));
                    break;
                }
                continue;
            };
            let mut scan = ForeignScan::default();
            scan.items(&parsed.items, name);
            if let Some(line) = scan.impl_line {
                found = Some(format!("an impl of {name} at {relative}:{line}"));
                break;
            }
            implementing.extend(scan.implementing);
            calls.extend(
                scan.calls
                    .into_iter()
                    .map(|(line, called)| (relative.clone(), line, called)),
            );
        }
        if found.is_none() {
            found = calls
                .into_iter()
                .find(|(_, _, called)| implementing.contains(called))
                .map(|(relative, line, called)| {
                    format!(
                        "the call to {called}! at {relative}:{line}, which names {name} and whose macro spells impl,"
                    )
                });
        }
        self.foreign
            .borrow_mut()
            .insert(name.to_owned(), found.clone());
        found
    }
}

/// The deepest chain of aliases and impl-stated types the census follows.
const MAX_TYPE_DEPTH: usize = 32;

/// Where a type is written: the module its paths resolve in and the
/// parameters it can name.
struct Site<'a> {
    file: &'a str,
    scope: &'a [String],
    generics: &'a syn::Generics,
    /// For a type an impl states, the impl's own parameters: what fills them
    /// is not read.
    impl_parameters: Option<&'a [String]>,
}

/// What a source outside the wire crates states about one trait name.
#[derive(Default)]
struct ForeignScan {
    /// The first impl of a trait of that name.
    impl_line: Option<usize>,
    /// The names of the `macro_rules!` definitions whose body spells `impl`.
    implementing: BTreeSet<String>,
    /// Each macro call whose tokens name the trait, as its line and macro.
    calls: Vec<(usize, String)>,
}

impl ForeignScan {
    fn items(&mut self, items: &[syn::Item], name: &str) {
        for item in items {
            match item {
                syn::Item::Impl(implementation) => {
                    let named = implementation.trait_.as_ref().is_some_and(|(path, _)| {
                        path.segments
                            .last()
                            .is_some_and(|segment| segment.ident == name)
                    });
                    if named && self.impl_line.is_none() {
                        self.impl_line = Some(implementation.self_ty.span().start().line);
                    }
                }
                syn::Item::Mod(module) if !is_test_module(module) => {
                    if let Some((_, nested)) = &module.content {
                        self.items(nested, name);
                    }
                }
                syn::Item::Fn(function) => {
                    let (nested, calls) = function_body_items(function);
                    self.items(&nested, name);
                    for call in calls {
                        self.macro_use(&call.mac, None, name);
                    }
                }
                syn::Item::Macro(invocation) => {
                    self.macro_use(&invocation.mac, invocation.ident.as_ref(), name);
                }
                _ => {}
            }
        }
    }

    /// Records a `macro_rules!` definition that spells `impl`, or a call
    /// whose tokens name `name`.
    fn macro_use(&mut self, mac: &syn::Macro, defined: Option<&syn::Ident>, name: &str) {
        let mut tokens = Vec::new();
        flatten_tokens(&mac.tokens, &mut tokens);
        let Some(called) = mac.path.segments.last() else {
            return;
        };
        let defined = match defined {
            Some(defined) => Some(defined.to_string()),
            None if called.ident == "macro_rules" => tokens.first().cloned(),
            None => None,
        };
        match defined {
            Some(defined) => {
                if tokens.iter().any(|token| token == "impl") {
                    self.implementing.insert(defined);
                }
            }
            None => {
                if tokens.iter().any(|token| token == name) {
                    self.calls
                        .push((mac.path.span().start().line, called.ident.to_string()));
                }
            }
        }
    }
}

/// Every `*.rs` under the workspace `root`, outside the build output and the
/// hidden directories at the root.
fn workspace_sources(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let entries = std::fs::read_dir(root)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", root.display()));
    for entry in entries {
        let path = entry
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", root.display()))
            .path();
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if path.is_dir() {
            if name != "target" && !name.starts_with('.') {
                collect_rust_sources(&path, &mut files)
                    .unwrap_or_else(|error| panic!("cannot collect {name}: {error}"));
            }
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
    files.sort();
    files
}

/// The last segment of the path `ty` spells, through borrows.
fn type_head(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Reference(borrowed) => type_head(&borrowed.elem),
        syn::Type::Path(typed) => typed
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string()),
        _ => None,
    }
}

/// Whether `ty` names any of `names` as a path's first segment, at any
/// depth. A type shape this walk does not read counts as naming one.
fn mentions_any(ty: &syn::Type, names: &[&syn::Ident]) -> bool {
    match ty {
        syn::Type::Path(typed) => {
            typed
                .qself
                .as_ref()
                .is_some_and(|qself| mentions_any(&qself.ty, names))
                || typed
                    .path
                    .segments
                    .first()
                    .is_some_and(|segment| names.contains(&&segment.ident))
                || typed.path.segments.iter().any(|segment| match &segment.arguments {
                    syn::PathArguments::AngleBracketed(arguments) => {
                        arguments.args.iter().any(|argument| {
                            matches!(argument, syn::GenericArgument::Type(inner) if mentions_any(inner, names))
                        })
                    }
                    syn::PathArguments::None => false,
                    syn::PathArguments::Parenthesized(_) => true,
                })
        }
        syn::Type::Reference(inner) => mentions_any(&inner.elem, names),
        syn::Type::Array(inner) => mentions_any(&inner.elem, names),
        syn::Type::Slice(inner) => mentions_any(&inner.elem, names),
        syn::Type::Paren(inner) => mentions_any(&inner.elem, names),
        syn::Type::Group(inner) => mentions_any(&inner.elem, names),
        syn::Type::Ptr(inner) => mentions_any(&inner.elem, names),
        syn::Type::Tuple(tuple) => tuple.elems.iter().any(|inner| mentions_any(inner, names)),
        _ => true,
    }
}

/// Read `P::Name` or `<P as Trait>::Name`, through a borrow, as
/// `(P, Trait, Name)` when `P` is a type parameter of `generics`.
fn associated_type(
    ty: &syn::Type,
    generics: &syn::Generics,
) -> Option<(String, Option<syn::Path>, String)> {
    if let syn::Type::Reference(borrowed) = ty {
        return associated_type(&borrowed.elem, generics);
    }
    let syn::Type::Path(typed) = ty else {
        return None;
    };
    let segments: Vec<&syn::PathSegment> = typed.path.segments.iter().collect();
    let is_parameter = |name: &str| {
        generics.params.iter().any(|parameter| {
            matches!(parameter, syn::GenericParam::Type(declared) if declared.ident == name)
        })
    };
    if let Some(qself) = &typed.qself {
        let syn::Type::Path(receiver) = qself.ty.as_ref() else {
            return None;
        };
        let parameter = receiver.path.get_ident()?.to_string();
        if receiver.qself.is_some()
            || !is_parameter(&parameter)
            || qself.position == 0
            || segments.len() != qself.position + 1
        {
            return None;
        }
        let trait_path = syn::Path {
            leading_colon: typed.path.leading_colon,
            segments: segments[..qself.position]
                .iter()
                .map(|segment| (*segment).clone())
                .collect(),
        };
        let name = segments.last()?.ident.to_string();
        return Some((parameter, Some(trait_path), name));
    }
    let [receiver, associated] = segments.as_slice() else {
        return None;
    };
    let parameter = receiver.ident.to_string();
    if typed.path.leading_colon.is_some()
        || !receiver.arguments.is_none()
        || !associated.arguments.is_none()
        || !is_parameter(&parameter)
    {
        return None;
    }
    Some((parameter, None, associated.ident.to_string()))
}

/// The trait bounds a type parameter states in its header and where clause.
fn parameter_bounds(generics: &syn::Generics, parameter: &str) -> Vec<syn::Path> {
    let mut bounds = Vec::new();
    let mut read = |list: &syn::punctuated::Punctuated<syn::TypeParamBound, syn::Token![+]>| {
        for bound in list {
            if let syn::TypeParamBound::Trait(traited) = bound {
                if traited.maybe.is_none() {
                    bounds.push(traited.path.clone());
                }
            }
        }
    };
    for declared in &generics.params {
        if let syn::GenericParam::Type(declared) = declared {
            if declared.ident == parameter {
                read(&declared.bounds);
            }
        }
    }
    if let Some(clause) = &generics.where_clause {
        for predicate in &clause.predicates {
            let syn::WherePredicate::Type(predicate) = predicate else {
                continue;
            };
            let bounded = matches!(
                &predicate.bounded_ty,
                syn::Type::Path(typed)
                    if typed.qself.is_none() && typed.path.is_ident(parameter)
            );
            if bounded && predicate.lifetimes.is_none() {
                read(&predicate.bounds);
            }
        }
    }
    bounds
}

fn relative_path(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .expect("a collected source sits under the repository root")
        .to_string_lossy()
        .replace('\\', "/")
}

fn path_text(path: &syn::Path) -> String {
    let segments: Vec<String> = path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();
    let joined = segments.join("::");
    if path.leading_colon.is_some() {
        format!("::{joined}")
    } else {
        joined
    }
}

fn type_name(ty: &syn::Type) -> String {
    match ty {
        syn::Type::Path(typed) => path_text(&typed.path),
        syn::Type::Reference(borrowed) => format!("&{}", type_name(&borrowed.elem)),
        _ => "a type".to_owned(),
    }
}

#[cfg(test)]
mod tests;
