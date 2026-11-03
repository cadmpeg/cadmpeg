// SPDX-License-Identifier: Apache-2.0
//! Constructor-position `syn` scan (§5.2): carried-variant evidence.
//!
//! For a writer X the scan covers X's codec crate (plus `cadmpeg-asm` and
//! `cadmpeg-protein` for f3d). It records struct-literal paths, call
//! callees, and unit paths used as values — not as comparison operands —
//! resolving `Self::` and glob imports, mapping `XInput::V` to `X::V`, and
//! reading `macro_rules!` bodies as tokens. Test files and encode-path
//! functions are excluded: test items never load (`encode_paths::source`),
//! and functions reachable from an `EncoderBackend` method are dropped here.
//!
//! The scan resolves paths through the shared workspace graph, then
//! classifies them against the [`Registry`] of `cadmpeg-ir` enums:
//! `Enum::Variant` paths become carried variants, record-type paths become
//! carried arenas, and remaining callees resolve through the generated
//! constructor [`table`]. The checked-in artifacts under
//! `crates/cadmpeg-test-support/src/data/` are regenerated with
//! `UPDATE_CARRIED=1` and asserted fresh otherwise.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::encode_paths::{symbol_candidates, Call, CtorKind, Function, Graph};

const IR_PACKAGE: &str = "cadmpeg_ir";

/// How an enum's values appear in CADIR JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TagStyle {
    /// Externally tagged: `{"Variant": ...}` or bare `"Variant"`.
    External,
    /// Internally tagged: `{"kind": "variant", ...}`.
    Internal,
    /// Adjacently tagged: `{"tag": "variant", "content": ...}`.
    Adjacent,
    /// Untagged: the inner value stands alone; the wrapper is invisible.
    Untagged,
}

/// One `cadmpeg-ir` enum with its JSON spelling rules.
#[derive(Debug, Clone, PartialEq, Eq)]
struct EnumDef {
    style: TagStyle,
    /// Tag key for internally and adjacently tagged enums.
    tag: Option<String>,
    /// `rename_all` rule; Rust names are kept when absent.
    rename: Option<String>,
    /// Rust variant name to JSON spelling.
    variants: BTreeMap<String, String>,
}

/// The `cadmpeg-ir` enums a scan classifies paths against.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Registry {
    enums: BTreeMap<String, EnumDef>,
}

impl Registry {
    fn variant(&self, name: &str, variant: &str) -> Option<String> {
        self.enums.get(name).and_then(|definition| {
            definition
                .variants
                .contains_key(variant)
                .then(|| format!("{name}::{variant}"))
        })
    }
}

fn snake_case(name: &str) -> String {
    let mut out = String::new();
    for (index, ch) in name.char_indices() {
        if ch.is_uppercase() {
            if index != 0 {
                out.push('_');
            }
            out.extend(ch.to_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

/// Applies a serde `rename_all` rule to a Rust variant name.
fn apply_rename(rule: Option<&str>, variant: &str) -> String {
    match rule {
        None | Some("PascalCase") => variant.to_owned(),
        Some("lowercase") => variant.to_lowercase(),
        Some("UPPERCASE") => variant.to_uppercase(),
        Some("camelCase") => {
            let snake = snake_case(variant);
            let mut out = String::new();
            let mut upper = false;
            for part in snake.split('_') {
                if out.is_empty() {
                    out.push_str(part);
                } else if let Some(first) = part.chars().next() {
                    out.extend(first.to_uppercase());
                    out.push_str(&part[first.len_utf8()..]);
                    upper = true;
                }
            }
            let _ = upper;
            out
        }
        Some("snake_case") => snake_case(variant),
        Some("SCREAMING_SNAKE_CASE") => snake_case(variant).to_uppercase(),
        Some("kebab-case") => snake_case(variant).replace('_', "-"),
        Some("SCREAMING-KEBAB-CASE") => snake_case(variant).replace('_', "-").to_uppercase(),
        Some(other) => panic!("unsupported rename_all rule {other}"),
    }
}

fn test_gated(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if attr.path().is_ident("test") {
            return true;
        }
        if !attr.path().is_ident("cfg") {
            return false;
        }
        let mut gated = false;
        let _ = attr.parse_nested_meta(|meta| {
            gated |= meta.path.is_ident("test");
            Ok(())
        });
        gated
    })
}

fn serde_enum(attrs: &[syn::Attribute], item: &syn::ItemEnum) -> Option<(String, EnumDef)> {
    let mut style = TagStyle::External;
    let mut tag = None;
    let mut rename = None;
    for attr in attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        let result = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("untagged") {
                style = TagStyle::Untagged;
            } else if meta.path.is_ident("tag") {
                let value: syn::LitStr = meta.value()?.parse()?;
                tag = Some(value.value());
                if style == TagStyle::External {
                    style = TagStyle::Internal;
                }
            } else if meta.path.is_ident("content") {
                let _: syn::LitStr = meta.value()?.parse()?;
                style = TagStyle::Adjacent;
            } else if meta.path.is_ident("rename_all") {
                let value: syn::LitStr = meta.value()?.parse()?;
                rename = Some(value.value());
            }
            Ok(())
        });
        if result.is_err() {
            return None;
        }
    }
    let mut variants = BTreeMap::new();
    for variant in &item.variants {
        let name = variant.ident.to_string();
        let mut rename_variant = None;
        for attr in &variant.attrs {
            if !attr.path().is_ident("serde") {
                continue;
            }
            let _ = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("rename") {
                    let value: syn::LitStr = meta.value()?.parse()?;
                    rename_variant = Some(value.value());
                }
                Ok(())
            });
        }
        let spelling = rename_variant.unwrap_or_else(|| apply_rename(rename.as_deref(), &name));
        variants.insert(name, spelling);
    }
    Some((
        item.ident.to_string(),
        EnumDef {
            style,
            tag,
            rename,
            variants,
        },
    ))
}

