// SPDX-License-Identifier: Apache-2.0
//! Path resolution from a reader's own module context.

use std::collections::{BTreeMap, BTreeSet};

use super::{PathShape, SourceIndex, SymbolKey, SymbolKind};
use crate::roots::crate_of;

/// A file or crate, a module scope in it, and a name.
type ScopedName = (String, Vec<String>, String);

/// The source index keyed the way resolution reads it.
pub(super) struct Lookups {
    /// The symbol, import, and macro-type counts the tables were built from.
    sizes: (usize, usize, usize),
    /// Positions of non-glob imports, by file, scope, and alias.
    explicit: BTreeMap<ScopedName, Vec<usize>>,
    /// Positions of glob imports, by file and scope.
    globs: BTreeMap<(String, Vec<String>), Vec<usize>>,
    /// Distinct symbols, by crate, scope, and name.
    symbols: BTreeMap<ScopedName, Vec<SymbolKey>>,
    /// Macro declarations as type symbols, by crate, scope, and name.
    macro_types: BTreeMap<ScopedName, Vec<SymbolKey>>,
    /// The least (scope length, file) of the symbols and imports under each
    /// crate and scope prefix.
    module_sources: BTreeMap<(String, Vec<String>), (usize, String)>,
}

impl Lookups {
    pub(super) fn new(index: &SourceIndex) -> Self {
        let mut lookups = Self {
            sizes: Self::sizes_of(index),
            explicit: BTreeMap::new(),
            globs: BTreeMap::new(),
            symbols: BTreeMap::new(),
            macro_types: BTreeMap::new(),
            module_sources: BTreeMap::new(),
        };
        for (position, import) in index.imports.iter().enumerate() {
            if import.glob {
                lookups
                    .globs
                    .entry((import.path.clone(), import.scope.clone()))
                    .or_default()
                    .push(position);
            } else {
                lookups
                    .explicit
                    .entry((
                        import.path.clone(),
                        import.scope.clone(),
                        import.alias.clone(),
                    ))
                    .or_default()
                    .push(position);
            }
            lookups.record_module_source(&import.path, &import.scope);
        }
        for symbol in &index.symbols {
            let found = lookups
                .symbols
                .entry((
                    crate_of(&symbol.path),
                    symbol.scope.clone(),
                    symbol.name.clone(),
                ))
                .or_default();
            if !found.contains(symbol) {
                found.push(symbol.clone());
            }
            lookups.record_module_source(&symbol.path, &symbol.scope);
        }
        for declaration in &index.macro_types {
            lookups
                .macro_types
                .entry((
                    crate_of(&declaration.path),
                    declaration.scope.clone(),
                    declaration.name.clone(),
                ))
                .or_default()
                .push(SymbolKey {
                    path: declaration.path.clone(),
                    scope: declaration.scope.clone(),
                    name: declaration.name.clone(),
                    kind: SymbolKind::Type,
                });
        }
        lookups
    }

    fn sizes_of(index: &SourceIndex) -> (usize, usize, usize) {
        (
            index.symbols.len(),
            index.imports.len(),
            index.macro_types.len(),
        )
    }

    /// Whether these tables were built from `index` as it stands.
    pub(super) fn indexes(&self, index: &SourceIndex) -> bool {
        self.sizes == Self::sizes_of(index)
    }

    fn record_module_source(&mut self, path: &str, scope: &[String]) {
        let owner = crate_of(path);
        for depth in 0..=scope.len() {
            let least = self
                .module_sources
                .entry((owner.clone(), scope[..depth].to_vec()))
                .or_insert_with(|| (scope.len(), path.to_owned()));
            if (scope.len(), path) < (least.0, least.1.as_str()) {
                *least = (scope.len(), path.to_owned());
            }
        }
    }

