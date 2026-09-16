// SPDX-License-Identifier: Apache-2.0
//! Path resolution from a reader's own module context.

use super::*;

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

pub(super) fn primitive_name(name: &str) -> bool {
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

pub(super) fn external_root(name: &str) -> bool {
    matches!(
        name,
        "std" | "serde" | "serde_json" | "cadmpeg_core" | "core" | "alloc"
    )
}

pub(super) fn standard_prelude(name: &str) -> Option<Vec<String>> {
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

pub(super) fn dedup_symbols(symbols: impl IntoIterator<Item = SymbolKey>) -> Vec<SymbolKey> {
    let mut found = BTreeSet::new();
    symbols
        .into_iter()
        .filter(|symbol| found.insert(symbol.clone()))
        .collect()
}

pub(super) fn module_scope_for(symbol: &SymbolKey) -> Vec<String> {
    let mut scope = symbol.scope.clone();
    scope.push(symbol.name.clone());
    scope
}

pub(super) fn module_source_path(
    fallback: &str,
    module_scope: &[String],
    index: &SourceIndex,
) -> String {
    let mut candidates: Vec<(&String, usize)> = index
        .symbols
        .iter()
        .filter(|symbol| {
            same_crate(fallback, &symbol.path)
                && symbol.scope.starts_with(module_scope)
                && symbol.scope.len() >= module_scope.len()
        })
        .map(|symbol| (&symbol.path, symbol.scope.len()))
        .chain(
            index
                .imports
                .iter()
                .filter(|import| {
                    same_crate(fallback, &import.path)
                        && import.scope.starts_with(module_scope)
                        && import.scope.len() >= module_scope.len()
                })
                .map(|import| (&import.path, import.scope.len())),
        )
        .collect();
    candidates.sort_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(right.0)));
    candidates
        .into_iter()
        .next()
        .map_or_else(|| fallback.to_owned(), |(path, _)| path.clone())
}