fn collect_enums(items: &[syn::Item], registry: &mut Registry) {
    for item in items {
        match item {
            syn::Item::Enum(item) if !test_gated(&item.attrs) => {
                if let Some((name, definition)) = serde_enum(&item.attrs, item) {
                    registry.enums.insert(name, definition);
                }
            }
            syn::Item::Mod(item) if !test_gated(&item.attrs) => {
                if let Some((_, items)) = &item.content {
                    collect_enums(items, registry);
                }
            }
            _ => {}
        }
    }
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries =
        std::fs::read_dir(dir).unwrap_or_else(|_| panic!("{} is readable", dir.display()));
    let mut entries: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// Parses every `cadmpeg-ir` enum from source for path classification.
fn registry_from_source(root: &Path) -> Registry {
    let mut files = Vec::new();
    rust_files(&root.join("crates/cadmpeg-ir/src"), &mut files);
    let mut registry = Registry::default();
    for file in files {
        let text =
            std::fs::read_to_string(&file).unwrap_or_else(|_| panic!("{} reads", file.display()));
        let parsed = syn::parse_file(&text).unwrap_or_else(|_| panic!("{} parses", file.display()));
        collect_enums(&parsed.items, &mut registry);
    }
    registry
}

/// Maps record-type names to arena registry fields by reading the arena list
/// in `document.rs`: lines of the form `field: Type, model|design, "doc", …`.
fn record_type_arenas(root: &Path) -> BTreeMap<String, String> {
    let path = root.join("crates/cadmpeg-ir/src/document.rs");
    let text = std::fs::read_to_string(&path).expect("document.rs reads");
    let mut arenas = BTreeMap::new();
    for line in text.lines() {
        let Some((field, rest)) = line.trim().split_once(':') else {
            continue;
        };
        let field = field.trim();
        if field.is_empty()
            || !field
                .chars()
                .all(|ch| ch.is_lowercase() || ch == '_' || ch.is_numeric())
        {
            continue;
        }
        let mut parts = rest.split(',');
        let Some(ty) = parts
            .next()
            .map(str::trim)
            .map(|ty| ty.rsplit("::").next().unwrap_or(ty).trim().to_owned())
        else {
            continue;
        };
        if !ty.chars().next().is_some_and(|ch| ch.is_uppercase()) {
            continue;
        }
        if !parts
            .next()
            .is_some_and(|class| matches!(class.trim(), "model" | "design"))
        {
            continue;
        }
        arenas.insert(ty.to_owned(), field.to_owned());
    }
    arenas
}

/// `Enum::Variant` forms of one resolved path, with the `XInput` mapping.
fn variant_forms(path: &[String], registry: &Registry) -> Vec<String> {
    let mut found = Vec::new();
    if path.len() >= 2 {
        let (name, variant) = (&path[path.len() - 2], &path[path.len() - 1]);
        if let Some(hit) = registry.variant(name, variant) {
            found.push(hit);
        }
        if let Some(stripped) = name.strip_suffix("Input") {
            if let Some(hit) = registry.variant(stripped, variant) {
                found.push(hit);
            }
        }
    }
    found
}

/// The longest table key that is a suffix of `path`, preferring longer keys.
fn table_lookup<'a: 'k, 'k>(
    path: &[String],
    keys: &'k [(String, Vec<String>)],
    table: &'a BTreeMap<String, BTreeSet<String>>,
) -> impl Iterator<Item = &'a BTreeSet<String>> + 'k {
    let mut best = 0;
    for (_, parts) in keys {
        if parts.len() > best && parts.len() >= 2 && suffix_match(path, parts) {
            best = parts.len();
        }
    }
    keys.iter().filter_map(move |(name, parts)| {
        (parts.len() == best && best >= 2)
            .then(|| table.get(name))
            .flatten()
    })
}

