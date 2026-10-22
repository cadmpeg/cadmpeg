// SPDX-License-Identifier: Apache-2.0
//! Call-path resolution through module re-exports, aliases and glob imports.
//!
//! A path resolves to an exact workspace function, through named `use`
//! re-exports, renamed imports, type aliases and glob imports. A path that
//! names a type, constant, variant or module is not a call. A path rooted
//! outside the workspace (std, core, alloc, a dependency crate, a primitive
//! or a prelude type) is external. A call through a trait path dispatches to
//! every method of its name, and a name that a `macro_rules!` body generates
//! reaches that body. Every other path is unresolved, and the census reports
//! it as a finding.

use std::collections::{BTreeMap, BTreeSet};

use super::{symbol_candidates, Function, Graph};

/// The non-function workspace items a path can name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Item {
    /// A struct, enum, union or type alias.
    Type,
    /// A trait.
    Trait,
    /// A constant, static or enum variant.
    Value,
}

/// The bound on alias expansions of one path; re-export chains are short.
pub(super) const EXPANSION_LIMIT: usize = 256;

/// Roots that name no workspace item.
const BUILTIN_ROOTS: &[&str] = &[
    "std",
    "core",
    "alloc",
    "proc_macro",
    "bool",
    "char",
    "str",
    "u8",
    "u16",
    "u32",
    "u64",
    "u128",
    "usize",
    "i8",
    "i16",
    "i32",
    "i64",
    "i128",
    "isize",
    "f32",
    "f64",
    "Option",
    "Result",
    "Vec",
    "String",
    "Box",
    "Some",
    "None",
    "Ok",
    "Err",
];

/// Prelude traits: a call through one dispatches to every method of its name.
const PRELUDE_TRAITS: &[&str] = &[
    "AsMut",
    "AsRef",
    "Clone",
    "Default",
    "DoubleEndedIterator",
    "Drop",
    "Eq",
    "ExactSizeIterator",
    "Extend",
    "Fn",
    "FnMut",
    "FnOnce",
    "From",
    "FromIterator",
    "Into",
    "IntoIterator",
    "Iterator",
    "Ord",
    "PartialEq",
    "PartialOrd",
    "ToOwned",
    "ToString",
    "TryFrom",
    "TryInto",
];

/// Error enums whose variant vocabulary the census rules name directly.
const ERROR_FAMILIES: &[(&str, &[&str])] = &[
    (
        "CodecError",
        &[
            "WrongFormat",
            "Malformed",
            "InvalidInput",
            "Truncated",
            "ResourceLimit",
            "UnsupportedDialect",
            "UnsupportedTarget",
            "UnsupportedContent",
            "NotImplemented",
            "Io",
        ],
    ),
    (
        "EncodeError",
        &[
            "InvalidInput",
            "TargetCapacity",
            "UnsupportedTarget",
            "ResourceLimit",
            "Io",
        ],
    ),
    (
        "TargetRefusalKind",
        &[
            "ExplicitUnavailable",
            "InheritedUnavailable",
            "DefaultUnavailable",
            "UnrecordedSource",
        ],
    ),
];

/// How one call path resolved.
pub(super) enum Resolution {
    /// Workspace functions, exact or by the conservative fallback.
    Functions(Vec<usize>),
    /// A type, constant, variant or module: not a function call.
    NotCallable,
    /// Rooted outside the workspace.
    External,
    /// No workspace target proved, and no workspace function has the name.
    Unresolved,
}

pub(super) struct Index<'a> {
    graph: &'a Graph,
    pub symbols: BTreeMap<Vec<String>, Vec<usize>>,
    pub methods: BTreeMap<&'a str, Vec<usize>>,
    pub macros: BTreeMap<&'a str, Vec<usize>>,
    /// Names of the associated constants that workspace traits declare.
    trait_constants: BTreeSet<&'a str>,
}

