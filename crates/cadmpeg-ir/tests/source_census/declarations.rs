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
//! never a field taken to hold no `Option`. A field typed by a bare type
//! parameter holds whatever fills it: the default and every argument a wire
//! declaration states for it.

use std::cell::{OnceCell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use syn::spanned::Spanned;

use crate::reader_routes::DeclarationResolver;
use crate::{collect_rust_sources, is_option_type, is_test_module, is_test_path};
use tokens::{declaring_macro, FileTokens, MacroTree, TreeKind};

mod tokens;

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
    /// The line of its `impl` keyword.
    impl_line: usize,
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

/// Where a wire declaration writes a type with generic arguments: the fields
/// and parameter defaults of a struct or enum, the type an alias names, and
/// the types a trait impl states.
struct UseSite {
    path: String,
    scope: Vec<String>,
    /// The struct, enum or alias that writes it; `None` for an impl.
    owner: Option<Declaration>,
    generics: syn::Generics,
    /// For an impl, its own type parameters and its self type: what fills a
    /// parameter is what fills the self type's argument that names it.
    impl_parameters: Option<Vec<String>>,
    self_type: Option<syn::Type>,
}

/// One type written with generic arguments, as `Name<..>` or `Name::<..>`.
struct GenericUse {
    /// The index of its [`UseSite`].
    site: usize,
    line: usize,
    /// The path's last segment.
    name: String,
    /// The path without its arguments.
    path: syn::Path,
    /// Its type arguments, in order.
    arguments: Vec<syn::Type>,
}

/// The declarations of the wire crates, their traits, and every trait impl.
///
/// Each index is read from the source on first use.
pub(crate) struct Declarations {
    root: PathBuf,
    resolver: OnceCell<DeclarationResolver>,
    traits: OnceCell<TraitIndex>,
    /// Per trait name, why an impl of it outside the wire crates is unread.
    foreign: RefCell<BTreeMap<String, Option<String>>>,
    /// Each parameter position whose instantiations are being read, so a
    /// declaration that instantiates itself states no new type.
    enumerating: RefCell<BTreeSet<(Declaration, usize)>>,
}

/// The traits and trait impls of the wire crates.
#[derive(Default)]
struct TraitIndex {
    traits: BTreeMap<Declaration, BTreeMap<String, AssociatedDeclaration>>,
    impls: Vec<TraitImpl>,
    aliases: BTreeMap<Declaration, Alias>,
    sites: Vec<UseSite>,
    uses: Vec<GenericUse>,
    /// What each wire source states in tokens, by repository-relative path.
    tokens: Vec<(String, FileTokens)>,
}

impl Declarations {
    pub(crate) fn read(root: &Path) -> Self {
        Self {
            root: root.to_owned(),
            resolver: OnceCell::new(),
            traits: OnceCell::new(),
            foreign: RefCell::new(BTreeMap::new()),
            enumerating: RefCell::new(BTreeSet::new()),
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
                let found = tokens::scan(&text, &test_module_lines(&parsed.items))
                    .expect("every source file in the wire crates parses, so it lexes");
                index.tokens.push((relative, found));
            }
        }
        index
    }

    /// Records each type with generic arguments that `ty` writes, at any
    /// depth, as written at `site`.
    fn record_uses(&mut self, ty: &syn::Type, site: usize) {
        match ty {
            syn::Type::Path(typed) => {
                if let Some(qself) = &typed.qself {
                    self.record_uses(&qself.ty, site);
                }
                for segment in &typed.path.segments {
                    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
                        continue;
                    };
                    for argument in &arguments.args {
                        match argument {
                            syn::GenericArgument::Type(inner) => self.record_uses(inner, site),
                            syn::GenericArgument::AssocType(bound) => {
                                self.record_uses(&bound.ty, site);
                            }
                            _ => {}
                        }
                    }
                }
                let Some(last) = typed.path.segments.last() else {
                    return;
                };
                let syn::PathArguments::AngleBracketed(arguments) = &last.arguments else {
                    return;
                };
                if typed.qself.is_some() {
                    return;
                }
                let mut path = typed.path.clone();
                if let Some(segment) = path.segments.last_mut() {
                    segment.arguments = syn::PathArguments::None;
                }
                self.uses.push(GenericUse {
                    site,
                    line: last.ident.span().start().line,
                    name: last.ident.to_string(),
                    path,
                    arguments: arguments
                        .args
                        .iter()
                        .filter_map(|argument| match argument {
                            syn::GenericArgument::Type(inner) => Some(inner.clone()),
                            _ => None,
                        })
                        .collect(),
                });
            }
            syn::Type::Reference(inner) => self.record_uses(&inner.elem, site),
            syn::Type::Array(inner) => self.record_uses(&inner.elem, site),
            syn::Type::Slice(inner) => self.record_uses(&inner.elem, site),
            syn::Type::Paren(inner) => self.record_uses(&inner.elem, site),
            syn::Type::Group(inner) => self.record_uses(&inner.elem, site),
            syn::Type::Ptr(inner) => self.record_uses(&inner.elem, site),
            syn::Type::Tuple(tuple) => {
                for inner in &tuple.elems {
                    self.record_uses(inner, site);
                }
            }
            _ => {}
        }
    }

    /// Records the uses a struct, enum or alias writes in its fields, its
    /// parameter defaults and `types`.
    fn record_declaration_uses<'a>(
        &mut self,
        owner: Declaration,
        generics: &syn::Generics,
        types: impl IntoIterator<Item = &'a syn::Type>,
    ) {
        let site = self.sites.len();
        self.sites.push(UseSite {
            path: owner.path.clone(),
            scope: owner.scope.clone(),
            owner: Some(owner),
            generics: generics.clone(),
            impl_parameters: None,
            self_type: None,
        });
        for parameter in generics.type_params() {
            if let Some((_, default)) = &parameter.default {
                self.record_uses(default, site);
            }
        }
        for ty in types {
            self.record_uses(ty, site);
        }
    }

    fn collect(&mut self, items: &[syn::Item], relative: &str, scope: &[String]) {
        let declaration = |name: &syn::Ident| Declaration {
            path: relative.to_owned(),
            scope: scope.to_owned(),
            name: name.to_string(),
        };
        for item in items {
            match item {
                syn::Item::Struct(declared) => {
                    let fields = declared.fields.iter().map(|field| &field.ty);
                    self.record_declaration_uses(
                        declaration(&declared.ident),
                        &declared.generics,
                        fields,
                    );
                }
                syn::Item::Enum(declared) => {
                    let fields = declared
                        .variants
                        .iter()
                        .flat_map(|variant| variant.fields.iter().map(|field| &field.ty));
                    self.record_declaration_uses(
                        declaration(&declared.ident),
                        &declared.generics,
                        fields,
                    );
                }
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
                    self.record_declaration_uses(
                        declaration(&alias.ident),
                        &alias.generics,
                        [alias.ty.as_ref()],
                    );
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
                    self.collect(&function_body_items(function), relative, scope);
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
                _ => {}
            }
        }
    }

    /// Every `macro_rules!` tree of the wire crates.
    fn definitions(&self) -> Vec<&MacroTree> {
        self.tokens
            .iter()
            .flat_map(|(_, found)| &found.trees)
            .filter(|tree| tree.kind == TreeKind::Definition)
            .collect()
    }

    /// `name` and every name a wire-crate `use` renames it to, transitively.
    fn renamed(&self, name: &str) -> Vec<String> {
        renamed(
            name,
            self.tokens.iter().flat_map(|(_, found)| &found.renames),
        )
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
        let site = self.sites.len();
        self.sites.push(UseSite {
            path: relative.to_owned(),
            scope: scope.to_owned(),
            owner: None,
            generics: syn::Generics::default(),
            impl_parameters: Some(parameters.clone()),
            self_type: Some((*implementation.self_ty).clone()),
        });
        // A `Deserialize` impl reads the type it is written for.
        if trait_path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "Deserialize")
        {
            self.record_uses(&implementation.self_ty, site);
        }
        let mut types: BTreeMap<String, Vec<Result<syn::Type, String>>> = BTreeMap::new();
        for member in &implementation.items {
            let syn::ImplItem::Type(associated) = member else {
                continue;
            };
            self.record_uses(&associated.ty, site);
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
            impl_line: implementation.impl_token.span.start().line,
            trait_path: trait_path.clone(),
            self_type: type_name(&implementation.self_ty),
            parameters,
            types,
        });
    }
}

