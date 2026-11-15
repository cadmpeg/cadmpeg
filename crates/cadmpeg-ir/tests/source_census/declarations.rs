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
    /// Associated type name -> whether each type the impl states holds an
    /// `Option`, or why the census cannot read it.
    types: BTreeMap<String, Vec<Result<bool, String>>>,
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
                syn::Item::Fn(function) => {
                    let nested: Vec<syn::Item> = function
                        .block
                        .stmts
                        .iter()
                        .filter_map(|statement| match statement {
                            syn::Stmt::Item(item) => Some(item.clone()),
                            _ => None,
                        })
                        .collect();
                    self.collect(&nested, relative, scope);
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
                syn::Item::Macro(invocation) => {
                    let mut tokens = Vec::new();
                    flatten_tokens(&invocation.mac.tokens, &mut tokens);
                    let line = invocation.mac.path.span().start().line;
                    let spells_impl = tokens.iter().any(|token| token == "impl");
                    if let Some(defined) = &invocation.ident {
                        if spells_impl {
                            self.implementing.insert(defined.to_string());
                        }
                    } else if let Some(called) = invocation.mac.path.segments.last() {
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
                _ => {}
            }
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
        let mut types: BTreeMap<String, Vec<Result<bool, String>>> = BTreeMap::new();
        for member in &implementation.items {
            let syn::ImplItem::Type(associated) = member else {
                continue;
            };
            let stated = if associated.generics.params.is_empty() {
                impl_type_is_optional(&associated.ty, &parameters)
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
            types,
        });
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

    /// Whether a field of type `ty`, declared with `generics` in module
    /// `scope` of `file`, holds an `Option`.
    ///
    /// A field spelled `Option<..>` does. A field spelled `P::Name` or
    /// `<P as Trait>::Name` for a type parameter `P` of `generics` holds the
    /// type each impl of the one bound trait that declares `Name` states, so
    /// it holds an `Option` when any impl's does.
    pub(crate) fn is_optional(
        &self,
        file: &str,
        scope: &[String],
        generics: &syn::Generics,
        ty: &syn::Type,
    ) -> Result<bool, String> {
        if is_option_type(ty) {
            return Ok(true);
        }
        let Some((parameter, named_trait, name)) = associated_type(ty, generics) else {
            return Ok(false);
        };
        let index = self.trait_index();
        let label = format!("{parameter}::{name}");
        let bounds = match named_trait {
            Some(path) => vec![path],
            None => parameter_bounds(generics, &parameter),
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
            match types.get(&name) {
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
            let Some(stated) = implementation.types.get(&name) else {
                return Err(format!(
                    "{} states no type {name}",
                    implementation.describe()
                ));
            };
            for value in stated {
                match value {
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

    /// An impl of a trait named `name` in a crate outside the wire crates.
    fn foreign_impl(&self, name: &str) -> Option<String> {
        if let Some(found) = self.foreign.borrow().get(name) {
            return found.clone();
        }
        let mut files = Vec::new();
        collect_rust_sources(&self.root.join("crates"), &mut files)
            .unwrap_or_else(|error| panic!("cannot collect crates: {error}"));
        files.sort();
        let mut found = None;
        for file in files {
            let relative = relative_path(&self.root, &file);
            if !relative.contains("/src/")
                || is_test_path(&relative)
                || WIRE_ROOTS
                    .iter()
                    .any(|root| relative.starts_with(&format!("{root}/")))
            {
                continue;
            }
            let text = std::fs::read_to_string(&file).expect("read source");
            if !text.contains(name) {
                continue;
            }
            let Ok(parsed) = syn::parse_file(&text) else {
                found = Some(format!(
                    "{relative}, which names {name} and does not parse,"
                ));
                break;
            };
            if let Some(line) = impl_of_named_trait(&parsed.items, name) {
                found = Some(format!("an impl of {name} at {relative}:{line}"));
                break;
            }
        }
        self.foreign
            .borrow_mut()
            .insert(name.to_owned(), found.clone());
        found
    }
}

/// The line of an impl, at any depth of `items`, of a trait whose path ends
/// in `name`.
fn impl_of_named_trait(items: &[syn::Item], name: &str) -> Option<usize> {
    for item in items {
        match item {
            syn::Item::Impl(implementation) => {
                if let Some((path, _)) = &implementation.trait_ {
                    if path
                        .segments
                        .last()
                        .is_some_and(|segment| segment.ident == name)
                    {
                        return Some(implementation.self_ty.span().start().line);
                    }
                }
            }
            syn::Item::Mod(module) if !is_test_module(module) => {
                if let Some((_, nested)) = &module.content {
                    if let Some(line) = impl_of_named_trait(nested, name) {
                        return Some(line);
                    }
                }
            }
            syn::Item::Fn(function) => {
                let nested: Vec<syn::Item> = function
                    .block
                    .stmts
                    .iter()
                    .filter_map(|statement| match statement {
                        syn::Stmt::Item(item) => Some(item.clone()),
                        _ => None,
                    })
                    .collect();
                if let Some(line) = impl_of_named_trait(&nested, name) {
                    return Some(line);
                }
            }
            _ => {}
        }
    }
    None
}

/// Whether one associated type an impl states holds an `Option`, or why the
/// census cannot read it: a type that names the impl's own parameters or
/// another associated type depends on what fills them.
fn impl_type_is_optional(ty: &syn::Type, parameters: &[String]) -> Result<bool, String> {
    if is_option_type(ty) {
        return Ok(true);
    }
    let syn::Type::Path(typed) = ty else {
        return Ok(false);
    };
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
    Ok(false)
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
mod tests {
    use std::path::PathBuf;

    use crate::optional_absence_census;

    /// A source tree holding the three wire crates, removed when dropped.
    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new(name: &str, files: &[(&str, &str)]) -> Self {
            let root = std::env::temp_dir().join(format!(
                "cadmpeg-source-census-{name}-{}",
                std::process::id()
            ));
            if root.exists() {
                std::fs::remove_dir_all(&root).expect("clear a stale fixture");
            }
            let mut files = files.to_vec();
            files.push(("crates/cadmpeg-core/src/lib.rs", ""));
            files.push(("crates/cadmpeg-asm/src/lib.rs", ""));
            for (path, text) in files {
                let file = root.join(path);
                std::fs::create_dir_all(file.parent().expect("a fixture file has a parent"))
                    .expect("create a fixture directory");
                std::fs::write(&file, text).expect("write a fixture file");
            }
            Self { root }
        }

        fn offenders(&self) -> Vec<String> {
            optional_absence_census(&self.root).1
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            if let Err(error) = std::fs::remove_dir_all(&self.root) {
                eprintln!("remove {}: {error}", self.root.display());
            }
        }
    }

    const NULLABLE_NAME: &str = r#"
        #[derive(Serialize, Deserialize)]
        pub struct Sketch {
            #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
            pub name: Option<String>,
        }
    "#;

    #[test]
    fn same_named_types_in_two_modules_are_each_their_own_writer() {
        let fixture = Fixture::new(
            "same-named-writers",
            &[
                ("crates/cadmpeg-ir/src/lib.rs", "mod sketches; mod design;"),
                ("crates/cadmpeg-ir/src/sketches.rs", NULLABLE_NAME),
                (
                    "crates/cadmpeg-ir/src/design.rs",
                    &NULLABLE_NAME.replace(
                        "#[serde(deserialize_with",
                        "#[serde(skip_serializing_if = \"Option::is_none\", deserialize_with",
                    ),
                ),
            ],
        );
        let offenders = fixture.offenders();
        assert_eq!(offenders.len(), 1, "{offenders:#?}");
        assert!(
            offenders[0].starts_with("crates/cadmpeg-ir/src/design.rs:")
                && offenders[0]
                    .contains("admits null, but cadmpeg_ir::design::Sketch skips the key for None"),
            "{offenders:#?}"
        );
    }

    const ROUTED: &str = r#"
        #[derive(Deserialize)]
        struct SketchWire {
            #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
            name: Option<String>,
        }
        #[derive(Serialize, Deserialize)]
        #[serde(try_from = "SketchWire")]
        pub struct Sketch {
            SKIP
            pub name: Option<String>,
        }
    "#;

    #[test]
    fn each_wire_is_paired_with_the_owner_in_its_own_module() {
        let fixture = Fixture::new(
            "same-named-wires",
            &[
                ("crates/cadmpeg-ir/src/lib.rs", "mod sketches; mod design;"),
                (
                    "crates/cadmpeg-ir/src/sketches.rs",
                    &ROUTED.replace(
                        "SKIP",
                        "#[serde(skip_serializing_if = \"Option::is_none\")]",
                    ),
                ),
                (
                    "crates/cadmpeg-ir/src/design.rs",
                    &ROUTED.replace("SKIP", ""),
                ),
            ],
        );
        let offenders = fixture.offenders();
        assert_eq!(offenders.len(), 1, "{offenders:#?}");
        assert!(
            offenders[0].starts_with("crates/cadmpeg-ir/src/sketches.rs:")
                && offenders[0].contains(
                    "SketchWire.name admits null, but cadmpeg_ir::sketches::Sketch skips"
                ),
            "{offenders:#?}"
        );
    }

    #[test]
    fn a_conversion_that_resolves_nowhere_leaves_its_wire_unpaired() {
        let fixture = Fixture::new(
            "unresolved-wire",
            &[
                ("crates/cadmpeg-ir/src/lib.rs", "mod design;"),
                (
                    "crates/cadmpeg-ir/src/design.rs",
                    &ROUTED.replace("SKIP", "").replace(
                        "try_from = \"SketchWire\"",
                        "try_from = \"elsewhere::SketchWire\"",
                    ),
                ),
            ],
        );
        let offenders = fixture.offenders();
        assert_eq!(offenders.len(), 1, "{offenders:#?}");
        assert!(
            offenders[0].contains("SketchWire.name cadmpeg_ir::design::SketchWire may be read for cadmpeg_ir::design::Sketch"),
            "{offenders:#?}"
        );
    }

    const SPACE: &str = r"
        pub trait Space {
            type Point;
        }
        pub struct Planar;
        impl Space for Planar {
            type Point = PLANAR;
        }
        pub struct Spatial;
        impl Space for Spatial {
            type Point = u8;
        }
        #[derive(Serialize, Deserialize)]
        pub struct Entity<S: Space> {
            pub point: S::Point,
        }
    ";

    fn space(name: &str, source: &str) -> Fixture {
        Fixture::new(
            name,
            &[
                ("crates/cadmpeg-ir/src/lib.rs", "mod space;"),
                ("crates/cadmpeg-ir/src/space.rs", source),
            ],
        )
    }

    #[test]
    fn an_associated_type_no_impl_states_as_an_option_is_no_optional_key() {
        let offenders = space("associated-plain", &SPACE.replace("PLANAR", "u16")).offenders();
        assert!(offenders.is_empty(), "{offenders:#?}");
    }

    #[test]
    fn an_associated_type_one_impl_states_as_an_option_is_an_optional_key() {
        for source in [
            SPACE.replace("PLANAR", "Option<u16>"),
            SPACE
                .replace("PLANAR", "Option<u16>")
                .replace("S::Point", "<S as Space>::Point"),
            SPACE
                .replace("PLANAR", "Option<u16>")
                .replace("Entity<S: Space> {", "Entity<S> where S: Space {"),
        ] {
            let offenders = space("associated-option", &source).offenders();
            assert_eq!(offenders.len(), 1, "{offenders:#?}");
            assert!(
                offenders[0].starts_with("crates/cadmpeg-ir/src/space.rs:")
                    && offenders[0].contains("Entity.point states no deserialize_with"),
                "{offenders:#?}"
            );
        }
    }

    #[test]
    fn an_associated_type_the_census_cannot_read_is_an_offender() {
        let plain = SPACE.replace("PLANAR", "u16");
        for (source, reason) in [
            (
                plain.replace("type Point = u8;", ""),
                "impl Space for Spatial at crates/cadmpeg-ir/src/space.rs:",
            ),
            (
                plain.replace("Entity<S: Space>", "Entity<S>"),
                "S::Point names a type parameter with no trait bound that declares Point",
            ),
            (
                plain.replace("Entity<S: Space>", "Entity<S: Space + external::Mystery>"),
                "the bound external::Mystery of S is no wire-crate trait",
            ),
            (
                plain.replace("type Point;", "type Point = u8;"),
                "cadmpeg_ir::space::Space::Point states a default",
            ),
            (
                plain.replace("type Point = u8;", "type Point = Self::Other;"),
                "the type Self::Other depends on what fills the impl",
            ),
            (
                plain
                    .replace(
                        "impl Space for Planar {\n            type Point = u16;\n        }",
                        "",
                    )
                    .replace(
                        "impl Space for Spatial {\n            type Point = u8;\n        }",
                        "",
                    ),
                "cadmpeg_ir::space::Space has no impl in the wire crates",
            ),
        ] {
            let offenders = space("associated-unread", &source).offenders();
            assert_eq!(offenders.len(), 1, "{reason}: {offenders:#?}");
            assert!(offenders[0].contains(reason), "{reason}: {offenders:#?}");
        }
    }

    #[test]
    fn an_impl_outside_the_wire_crates_leaves_the_associated_type_unread() {
        let fixture = Fixture::new(
            "associated-foreign",
            &[
                ("crates/cadmpeg-ir/src/lib.rs", "mod space;"),
                ("crates/cadmpeg-ir/src/space.rs", &SPACE.replace("PLANAR", "u16")),
                (
                    "crates/codec/src/lib.rs",
                    "pub struct Codec; impl cadmpeg_ir::space::Space for Codec { type Point = u8; }",
                ),
            ],
        );
        let offenders = fixture.offenders();
        assert_eq!(offenders.len(), 1, "{offenders:#?}");
        assert!(
            offenders[0].contains(
                "an impl of Space at crates/codec/src/lib.rs:1 is outside the wire crates"
            ),
            "{offenders:#?}"
        );
    }

    #[test]
    fn a_macro_call_naming_the_trait_leaves_its_impls_unread() {
        let source = SPACE.replace("PLANAR", "u16")
            + r"
                macro_rules! implement {
                    ($trait:ident, $name:ident) => {
                        impl $trait for $name { type Point = Option<u8>; }
                    };
                }
                pub struct Extra;
                implement!(Space, Extra);
            ";
        let offenders = space("associated-macro", &source).offenders();
        assert_eq!(offenders.len(), 1, "{offenders:#?}");
        assert!(
            offenders[0].contains("the call to implement! at crates/cadmpeg-ir/src/space.rs:"),
            "{offenders:#?}"
        );
    }

    #[test]
    fn a_same_named_trait_in_another_module_is_another_trait() {
        let fixture = Fixture::new(
            "associated-same-named-trait",
            &[
                ("crates/cadmpeg-ir/src/lib.rs", "mod space; mod other;"),
                ("crates/cadmpeg-ir/src/space.rs", &SPACE.replace("PLANAR", "u16")),
                (
                    "crates/cadmpeg-ir/src/other.rs",
                    "pub trait Space { type Point; } pub struct Loose; impl Space for Loose { type Point = Option<u8>; }",
                ),
            ],
        );
        let offenders = fixture.offenders();
        assert!(offenders.is_empty(), "{offenders:#?}");
    }
}