impl<'a> Index<'a> {
    pub(super) fn new(graph: &'a Graph) -> Self {
        let mut index = Self {
            graph,
            symbols: BTreeMap::new(),
            methods: BTreeMap::new(),
            macros: BTreeMap::new(),
            trait_constants: BTreeSet::new(),
        };
        for (path, kind) in &graph.items {
            if let [owner @ .., name] = path.as_slice() {
                if *kind == Item::Value && graph.items.get(owner) == Some(&Item::Trait) {
                    index.trait_constants.insert(name);
                }
            }
        }
        for (at, function) in graph.functions.iter().enumerate() {
            if function.is_macro {
                index.macros.entry(&function.name).or_default().push(at);
            } else if let Some(owner) = &function.owner {
                index.methods.entry(&function.name).or_default().push(at);
                let mut path = owner.clone();
                path.push(function.name.clone());
                index.symbols.entry(path).or_default().push(at);
            } else {
                let mut path = function.scope.clone();
                path.push(function.name.clone());
                index.symbols.entry(path).or_default().push(at);
            }
        }
        index
    }

    fn external(&self, path: &[String]) -> bool {
        let Some(head) = path.first() else {
            return true;
        };
        path.len() < 2
            || BUILTIN_ROOTS.contains(&head.as_str())
            || self.graph.externals.contains(head)
    }

    fn error_variant(path: &[String]) -> bool {
        let [.., family, variant] = path else {
            return false;
        };
        ERROR_FAMILIES
            .iter()
            .any(|(name, variants)| name == family && variants.contains(&variant.as_str()))
    }

    pub(super) fn resolve(&self, caller: &Function, path: &[String]) -> Resolution {
        let mut forms = Vec::new();
        for candidate in symbol_candidates(path, caller) {
            let resolved = self.graph.resolved_paths(&candidate);
            let mut found: Vec<usize> = resolved
                .iter()
                .filter_map(|form| self.symbols.get(form))
                .flatten()
                .copied()
                .collect();
            if !found.is_empty() {
                found.sort_unstable();
                found.dedup();
                return Resolution::Functions(found);
            }
            forms.extend(resolved.into_iter().map(|form| (form, candidate.clone())));
        }
        if forms.iter().any(|(form, _)| {
            self.graph.items.contains_key(form)
                || self.graph.modules.contains(form)
                || Self::error_variant(form)
        }) || Self::error_variant(path)
        {
            return Resolution::NotCallable;
        }
        let Some(name) = path.last() else {
            return Resolution::External;
        };
        let external = self.external(path)
            || forms
                .iter()
                .any(|(form, candidate)| form != candidate && self.external(form));
        let prelude_trait = path.len() == 2 && PRELUDE_TRAITS.contains(&path[0].as_str());
        let owner = |kind: Item| {
            forms.iter().any(|(form, _)| {
                form.len() > 1 && self.graph.items.get(&form[..form.len() - 1]) == Some(&kind)
            })
        };
        let generic = path.len() > 1 && caller.generics.contains(&path[0]);
        let trait_call = prelude_trait || generic || owner(Item::Trait);
        if !trait_call && external {
            return Resolution::External;
        }
        let mut found: Vec<usize> = Vec::new();
        // A call through a trait, or a call on a workspace type that names no
        // inherent function of it, dispatches like a method call.
        if trait_call || owner(Item::Type) {
            found.extend(self.methods.get(name.as_str()).into_iter().flatten());
        }
        if found.is_empty() {
            for generator in self.graph.generated.get(name).into_iter().flatten() {
                found.extend(self.macros.get(generator.as_str()).into_iter().flatten());
            }
        }
        if found.is_empty() && generic && self.trait_constants.contains(name.as_str()) {
            return Resolution::NotCallable;
        }
        if found.is_empty() {
            return if external || prelude_trait {
                Resolution::External
            } else {
                Resolution::Unresolved
            };
        }
        found.sort_unstable();
        found.dedup();
        Resolution::Functions(found)
    }
}