/// Table keys split once, so repeated lookups compare segments, not strings.
fn split_keys(table: &BTreeMap<String, BTreeSet<String>>) -> Vec<(String, Vec<String>)> {
    table
        .keys()
        .map(|key| {
            (
                key.clone(),
                key.split("::").map(str::to_owned).collect::<Vec<_>>(),
            )
        })
        .collect()
}

fn suffix_match(path: &[String], key_parts: &[String]) -> bool {
    key_parts.len() <= path.len()
        && key_parts.last() == path.last()
        && path[path.len() - key_parts.len()..]
            .iter()
            .map(String::as_str)
            .eq(key_parts.iter().map(String::as_str))
}

/// Glob bases visible from `scope`: the glob imports of each ancestor module
/// in the same package. Function-local `use` items are not in the graph, so
/// this over-approximates with every same-package glob when none match.
fn glob_bases(graph: &Graph, scope: &[String]) -> Vec<Vec<String>> {
    let mut bases = Vec::new();
    for length in 1..=scope.len() {
        if let Some(targets) = graph.globs.get(&scope[..length]) {
            bases.extend(targets.iter().cloned());
        }
    }
    if bases.is_empty() {
        for (registered, targets) in &graph.globs {
            if registered.first() == scope.first() {
                bases.extend(targets.iter().cloned());
            }
        }
    }
    bases
}

/// Roots that never name an IR enum or record type: resolution cannot add
/// evidence for them, so the scan skips it.
const BARREN_ROOTS: &[&str] = &[
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
    "BTreeMap",
    "BTreeSet",
    "HashMap",
    "HashSet",
    "VecDeque",
    "LinkedList",
    "BinaryHeap",
    "Cow",
    "Rc",
    "Arc",
    "Cell",
    "RefCell",
    "Mutex",
    "RwLock",
    "PhantomData",
    "mem",
    "ptr",
    "slice",
    "iter",
    "fmt",
    "io",
    "fs",
    "env",
    "process",
    "time",
    "sync",
    "thread",
    "collections",
    "convert",
    "ops",
    "cmp",
    "hash",
    "any",
    "borrow",
    "clone",
    "default",
    "marker",
    "num",
    "str",
];

/// Cheap resolution-free forms: `Self::` through the owner, `crate::` and
/// `super::` through the scope. Classification only reads the last two
/// segments, which these forms already carry. Returns `None` when full
/// resolution is needed, and an empty vector when it cannot help.
fn cheap_forms(function: &Function, path: &[String]) -> Option<Vec<Vec<String>>> {
    if path.len() < 2 {
        // A bare name that the raw spelling missed is a glob-imported
        // variant the graph cannot resolve either; skip it.
        return Some(Vec::new());
    }
    match path.first().map(String::as_str) {
        Some("Self") => {
            let Some(owner) = &function.owner else {
                return Some(Vec::new());
            };
            let mut form = owner.clone();
            form.extend_from_slice(&path[1..]);
            Some(vec![form])
        }
        Some("crate") => {
            let mut form = function.scope[..1].to_vec();
            form.extend_from_slice(&path[1..]);
            Some(vec![form])
        }
        Some("super") => {
            let mut scope: Vec<String> = function
                .scope
                .iter()
                .take_while(|part| !part.starts_with('$'))
                .cloned()
                .collect();
            let mut rest = &path[..];
            while rest.first().is_some_and(|part| part == "super") {
                if scope.len() > 1 {
                    scope.pop();
                }
                rest = &rest[1..];
            }
            scope.extend_from_slice(rest);
            Some(vec![scope])
        }
        Some(root) if BARREN_ROOTS.contains(&root) => Some(Vec::new()),
        _ => None,
    }
}

/// Every resolution form of one constructor-position use: alias-resolved
/// paths plus same-package glob expansions of the raw path.
fn use_forms(graph: &Graph, function: &Function, path: &[String]) -> Vec<Vec<String>> {
    let mut forms = BTreeSet::new();
    for candidate in symbol_candidates(path, function) {
        forms.extend(graph.resolved_paths(&candidate));
    }
    for base in glob_bases(graph, &function.scope) {
        let mut expanded = base;
        expanded.extend_from_slice(path);
        forms.insert(expanded);
    }
    forms.into_iter().collect()
}