pub(super) fn module_context_path(
    fallback: &str,
    module_scope: &[String],
    index: &SourceIndex,
) -> String {
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

pub(super) fn resolve_absolute_path(
    source_path: &str,
    segments: &[String],
    index: &SourceIndex,
    seen: &mut BTreeSet<String>,
) -> ResolvedPath {
    let Some(first) = segments.first() else {
        return ResolvedPath::Unknown;
    };
    if external_root(first) {
        return ResolvedPath::External(segments.to_owned());
    }
    // A leading `::` starts at the crate or extern prelude root. Lexical
    // imports in the current source module must not rewrite it: consulting
    // them would let an unrelated alias certify an absolute path.
    let root_path = module_context_path(source_path, &[], index);
    let root_symbols = dedup_symbols(
        index
            .symbols
            .iter()
            .filter(|symbol| {
                same_crate(&root_path, &symbol.path)
                    && symbol.scope.is_empty()
                    && symbol.name == *first
            })
            .cloned(),
    );
    let Some(symbol) = root_symbols.into_iter().next() else {
        return ResolvedPath::Unknown;
    };
    if segments.len() == 1 {
        return ResolvedPath::Local(symbol);
    }
    if symbol.kind != SymbolKind::Module {
        return ResolvedPath::Unknown;
    }
    let nested_scope = module_scope_for(&symbol);
    let nested_path = module_source_path(&root_path, &nested_scope, index);
    resolve_from_module(&nested_path, &nested_scope, &segments[1..], index, seen)
}

pub(super) fn resolve_from_module(
    source_path: &str,
    module_scope: &[String],
    segments: &[String],
    index: &SourceIndex,
    seen: &mut BTreeSet<String>,
) -> ResolvedPath {
    let Some(first) = segments.first() else {
        return ResolvedPath::Unknown;
    };
    let explicit: Vec<&ImportBinding> = index
        .imports
        .iter()
        .filter(|import| {
            import.path == source_path
                && import.scope == module_scope
                && !import.glob
                && import.alias == *first
        })
        .collect();
    if explicit.len() > 1 {
        return ResolvedPath::Unknown;
    }
    if let Some(import) = explicit.first() {
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

    let local: Vec<SymbolKey> = dedup_symbols(
        index
            .symbols
            .iter()
            .filter(|symbol| same_crate(source_path, &symbol.path) && symbol.scope == module_scope)
            .filter(|symbol| symbol.name == *first)
            .cloned(),
    );
    if local.len() > 1 {
        return ResolvedPath::Unknown;
    }
    if let Some(symbol) = local.into_iter().next() {
        if segments.len() == 1 {
            return ResolvedPath::Local(symbol);
        }
        if symbol.kind != SymbolKind::Module {
            return ResolvedPath::Unknown;
        }
        let nested_scope = module_scope_for(&symbol);
        let nested_path = module_source_path(source_path, &nested_scope, index);
        return resolve_from_module(&nested_path, &nested_scope, &segments[1..], index, seen);
    }

    let denied: Vec<SymbolKey> = dedup_symbols(
        index
            .denied
            .iter()
            .filter(|wire| same_crate(source_path, &wire.path) && wire.scope == module_scope)
            .filter(|wire| wire.name == *first)
            .map(|wire| SymbolKey {
                path: wire.path.clone(),
                scope: wire.scope.clone(),
                name: wire.name.clone(),
                kind: SymbolKind::Type,
            }),
    );
    if denied.len() > 1 {
        return ResolvedPath::Unknown;
    }
    if let Some(symbol) = denied.into_iter().next() {
        if segments.len() == 1 {
            return ResolvedPath::Local(symbol);
        }
        return ResolvedPath::Unknown;
    }

    let globs: Vec<&ImportBinding> = index
        .imports
        .iter()
        .filter(|import| import.path == source_path && import.scope == module_scope && import.glob)
        .collect();
    let mut candidates = Vec::new();
    for import in globs {
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
        return ResolvedPath::External(segments.to_owned());
    }
    if !module_scope.is_empty() {
        // A nested module can use an item from its parent without a `use`.
        let parent_scope = &module_scope[..module_scope.len() - 1];
        let parent_path = module_context_path(source_path, parent_scope, index);
        return resolve_from_module(&parent_path, parent_scope, segments, index, seen);
    }
    if segments.len() == 1 {
        if external_root(first) {
            return ResolvedPath::External(vec![first.clone()]);
        }
        if let Some(path) = standard_prelude(first) {
            return ResolvedPath::External(path);
        }
    }
    ResolvedPath::Unknown
}

pub(super) fn append_resolved(
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

pub(super) fn resolve_receiver_path(
    route: &HandImplSource,
    path: &PathShape,
    index: &SourceIndex,
) -> ResolvedPath {
    resolve_path_from_context(&route.path, &route.scope, path, index, &mut BTreeSet::new())
}

pub(super) fn external_is_keyless(path: &[String]) -> bool {
    fn matches(path: &[String], expected: &[&str]) -> bool {
        path.len() == expected.len()
            && path
                .iter()
                .zip(expected)
                .all(|(actual, wanted)| actual == wanted)
    }
    (path.len() == 2
        && path.first().is_some_and(|segment| segment == "primitive")
        && path.get(1).is_some_and(|segment| primitive_name(segment)))
        || matches(path, &["std", "string", "String"])
        || matches(path, &["std", "vec", "Vec"])
        || matches(path, &["std", "collections", "VecDeque"])
        || matches(path, &["std", "collections", "LinkedList"])
        || matches(path, &["std", "collections", "BinaryHeap"])
        || matches(path, &["std", "collections", "HashSet"])
        || matches(path, &["std", "collections", "BTreeSet"])
        || matches(path, &["std", "boxed", "Box"])
        || matches(path, &["core", "option", "Option"])
        || matches(path, &["std", "rc", "Rc"])
        || matches(path, &["std", "sync", "Arc"])
        || matches(path, &["core", "pin", "Pin"])
        || matches(path, &["std", "cell", "RefCell"])
        || matches(path, &["std", "cell", "Cell"])
        || matches(path, &["std", "sync", "Mutex"])
        || matches(path, &["std", "sync", "RwLock"])
        || matches(path, &["serde_bytes", "ByteBuf"])
}

pub(super) fn external_is_value(path: &[String]) -> bool {
    path.len() == 2 && path[0] == "serde_json" && path[1] == "Value"
}