/// The items a function body declares in statement position.
fn function_body_items(function: &syn::ItemFn) -> Vec<syn::Item> {
    function
        .block
        .stmts
        .iter()
        .filter_map(|statement| match statement {
            syn::Stmt::Item(item) => Some(item.clone()),
            _ => None,
        })
        .collect()
}

/// The inclusive line ranges of the test modules among `items`, at any depth.
fn test_module_lines(items: &[syn::Item]) -> Vec<(usize, usize)> {
    let mut lines = Vec::new();
    for item in items {
        let syn::Item::Mod(module) = item else {
            continue;
        };
        let first = module.mod_token.span.start().line;
        match &module.content {
            Some((brace, _)) if is_test_module(module) => {
                lines.push((first, brace.span.close().end().line));
            }
            Some((_, nested)) => lines.extend(test_module_lines(nested)),
            None => {}
        }
    }
    lines
}

/// `name` and every name one of `renames` gives it, transitively. A rename
/// of another declaration of the same name is included: each use the extra
/// name finds is still resolved, so it widens what is read and proves nothing.
fn renamed<'a>(name: &str, renames: impl Iterator<Item = &'a (String, String)>) -> Vec<String> {
    let renames: Vec<&(String, String)> = renames.collect();
    let mut names = vec![name.to_owned()];
    loop {
        let before = names.len();
        for (original, alias) in &renames {
            if names.contains(original) && !names.contains(alias) {
                names.push(alias.clone());
            }
        }
        if names.len() == before {
            return names;
        }
    }
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

    /// Whether a field of type `ty` of `owner`, declared with `generics`,
    /// holds an `Option`.
    ///
    /// A field spelled `Option<..>` does. A field spelled `P::Name` or
    /// `<P as Trait>::Name` for a type parameter `P` of `generics` holds the
    /// type each impl of the one bound trait that declares `Name` states, so
    /// it holds an `Option` when any impl's does. A field spelled by a bare
    /// type parameter holds an `Option` when its default or any argument a
    /// wire declaration states for it does. A field spelled by a type alias
    /// holds what the alias names, read where the alias is declared; an alias
    /// that names one of its own parameters holds the argument the field
    /// states for it.
    pub(crate) fn is_optional(
        &self,
        owner: &Declaration,
        generics: &syn::Generics,
        ty: &syn::Type,
    ) -> Result<bool, String> {
        let site = Site {
            file: &owner.path,
            scope: &owner.scope,
            generics,
            impl_parameters: None,
            owner: Some(owner),
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
        if let Some(parameter) = bare_parameter(ty, site.generics) {
            return self.parameter_holds_option(site, parameter, depth);
        }
        if let Some((parameter, named_trait, name)) = associated_type(ty, site.generics) {
            return self.associated_holds_option(site, &parameter, named_trait, &name, depth);
        }
        self.alias_holds_option(site, ty, depth)
    }

    /// Whether the type parameter `parameter` of `site`'s declaration can be
    /// filled with an `Option`: by its default, or by an argument a wire
    /// declaration states for it, read where that declaration writes it.
    ///
    /// A `Deserialize` impl's self type is a use too. A use whose path
    /// resolves to no one declaration, and a macro token tree that can
    /// declare a use, leave the parameter unread, as does a field's own
    /// parameter with no default and no use. A use that forwards the
    /// parameter back to a position being read states no new type.
    fn parameter_holds_option(
        &self,
        site: &Site<'_>,
        parameter: &syn::Ident,
        depth: usize,
    ) -> Result<bool, String> {
        let Some(owner) = site.owner else {
            return Err(format!(
                "the type {parameter} is a parameter whose instantiations the census does not read"
            ));
        };
        let parameters: Vec<&syn::TypeParam> = site.generics.type_params().collect();
        let Some(position) = parameters
            .iter()
            .position(|declared| declared.ident == *parameter)
        else {
            return Err(format!("{parameter} is no type parameter of {owner}"));
        };
        let key = (owner.clone(), position);
        let forwarding = !self.enumerating.borrow().is_empty();
        if !self.enumerating.borrow_mut().insert(key.clone()) {
            return Ok(false);
        }
        let read = self.instantiations_hold_option(
            site,
            owner,
            parameters[position],
            position,
            forwarding,
            depth,
        );
        self.enumerating.borrow_mut().remove(&key);
        read
    }

    /// Whether the default or an argument of `owner`'s parameter at
    /// `position` holds an `Option`. When `forwarding`, another declaration's
    /// parameter is filled with this one, so a parameter nothing fills
    /// states nothing that reaches it.
    fn instantiations_hold_option(
        &self,
        site: &Site<'_>,
        owner: &Declaration,
        parameter: &syn::TypeParam,
        position: usize,
        forwarding: bool,
        depth: usize,
    ) -> Result<bool, String> {
        let index = self.trait_index();
        let names = index.renamed(&owner.name);
        let label = &parameter.ident;
        let definitions = index.definitions();
        for (relative, found) in &index.tokens {
            for tree in &found.trees {
                let Some(name) = tree.instantiates(&names) else {
                    continue;
                };
                if tree.kind == TreeKind::Attribute
                    || tree.spells_declaration()
                    || declaring_macro(&tree.name, &definitions, &mut Vec::new())
                {
                    return Err(format!(
                        "{} spells {name}<..>, and the census does not expand it, so its argument for {label} is unread",
                        tree.describe(relative)
                    ));
                }
            }
        }
        let mut stated = false;
        let mut optional = false;
        if let Some((_, default)) = &parameter.default {
            stated = true;
            optional |= self.holds_option(site, default, depth + 1)?;
        }
        for usage in index
            .uses
            .iter()
            .filter(|usage| names.contains(&usage.name))
        {
            let written = &index.sites[usage.site];
            let resolved = self
                .resolver()
                .resolve_across_crates(&written.path, &written.scope, &usage.path)
                .map(|(path, scope, name)| Declaration { path, scope, name });
            let Some(resolved) = resolved else {
                return Err(format!(
                    "the use of {} at {}:{} resolves to no one declaration, so its argument for {label} is unread",
                    path_text(&usage.path),
                    written.path,
                    usage.line
                ));
            };
            if resolved != *owner {
                continue;
            }
            let Some(argument) = usage.arguments.get(position) else {
                continue;
            };
            stated = true;
            if let Some(forwarded) = impl_parameter(argument, written) {
                optional |= self.impl_parameter_holds_option(written, forwarded, depth + 1)?;
                continue;
            }
            let use_site = Site {
                file: &written.path,
                scope: &written.scope,
                generics: &written.generics,
                impl_parameters: written.impl_parameters.as_deref(),
                owner: written.owner.as_ref(),
            };
            optional |= self.holds_option(&use_site, argument, depth + 1)?;
        }
        if !stated && !forwarding {
            return Err(format!(
                "{owner} has no default and no instantiation in the wire crates for its parameter {label}"
            ));
        }
        Ok(optional)
    }

    /// Whether the impl parameter `parameter` of the impl `written` can be
    /// filled with an `Option`: whether the self type's argument it names
    /// can be. A parameter the self type does not name bare, and a self type
    /// that is no wire declaration, are unread.
    fn impl_parameter_holds_option(
        &self,
        written: &UseSite,
        parameter: &syn::Ident,
        depth: usize,
    ) -> Result<bool, String> {
        let unread = || {
            Err(format!(
                "the impl at {} fills {parameter} with what fills its self type, which the census does not read",
                written.path
            ))
        };
        let mut self_type = written.self_type.as_ref();
        while let Some(syn::Type::Reference(borrowed)) = self_type {
            self_type = Some(&borrowed.elem);
        }
        let Some(syn::Type::Path(typed)) = self_type else {
            return unread();
        };
        let Some(syn::PathArguments::AngleBracketed(arguments)) =
            typed.path.segments.last().map(|segment| &segment.arguments)
        else {
            return unread();
        };
        let Some(position) = arguments
            .args
            .iter()
            .filter(|argument| matches!(argument, syn::GenericArgument::Type(_)))
            .position(|argument| {
                matches!(argument, syn::GenericArgument::Type(syn::Type::Path(named))
                    if named.qself.is_none() && named.path.is_ident(parameter))
            })
        else {
            return unread();
        };
        let mut path = typed.path.clone();
        if let Some(segment) = path.segments.last_mut() {
            segment.arguments = syn::PathArguments::None;
        }
        let Some(declared) = self
            .resolver()
            .resolve_across_crates(&written.path, &written.scope, &path)
            .map(|(path, scope, name)| Declaration { path, scope, name })
        else {
            return unread();
        };
        let index = self.trait_index();
        let Some(declaration) = index
            .sites
            .iter()
            .find(|site| site.owner.as_ref() == Some(&declared))
        else {
            return unread();
        };
        let Some(filled) = declaration.generics.type_params().nth(position) else {
            return unread();
        };
        let site = Site {
            file: &declared.path,
            scope: &declared.scope,
            generics: &declaration.generics,
            impl_parameters: None,
            owner: Some(&declared),
        };
        self.parameter_holds_option(&site, &filled.ident, depth)
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
            owner: Some(&declaration),
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
        let names = index.renamed(&trait_declaration.name);
        if let Some(reason) = index.unread_impls(&names) {
            return Err(reason);
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
                .is_none_or(|segment| !names.contains(&segment.ident.to_string()))
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
                owner: None,
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

    /// Why an impl of the trait named `name` outside the wire crates' sources
    /// is unread, or `None`.
    ///
    /// Only an impl in a wire crate's `src` is read; one in another crate,
    /// in `examples/`, in `benches/` or in a build script states a type the
    /// census does not read. The trait is named by `name` and by every name a
    /// `use` there or in the wire crates renames it to. An impl of it, a
    /// macro token tree that names it, and a source that names it and does
    /// not parse each leave it unread.
    fn foreign_impl(&self, name: &str) -> Option<String> {
        if let Some(found) = self.foreign.borrow().get(name) {
            return found.clone();
        }
        let index = self.trait_index();
        let definitions = index.definitions();
        let mut names = index.renamed(name);
        let mut scanned: BTreeMap<String, FileTokens> = BTreeMap::new();
        let mut found = None;
        let foreign: Vec<(String, PathBuf)> = workspace_sources(&self.root)
            .into_iter()
            .map(|file| (relative_path(&self.root, &file), file))
            // The leading separator makes a test tree at the workspace root
            // one too.
            .filter(|(relative, _)| {
                !is_test_path(&format!("/{relative}"))
                    && !WIRE_ROOTS
                        .iter()
                        .any(|root| relative.starts_with(&format!("{root}/")))
            })
            .collect();
        'renames: loop {
            for (relative, file) in &foreign {
                if scanned.contains_key(relative) {
                    continue;
                }
                let text = std::fs::read_to_string(file).expect("read source");
                let Some(named) = names.iter().find(|named| text.contains(named.as_str())) else {
                    continue;
                };
                let tokens = syn::parse_file(&text)
                    .ok()
                    .and_then(|parsed| tokens::scan(&text, &test_module_lines(&parsed.items)));
                let Some(tokens) = tokens else {
                    found = Some(format!(
                        "{relative}, which names {named} and does not parse,"
                    ));
                    break 'renames;
                };
                let before = names.len();
                scanned.insert(relative.clone(), tokens);
                names = renamed(
                    name,
                    index
                        .tokens
                        .iter()
                        .map(|(_, wire)| wire)
                        .chain(scanned.values())
                        .flat_map(|scan| &scan.renames),
                );
                if names.len() != before {
                    continue 'renames;
                }
            }
            break;
        }
        if found.is_none() {
            found = scanned.iter().find_map(|(relative, tokens)| {
                if let Some((named, line)) = tokens
                    .impl_headers
                    .iter()
                    .find(|(named, _)| names.contains(named))
                {
                    return Some(format!("an impl of {named} at {relative}:{line}"));
                }
                let tree = tokens
                    .trees
                    .iter()
                    .find(|tree| tree.names(&names).is_some())?;
                let named = tree.names(&names)?;
                let implementing = tree.kind == TreeKind::Call
                    && definitions
                        .iter()
                        .copied()
                        .chain(scanned.values().flat_map(|scan| &scan.trees))
                        .any(|defined| {
                            defined.kind == TreeKind::Definition
                                && defined.name == tree.name
                                && defined.tokens.iter().any(|token| token == "impl")
                        });
                Some(if implementing {
                    format!(
                        "{}, which names {named} and whose macro spells impl,",
                        tree.describe(relative)
                    )
                } else {
                    format!("{}, which names {named},", tree.describe(relative))
                })
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
    /// The struct, enum or alias whose `generics` these are, whose uses state
    /// what fills them.
    owner: Option<&'a Declaration>,
}

impl TraitIndex {
    /// Why a wire-crate impl of a trait named by one of `names` is not one
    /// the census reads, or `None`.
    ///
    /// The census expands no macro. A token tree naming the trait can expand
    /// to an impl no parsed item states: a `macro_rules!` body, a call (to a
    /// renamed macro, to one that calls another, or to one from another
    /// crate), and an attribute a derive or attribute macro reads. An impl
    /// header the item walk does not reach, such as one in a block, is unread
    /// too.
    fn unread_impls(&self, names: &[String]) -> Option<String> {
        let definitions = self.definitions();
        for (relative, found) in &self.tokens {
            for tree in &found.trees {
                let Some(named) = tree.names(names) else {
                    continue;
                };
                let spells_impl =
                    |tree: &MacroTree| tree.tokens.iter().any(|token| token == "impl");
                let why = match tree.kind {
                    TreeKind::Definition if spells_impl(tree) => format!(
                        "the macro at {relative}:{} spells impl and {named}",
                        tree.line
                    ),
                    TreeKind::Call
                        if definitions
                            .iter()
                            .any(|defined| defined.name == tree.name && spells_impl(defined)) =>
                    {
                        format!(
                            "{} names {named} and the macro spells impl",
                            tree.describe(relative)
                        )
                    }
                    _ => format!(
                        "{} names {named}, and the census does not expand it",
                        tree.describe(relative)
                    ),
                };
                return Some(format!("{why}, so an impl it expands is unread"));
            }
            for (named, line) in found
                .impl_headers
                .iter()
                .filter(|(named, _)| names.contains(named))
            {
                let read = self.impls.iter().any(|implementation| {
                    implementation.path == *relative && implementation.impl_line == *line
                });
                if !read {
                    return Some(format!(
                        "the impl of {named} at {relative}:{line} sits where the census reads no impl, so its types are unread"
                    ));
                }
            }
        }
        None
    }
}

/// The impl parameter of `written` that `ty` names bare.
fn impl_parameter<'a>(ty: &'a syn::Type, written: &UseSite) -> Option<&'a syn::Ident> {
    let syn::Type::Path(typed) = ty else {
        return None;
    };
    let ident = typed.path.get_ident().filter(|_| typed.qself.is_none())?;
    written
        .impl_parameters
        .as_ref()?
        .contains(&ident.to_string())
        .then_some(ident)
}

/// The type parameter of `generics` that `ty` names bare, through borrows.
fn bare_parameter<'a>(ty: &syn::Type, generics: &'a syn::Generics) -> Option<&'a syn::Ident> {
    match ty {
        syn::Type::Reference(borrowed) => bare_parameter(&borrowed.elem, generics),
        syn::Type::Paren(inner) => bare_parameter(&inner.elem, generics),
        syn::Type::Group(inner) => bare_parameter(&inner.elem, generics),
        syn::Type::Path(typed) if typed.qself.is_none() => {
            let ident = typed.path.get_ident()?;
            generics
                .type_params()
                .map(|declared| &declared.ident)
                .find(|declared| *declared == ident)
        }
        _ => None,
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