/// Classifies one resolved constructor-position path.
fn classify_use(
    path: &[String],
    kind: &CtorKind,
    registry: &Registry,
    record_types: &BTreeMap<String, String>,
    keys: &[(String, Vec<String>)],
    table: &BTreeMap<String, BTreeSet<String>>,
) -> (Vec<String>, Vec<String>) {
    let mut variants = variant_forms(path, registry);
    let mut arenas = Vec::new();
    if let Some(last) = path.last() {
        if let Some(arena) = record_types.get(last) {
            arenas.push(arena.clone());
        }
    }
    if *kind == CtorKind::Call {
        variants.extend(table_lookup(path, keys, table).flatten().cloned());
    }
    variants.sort();
    variants.dedup();
    (variants, arenas)
}

/// Memoized resolution forms of one raw path in one scope.
struct FormCache<'a> {
    graph: &'a Graph,
    cache: BTreeMap<(String, String), Vec<Vec<String>>>,
    cheap: BTreeMap<(String, String), Vec<Vec<String>>>,
}

impl<'a> FormCache<'a> {
    fn forms(&mut self, function: &Function, path: &[String]) -> &[Vec<String>] {
        if let Some(cheap) = cheap_forms(function, path) {
            self.cheap
                .insert((function.scope.join("::"), path.join("::")), cheap);
            let key = (function.scope.join("::"), path.join("::"));
            return &self.cheap[&key];
        }
        let key = (function.scope.join("::"), path.join("::"));
        if !self.cache.contains_key(&key) {
            let forms = use_forms(self.graph, function, path);
            self.cache.insert(key.clone(), forms);
        }
        &self.cache[&key]
    }
}

/// Classifies one raw constructor-position path without resolution, then
/// resolves only when the raw spelling says nothing. The raw spelling
/// already carries the last-two-segments evidence in the common case
/// (`Enum::Variant`, record-type struct paths); resolution covers `Self::`,
/// renamed imports and glob-imported names. A raw hit skips resolution, so
/// an alias chain that would add further variants beyond the spelling is
/// not followed (the observation side covers actual decodes).
fn classify_cached(
    cache: &mut FormCache<'_>,
    function: &Function,
    path: &[String],
    kind: &CtorKind,
    registry: &Registry,
    record_types: &BTreeMap<String, String>,
    keys: &[(String, Vec<String>)],
    table: &BTreeMap<String, BTreeSet<String>>,
) -> (BTreeSet<String>, BTreeSet<String>, Vec<String>) {
    let mut variants = BTreeSet::new();
    let mut arenas = BTreeSet::new();
    let mut callees = Vec::new();
    let (raw_variants, raw_arenas) = classify_use(path, kind, registry, record_types, keys, table);
    variants.extend(raw_variants);
    arenas.extend(raw_arenas);
    let raw_call_hit = *kind == CtorKind::Call
        && path.len() >= 2
        && table_lookup(path, keys, table).next().is_some();
    if !variants.is_empty() || !arenas.is_empty() || raw_call_hit {
        if *kind == CtorKind::Call {
            callees.push(path.join("::"));
        }
        return (variants, arenas, callees);
    }
    for resolved in cache.forms(function, path).to_vec() {
        let (more_variants, more_arenas) =
            classify_use(&resolved, kind, registry, record_types, keys, table);
        variants.extend(more_variants);
        arenas.extend(more_arenas);
        if *kind == CtorKind::Call {
            callees.push(resolved.join("::"));
        }
    }
    (variants, arenas, callees)
}

/// Cached resolution forms, with the cheap scope-local forms first.
fn function_key(scope: &[String], owner: &Option<Vec<String>>, name: &str) -> String {
    let mut key = owner.clone().unwrap_or_else(|| scope.to_vec());
    key.push(name.to_owned());
    key.join("::")
}