    /// Positions of the non-glob imports binding `alias` in module `scope` of
    /// the file `path`.
    pub(super) fn explicit(&self, path: &str, scope: &[String], alias: &str) -> &[usize] {
        self.explicit
            .get(&(path.to_owned(), scope.to_vec(), alias.to_owned()))
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    fn globs(&self, path: &str, scope: &[String]) -> &[usize] {
        self.globs
            .get(&(path.to_owned(), scope.to_vec()))
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    /// The distinct symbols named `name` in module `scope` of the crate of `path`.
    fn symbols(&self, path: &str, scope: &[String], name: &str) -> &[SymbolKey] {
        self.symbols
            .get(&(crate_of(path), scope.to_vec(), name.to_owned()))
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    fn macro_types(&self, path: &str, scope: &[String], name: &str) -> &[SymbolKey] {
        self.macro_types
            .get(&(crate_of(path), scope.to_vec(), name.to_owned()))
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    fn module_source(&self, path: &str, scope: &[String]) -> Option<&str> {
        self.module_sources
            .get(&(crate_of(path), scope.to_vec()))
            .map(|(_, source)| source.as_str())
    }
}

#[derive(Debug, Clone)]
pub(super) enum ResolvedPath {
    /// A standard or external path. Only the exact canonical paths below are
    /// granted a keyless/value contract; an arbitrary qualified basename is
    /// never enough.
    External(Vec<String>),
    /// A declaration in one of the scanned source modules.
    Local(SymbolKey),
    /// The syntax did not resolve uniquely in the lexical source index.
    Unknown,
}

fn primitive_name(name: &str) -> bool {
    matches!(
        name,
        "bool"
            | "char"
            | "str"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "isize"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "usize"
            | "f32"
            | "f64"
    )
}

fn external_root(name: &str) -> bool {
    matches!(
        name,
        "std" | "serde" | "serde_json" | "cadmpeg_core" | "core" | "alloc"
    )
}

/// `segments` as an external path when its root binds its real crate from
/// `source_path`; a root something else can bind there resolves to nothing.
fn external(
    source_path: &str,
    segments: &[String],
    absolute: bool,
    index: &SourceIndex,
) -> ResolvedPath {
    match segments.first() {
        Some(root) if index.shadows.failure(source_path, root, absolute).is_some() => {
            ResolvedPath::Unknown
        }
        _ => ResolvedPath::External(segments.to_owned()),
    }
}

fn standard_prelude(name: &str) -> Option<Vec<String>> {
    if primitive_name(name) {
        return Some(vec!["primitive".to_owned(), name.to_owned()]);
    }
    let path = match name {
        "String" => ["std", "string", "String"].as_slice(),
        "Vec" => ["std", "vec", "Vec"].as_slice(),
        "VecDeque" => ["std", "collections", "VecDeque"].as_slice(),
        "LinkedList" => ["std", "collections", "LinkedList"].as_slice(),
        "BinaryHeap" => ["std", "collections", "BinaryHeap"].as_slice(),
        "HashSet" => ["std", "collections", "HashSet"].as_slice(),
        "BTreeSet" => ["std", "collections", "BTreeSet"].as_slice(),
        "Box" => ["std", "boxed", "Box"].as_slice(),
        "Option" => ["core", "option", "Option"].as_slice(),
        "Rc" => ["std", "rc", "Rc"].as_slice(),
        "Arc" => ["std", "sync", "Arc"].as_slice(),
        "Pin" => ["core", "pin", "Pin"].as_slice(),
        "RefCell" => ["std", "cell", "RefCell"].as_slice(),
        "Cell" => ["std", "cell", "Cell"].as_slice(),
        "Mutex" => ["std", "sync", "Mutex"].as_slice(),
        "RwLock" => ["std", "sync", "RwLock"].as_slice(),
        "ByteBuf" => ["serde_bytes", "ByteBuf"].as_slice(),
        _ => return None,
    };
    Some(path.iter().map(|segment| (*segment).to_owned()).collect())
}

fn module_scope_for(symbol: &SymbolKey) -> Vec<String> {
    let mut scope = symbol.scope.clone();
    scope.push(symbol.name.clone());
    scope
}

fn module_source_path(fallback: &str, module_scope: &[String], index: &SourceIndex) -> String {
    index
        .lookups()
        .module_source(fallback, module_scope)
        .unwrap_or(fallback)
        .to_owned()
}

fn module_context_path(fallback: &str, module_scope: &[String], index: &SourceIndex) -> String {
    if module_scope.is_empty() {
        let mut components: Vec<&str> = fallback.split('/').collect();
        if let Some(source_index) = components.iter().position(|component| *component == "src") {
            components.truncate(source_index + 1);
            components.push("lib.rs");
            return components.join("/");
        }
    }
    module_source_path(fallback, module_scope, index)
}

pub(super) fn resolve_path_from_context(
    source_path: &str,
    context_scope: &[String],
    path: &PathShape,
    index: &SourceIndex,
    seen: &mut BTreeSet<String>,
) -> ResolvedPath {
    let Some(first) = path.segments.first() else {
        return ResolvedPath::Unknown;
    };
    if path.absolute {
        return resolve_absolute_path(source_path, &path.segments, index, seen);
    }
    if first == "crate" {
        let root_path = module_context_path(source_path, &[], index);
        return resolve_from_module(&root_path, &[], &path.segments[1..], index, seen);
    }
    if first == "self" || first == "super" {
        let mut scope = context_scope.to_vec();
        let mut offset = 0;
        while path
            .segments
            .get(offset)
            .is_some_and(|segment| segment == "super")
        {
            if scope.pop().is_none() {
                return ResolvedPath::Unknown;
            }
            offset += 1;
        }
        if path
            .segments
            .get(offset)
            .is_some_and(|segment| segment == "self")
        {
            offset += 1;
        }
        let context_path = module_context_path(source_path, &scope, index);
        return resolve_from_module(&context_path, &scope, &path.segments[offset..], index, seen);
    }
    resolve_from_module(source_path, context_scope, &path.segments, index, seen)
}

fn resolve_absolute_path(
    source_path: &str,
    segments: &[String],
    index: &SourceIndex,
    seen: &mut BTreeSet<String>,
) -> ResolvedPath {
    let Some(first) = segments.first() else {
        return ResolvedPath::Unknown;
    };
    if external_root(first) {
        return external(source_path, segments, true, index);
    }
    // A leading `::` starts at the crate or extern prelude root. Lexical
    // imports in the current source module must not rewrite it: consulting
    // them would let an unrelated alias certify an absolute path.
    let root_path = module_context_path(source_path, &[], index);
    let Some(symbol) = index.lookups().symbols(&root_path, &[], first).first() else {
        return ResolvedPath::Unknown;
    };
    if segments.len() == 1 {
        return ResolvedPath::Local(symbol.clone());
    }
    if symbol.kind != SymbolKind::Module {
        return ResolvedPath::Unknown;
    }
    let nested_scope = module_scope_for(symbol);
    let nested_path = module_source_path(&root_path, &nested_scope, index);
    resolve_from_module(&nested_path, &nested_scope, &segments[1..], index, seen)
}

fn resolve_from_module(
    source_path: &str,
    module_scope: &[String],
    segments: &[String],
    index: &SourceIndex,
    seen: &mut BTreeSet<String>,
) -> ResolvedPath {
    let Some(first) = segments.first() else {
        return ResolvedPath::Unknown;
    };
    let lookups = index.lookups();
    let explicit = lookups.explicit(source_path, module_scope, first);
    if explicit.len() > 1 {
        return ResolvedPath::Unknown;
    }
    if let Some(position) = explicit.first() {
        let import = &index.imports[*position];
        let key = format!("{}::{:?}::{}", import.path, import.scope, import.alias);
        if !seen.insert(key) {
            return ResolvedPath::Unknown;
        }
        let resolved =
            resolve_path_from_context(&import.path, &import.scope, &import.target, index, seen);
        seen.remove(&format!(
            "{}::{:?}::{}",
            import.path, import.scope, import.alias
        ));
        return append_resolved(resolved, &segments[1..], source_path, index, seen);
    }

    let local = lookups.symbols(source_path, module_scope, first);
    if local.len() > 1 {
        return ResolvedPath::Unknown;
    }
    if let Some(symbol) = local.first() {
        if segments.len() == 1 {
            return ResolvedPath::Local(symbol.clone());
        }
        if symbol.kind != SymbolKind::Module {
            return ResolvedPath::Unknown;
        }
        let nested_scope = module_scope_for(symbol);
        let nested_path = module_source_path(source_path, &nested_scope, index);
        return resolve_from_module(&nested_path, &nested_scope, &segments[1..], index, seen);
    }

    let macro_types = lookups.macro_types(source_path, module_scope, first);
    if macro_types.len() > 1 {
        return ResolvedPath::Unknown;
    }
    if let Some(symbol) = macro_types.first() {
        if segments.len() == 1 {
            return ResolvedPath::Local(symbol.clone());
        }
        return ResolvedPath::Unknown;
    }

    let mut candidates = Vec::new();
    for position in lookups.globs(source_path, module_scope) {
        let import = &index.imports[*position];
        let key = format!(
            "{}::{:?}::*::{:?}",
            import.path, import.scope, import.target
        );
        if !seen.insert(key.clone()) {
            continue;
        }
        let target =
            resolve_path_from_context(&import.path, &import.scope, &import.target, index, seen);
        let candidate = match target {
            ResolvedPath::Local(symbol) if symbol.kind == SymbolKind::Module => {
                let nested_scope = module_scope_for(&symbol);
                let nested_path = module_source_path(&symbol.path, &nested_scope, index);
                resolve_from_module(&nested_path, &nested_scope, segments, index, seen)
            }
            ResolvedPath::Local(_) => ResolvedPath::Unknown,
            ResolvedPath::External(prefix) => {
                ResolvedPath::External(prefix.into_iter().chain(segments.iter().cloned()).collect())
            }
            ResolvedPath::Unknown => ResolvedPath::Unknown,
        };
        seen.remove(&key);
        if !matches!(candidate, ResolvedPath::Unknown) {
            candidates.push(candidate);
        }
    }
    if candidates.len() == 1 {
        return candidates.remove(0);
    }
    if external_root(first) {
        return external(source_path, segments, false, index);
    }
    if !module_scope.is_empty() {
        // A nested module can use an item from its parent without a `use`.
        let parent_scope = &module_scope[..module_scope.len() - 1];
        let parent_path = module_context_path(source_path, parent_scope, index);
        return resolve_from_module(&parent_path, parent_scope, segments, index, seen);
    }
    if segments.len() == 1 {
        if external_root(first) {
            return external(source_path, segments, false, index);
        }
        if let Some(path) = standard_prelude(first) {
            return ResolvedPath::External(path);
        }
    }
    ResolvedPath::Unknown
}

fn append_resolved(
    resolved: ResolvedPath,
    suffix: &[String],
    source_path: &str,
    index: &SourceIndex,
    seen: &mut BTreeSet<String>,
) -> ResolvedPath {
    if suffix.is_empty() {
        return resolved;
    }
    match resolved {
        ResolvedPath::External(prefix) => {
            ResolvedPath::External(prefix.into_iter().chain(suffix.iter().cloned()).collect())
        }
        ResolvedPath::Local(symbol) => {
            if symbol.kind != SymbolKind::Module {
                return ResolvedPath::Unknown;
            }
            let nested_scope = module_scope_for(&symbol);
            let nested_path = module_source_path(source_path, &nested_scope, index);
            resolve_from_module(&nested_path, &nested_scope, suffix, index, seen)
        }
        ResolvedPath::Unknown => ResolvedPath::Unknown,
    }
}