/// The variants each constructor of `package` builds, with fixpoint over
/// constructor-to-constructor calls. Keys are `scope::owner::name` paths.
fn constructor_table(
    graph: &Graph,
    package: &str,
    registry: &Registry,
    record_types: &BTreeMap<String, String>,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut direct: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut callees: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut cache = FormCache {
        graph,
        cache: BTreeMap::new(),
        cheap: BTreeMap::new(),
    };
    for function in &graph.functions {
        if function.scope.first().map(String::as_str) != Some(package) {
            continue;
        }
        let key = function_key(&function.scope, &function.owner, &function.name);
        for use_ in &function.ctor {
            let empty: Vec<(String, Vec<String>)> = Vec::new();
            let (found, _, targets) = classify_cached(
                &mut cache,
                function,
                &use_.path,
                &use_.kind,
                registry,
                record_types,
                &empty,
                &BTreeMap::new(),
            );
            direct.entry(key.clone()).or_default().extend(found);
            callees.entry(key.clone()).or_default().extend(targets);
        }
    }
    let mut table: BTreeMap<String, BTreeSet<String>> = direct
        .iter()
        .filter_map(|(key, variants)| {
            (!variants.is_empty()).then(|| (key.clone(), variants.clone()))
        })
        .collect();
    // Each (caller, target) pair meets each key once: new keys can only
    // lengthen the best suffix match, so matches refine to the fixpoint.
    // Resolved callees usually name their key exactly; the suffix scan is
    // the safety net for re-export drift.
    let mut matches: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    let mut scanned: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    for _ in 0..20 {
        let keys = split_keys(&table);
        let mut changed = false;
        for (key, targets) in &callees {
            let mut adds = BTreeSet::new();
            for target in targets {
                let parts: Vec<String> = target.split("::").map(str::to_owned).collect();
                let slot = matches.entry((key.clone(), target.clone())).or_default();
                let done = scanned.entry((key.clone(), target.clone())).or_default();
                let mut best = slot
                    .iter()
                    .map(|name| name.split("::").count())
                    .max()
                    .unwrap_or(0);
                for (name, key_parts) in &keys {
                    if !done.insert(name.clone()) {
                        continue;
                    }
                    if key_parts.len() >= best
                        && key_parts.len() <= parts.len()
                        && key_parts.last() == parts.last()
                        && parts[parts.len() - key_parts.len()..]
                            .iter()
                            .map(String::as_str)
                            .eq(key_parts.iter().map(String::as_str))
                    {
                        if key_parts.len() > best {
                            best = key_parts.len();
                            slot.clear();
                        }
                        if key_parts.len() == best {
                            slot.insert(name.clone());
                        }
                    }
                }
                for matched in slot.iter() {
                    if let Some(inherited) = table.get(matched) {
                        adds.extend(inherited.iter().cloned());
                    }
                }
            }
            let row = table.entry(key.clone()).or_default();
            let before = row.len();
            row.extend(adds);
            changed |= row.len() != before;
        }
        if !changed {
            break;
        }
    }
    table.retain(|_, variants| !variants.is_empty());
    table
}

/// Functions reachable from an `EncoderBackend` method through static
/// function paths and macro invocations only (§5.2 scan exclusion).
///
/// The shared call graph resolves method calls by name across the workspace:
/// applied to exclusion, that broadcast marks 96% of the STEP crate
/// encode-reachable (measured: 1015 of 1052 functions) and empties the scan.
/// Static-path reachability keeps genuinely shared helpers out while an
/// encode-path method reached only through dynamic dispatch may contribute
/// scan evidence; any finding it causes names its construction site.
fn narrow_reachable(graph: &Graph) -> BTreeSet<usize> {
    let mut symbols: BTreeMap<Vec<String>, Vec<usize>> = BTreeMap::new();
    let mut macros: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (at, function) in graph.functions.iter().enumerate() {
        if function.is_macro {
            macros.entry(function.name.as_str()).or_default().push(at);
        } else {
            let mut path = function
                .owner
                .clone()
                .unwrap_or_else(|| function.scope.clone());
            path.push(function.name.clone());
            symbols.entry(path).or_default().push(at);
        }
    }
    let mut roots = BTreeSet::new();
    for (at, function) in graph.functions.iter().enumerate() {
        let root = function.root
            || function.root_trait.as_ref().is_some_and(|path| {
                symbol_candidates(path, function)
                    .iter()
                    .flat_map(|candidate| graph.resolved_paths(candidate))
                    .any(|path| {
                        path.iter().map(String::as_str).eq([
                            "cadmpeg_ir",
                            "codec",
                            "write",
                            "EncoderBackend",
                        ])
                    })
            });
        if root {
            roots.insert(at);
        }
    }
    let mut reachable = roots;
    let mut pending: Vec<usize> = reachable.iter().copied().collect();
    while let Some(at) = pending.pop() {
        for call in &graph.functions[at].calls {
            match call {
                Call::Function(path) => {
                    for candidate in symbol_candidates(path, &graph.functions[at]) {
                        for form in graph.resolved_paths(&candidate) {
                            if let Some(found) = symbols.get(&form) {
                                for next in found {
                                    if reachable.insert(*next) {
                                        pending.push(*next);
                                    }
                                }
                            }
                        }
                    }
                }
                Call::Macro(name) => {
                    if let Some(found) = macros.get(name.as_str()) {
                        for next in found {
                            if reachable.insert(*next) {
                                pending.push(*next);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    reachable
}

/// Carried sets of one writer's scan.
#[derive(Debug, Default, PartialEq, Eq)]
struct CrateScan {
    variants: BTreeSet<String>,
    arenas: BTreeSet<String>,
}

/// Scans one codec crate plus `extra` scopes, excluding encode-path
/// functions. Macro bodies count only when included code invokes the macro.
fn crate_scan(
    graph: &Graph,
    package: &str,
    extra: &[&str],
    registry: &Registry,
    table: &BTreeMap<String, BTreeSet<String>>,
    record_types: &BTreeMap<String, String>,
) -> CrateScan {
    let reachable = narrow_reachable(graph);
    let mut packages = BTreeSet::from([package]);
    packages.extend(extra.iter().copied());
    let mut invoked: BTreeSet<String> = BTreeSet::new();
    for (index, function) in graph.functions.iter().enumerate() {
        if function.scope.first().map(String::as_str) != Some(package) || reachable.contains(&index)
        {
            continue;
        }
        for call in &function.calls {
            if let Call::Macro(name) = call {
                invoked.insert(name.clone());
            }
        }
    }
    let mut scan = CrateScan::default();
    let mut cache = FormCache {
        graph,
        cache: BTreeMap::new(),
        cheap: BTreeMap::new(),
    };
    let keys = split_keys(table);
    for (index, function) in graph.functions.iter().enumerate() {
        if !packages.contains(function.scope.first().map(String::as_str).unwrap_or("")) {
            continue;
        }
        if reachable.contains(&index) {
            continue;
        }
        if function.is_macro && !invoked.contains(&function.name) {
            continue;
        }
        for use_ in &function.ctor {
            let (found_variants, found_arenas, _) = classify_cached(
                &mut cache,
                function,
                &use_.path,
                &use_.kind,
                registry,
                record_types,
                &keys,
                table,
            );
            scan.variants.extend(found_variants);
            scan.arenas.extend(found_arenas);
        }
    }
    scan
}

fn artifacts_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../cadmpeg-test-support/src/data")
}

fn registry_json(registry: &Registry) -> String {
    let mut out = String::from("{\"enums\":{");
    for (index, (name, definition)) in registry.enums.iter().enumerate() {
        if index != 0 {
            out.push(',');
        }
        let style = match definition.style {
            TagStyle::External => "external",
            TagStyle::Internal => "internal",
            TagStyle::Adjacent => "adjacent",
            TagStyle::Untagged => "untagged",
        };
        out.push_str(&format!("\"{name}\":{{\"style\":\"{style}\""));
        if let Some(tag) = &definition.tag {
            out.push_str(&format!(",\"tag\":\"{tag}\""));
        }
        if let Some(rename) = &definition.rename {
            out.push_str(&format!(",\"rename_all\":\"{rename}\""));
        }
        out.push_str(",\"variants\":{");
        for (variant, (variant_name, spelling)) in definition.variants.iter().enumerate() {
            if variant != 0 {
                out.push(',');
            }
            out.push_str(&format!("\"{variant_name}\":\"{spelling}\""));
        }
        out.push_str("}}");
    }
    out.push_str("}}");
    out
}

fn table_json(table: &BTreeMap<String, BTreeSet<String>>) -> String {
    let mut out = String::from("{");
    for (index, (key, variants)) in table.iter().enumerate() {
        if index != 0 {
            out.push(',');
        }
        out.push_str(&format!("\"{key}\":["));
        for (variant, name) in variants.iter().enumerate() {
            if variant != 0 {
                out.push(',');
            }
            out.push_str(&format!("\"{name}\""));
        }
        out.push(']');
    }
    out.push('}');
    out
}

fn scan_json(scan: &CrateScan) -> String {
    let mut out = String::from("{\"variants\":[");
    for (index, variant) in scan.variants.iter().enumerate() {
        if index != 0 {
            out.push(',');
        }
        out.push_str(&format!("\"{variant}\""));
    }
    out.push_str("],\"arenas\":[");
    for (index, arena) in scan.arenas.iter().enumerate() {
        if index != 0 {
            out.push(',');
        }
        out.push_str(&format!("\"{arena}\""));
    }
    out.push_str("]}");
    out
}

fn write_or_check(name: &str, bytes: &str) {
    let path = artifacts_dir().join(name);
    if std::env::var("UPDATE_CARRIED").is_ok() {
        std::fs::create_dir_all(artifacts_dir()).expect("data dir is writable");
        std::fs::write(&path, bytes).unwrap_or_else(|_| panic!("{} writes", path.display()));
        return;
    }
    let current = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("{} reads; regenerate with UPDATE_CARRIED=1", path.display()));
    assert_eq!(
        current, bytes,
        "{name} is stale; regenerate with UPDATE_CARRIED=1"
    );
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_owned()
}

#[test]
fn carried_artifacts_are_fresh() {
    let root = workspace_root();
    let now = std::time::Instant::now();
    let registry = registry_from_source(&root);
    eprintln!(
        "registry: {:?} ({} enums)",
        now.elapsed(),
        registry.enums.len()
    );
    let record_types = record_type_arenas(&root);
    eprintln!(
        "record types: {:?} ({} arenas)",
        now.elapsed(),
        record_types.len()
    );
    let graph = super::encode_paths::source::workspace(&root);
    eprintln!("workspace: {:?}", now.elapsed());
    let table = constructor_table(&graph, IR_PACKAGE, &registry, &record_types);
    eprintln!("table: {:?} ({} rows)", now.elapsed(), table.len());
    write_or_check("variant_registry.json", &registry_json(&registry));
    write_or_check("constructor_table.json", &table_json(&table));
    let step = crate_scan(
        &graph,
        "cadmpeg_codec_step",
        &[],
        &registry,
        &table,
        &record_types,
    );
    write_or_check("scan_step.json", &scan_json(&step));
}

#[cfg(test)]
mod tests {
    use super::super::encode_paths::source::fixture;
    use super::*;

    fn test_registry() -> Registry {
        let mut registry = Registry::default();
        registry.enums.insert(
            "Surface".into(),
            EnumDef {
                style: TagStyle::External,
                tag: None,
                rename: None,
                variants: BTreeMap::from([
                    ("Cone".into(), "Cone".into()),
                    ("Plane".into(), "Plane".into()),
                ]),
            },
        );
        registry.enums.insert(
            "Sense".into(),
            EnumDef {
                style: TagStyle::External,
                tag: None,
                rename: Some("snake_case".into()),
                variants: BTreeMap::from([("Forward".into(), "forward".into())]),
            },
        );
        registry
    }

    fn test_records() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("Body".into(), "bodies".into()),
            ("Face".into(), "faces".into()),
        ])
    }

    fn scan_variants(source: &str) -> CrateScan {
        let graph = fixture(source);
        let registry = test_registry();
        let records = test_records();
        let table = constructor_table(&graph, "fixture", &registry, &records);
        crate_scan(&graph, "fixture", &[], &registry, &table, &records)
    }

    #[test]
    fn struct_literals_and_self_resolve_to_arenas_and_variants() {
        let scan = scan_variants(
            r#"
            use crate::Body;
            use crate::Surface;
            fn decode() -> Body { Body { id: 1 } }
            impl Surface {
                fn make() -> Self { Self::Plane }
            }
            "#,
        );
        assert!(scan.arenas.contains("bodies"), "{scan:?}");
        assert!(scan.variants.contains("Surface::Plane"), "{scan:?}");
    }

    #[test]
    fn call_callees_feed_the_table_with_fixpoint() {
        let graph = fixture(
            r#"
            use crate::Surface;
            fn helper() -> Surface { Surface::Cone }
            fn build() -> Surface { helper() }
            "#,
        );
        let registry = test_registry();
        let records = test_records();
        let table = constructor_table(&graph, "fixture", &registry, &records);
        let helper = table.keys().find(|key| key.ends_with("helper")).cloned();
        let build = table.keys().find(|key| key.ends_with("build")).cloned();
        assert_eq!(
            table.get(helper.as_ref().expect("helper row")),
            Some(&BTreeSet::from(["Surface::Cone".to_owned()])),
            "{table:?}"
        );
        assert_eq!(
            table.get(build.as_ref().expect("build row")),
            Some(&BTreeSet::from(["Surface::Cone".to_owned()])),
            "constructors inherit their callees' variants: {table:?}"
        );
    }

    #[test]
    fn unit_paths_as_values_count_but_comparison_operands_do_not() {
        let scan = scan_variants(
            r#"
            use crate::Sense;
            fn decode(value: Sense) -> Sense {
                let kept = Sense::Forward;
                let _ = value == Sense::Forward;
                let _ = value != Sense::Forward;
                let _ = matches!(value, Sense::Forward);
                let _ = 1;
                kept
            }
            "#,
        );
        assert!(scan.variants.contains("Sense::Forward"), "{scan:?}");
        let graph = fixture(
            r#"
            use crate::Sense;
            fn decode(value: Sense) -> bool { value == Sense::Forward }
            "#,
        );
        let registry = test_registry();
        let records = test_records();
        let table = constructor_table(&graph, "fixture", &registry, &records);
        let scan = crate_scan(&graph, "fixture", &[], &registry, &table, &records);
        assert!(!scan.variants.contains("Sense::Forward"), "{scan:?}");
    }

    #[test]
    fn assert_eq_operands_are_comparisons() {
        let scan = scan_variants(
            r#"
            use crate::Sense;
            fn decode(value: Sense) { assert_eq!(value, Sense::Forward); }
            "#,
        );
        assert!(!scan.variants.contains("Sense::Forward"), "{scan:?}");
    }

    #[test]
    fn test_items_are_excluded() {
        let scan = scan_variants(
            r#"
            use crate::Sense;
            #[test]
            fn unit() { let _ = Sense::Forward; }
            #[cfg(test)]
            mod tests { use crate::Sense; fn helper() { let _ = Sense::Forward; } }
            fn decode() {}
            "#,
        );
        assert!(!scan.variants.contains("Sense::Forward"), "{scan:?}");
    }

    #[test]
    fn encode_path_functions_are_excluded() {
        let scan = scan_variants(
            r#"
            use crate::Sense;
            impl EncoderBackend for Writer {
                fn plan(&self) { let _ = Sense::Forward; }
            }
            fn decode() { let _ = Sense::Forward; }
            "#,
        );
        assert!(scan.variants.contains("Sense::Forward"), "{scan:?}");
        let graph = fixture(
            r#"
            use crate::Sense;
            impl EncoderBackend for Writer {
                fn plan(&self) { helper(); }
            }
            fn helper() { let _ = Sense::Forward; }
            "#,
        );
        let registry = test_registry();
        let records = test_records();
        let table = constructor_table(&graph, "fixture", &registry, &records);
        let scan = crate_scan(&graph, "fixture", &[], &registry, &table, &records);
        assert!(
            !scan.variants.contains("Sense::Forward"),
            "helpers reachable from the encode path are excluded: {scan:?}"
        );
    }

    #[test]
    fn glob_imports_resolve() {
        let scan = scan_variants(
            r#"
            mod shapes { pub enum Surface { Plane } }
            use shapes::*;
            fn decode() { let _ = Surface::Plane; }
            "#,
        );
        assert!(scan.variants.contains("Surface::Plane"), "{scan:?}");
    }

    #[test]
    fn xinput_variants_map_to_their_ir_enum() {
        let scan = scan_variants(
            r#"
            mod input { pub enum SurfaceInput { Cone } }
            use input::SurfaceInput;
            fn decode() { let _ = SurfaceInput::Cone; }
            "#,
        );
        assert!(scan.variants.contains("Surface::Cone"), "{scan:?}");
    }

    #[test]
    fn macro_bodies_read_as_tokens_but_unused_macros_do_not_count() {
        let scan = scan_variants(
            r#"
            use crate::Sense;
            macro_rules! sense { () => { Sense::Forward }; }
            macro_rules! unused { () => { Sense::Forward }; }
            fn decode() -> Sense { sense!() }
            "#,
        );
        assert!(scan.variants.contains("Sense::Forward"), "{scan:?}");
        let graph = fixture(
            r#"
            use crate::Sense;
            macro_rules! unused { () => { Sense::Forward }; }
            fn decode() {}
            "#,
        );
        let registry = test_registry();
        let records = test_records();
        let table = constructor_table(&graph, "fixture", &registry, &records);
        let scan = crate_scan(&graph, "fixture", &[], &registry, &table, &records);
        assert!(
            !scan.variants.contains("Sense::Forward"),
            "never-invoked macro bodies do not count: {scan:?}"
        );
    }

    #[test]
    fn rename_rules_match_serde_spellings() {
        assert_eq!(apply_rename(Some("snake_case"), "Forward"), "forward");
        assert_eq!(
            apply_rename(Some("snake_case"), "SpatialSketchSelection"),
            "spatial_sketch_selection"
        );
        assert_eq!(
            apply_rename(Some("kebab-case"), "DatumPlane"),
            "datum-plane"
        );
        assert_eq!(
            apply_rename(Some("SCREAMING_SNAKE_CASE"), "DatumPlane"),
            "DATUM_PLANE"
        );
        assert_eq!(apply_rename(Some("camelCase"), "DatumPlane"), "datumPlane");
        assert_eq!(apply_rename(None, "Plane"), "Plane");
    }

    #[test]
    fn real_registry_names_tagged_variants() {
        let registry = registry_from_source(&workspace_root());
        let solved = registry
            .enums
            .get("SolvedSurfaceGeometry")
            .expect("IR enum");
        assert_eq!(solved.style, TagStyle::Internal);
        assert_eq!(solved.tag.as_deref(), Some("kind"));
        assert_eq!(
            solved.variants.get("Plane").map(String::as_str),
            Some("plane")
        );
        assert_eq!(
            solved.variants.get("Cone").map(String::as_str),
            Some("cone")
        );
        assert!(registry.enums.contains_key("Sense") || !registry.enums.is_empty());
    }

    #[test]
    fn real_record_types_name_model_arenas() {
        let arenas = record_type_arenas(&workspace_root());
        assert_eq!(arenas.get("Body").map(String::as_str), Some("bodies"));
        assert_eq!(arenas.get("Drawing").map(String::as_str), Some("drawings"));
        assert_eq!(
            arenas.get("Occurrence").map(String::as_str),
            Some("occurrences")
        );
        assert!(
            arenas.len() >= 48,
            "the full arena list parses: {}",
            arenas.len()
        );
    }
}
