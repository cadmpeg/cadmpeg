// SPDX-License-Identifier: Apache-2.0
//! Module and type resolution for the optional-key census.
use std::cell::OnceCell;
use std::collections::BTreeSet;
use std::path::Path;

use proc_macro2::{Delimiter, TokenStream, TokenTree};

use super::roots::{crate_of, RootShadows};
use super::{collect_rust_sources, is_test_module, is_test_path, parse_source};
use resolve::Lookups;

/// A token tree with delimiter boundaries retained.
///
/// A flattened token list cannot distinguish `Box::<T>::deserialize` from an
/// unrelated `Box::<T>` followed by a later `>::deserialize`. Keeping groups
/// also makes calls and attributes belong to the syntax item that owns them.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Node {
    Atom(String),
    Group(Delimiter, Vec<Node>),
}

fn nodes_from_stream(stream: &TokenStream) -> Vec<Node> {
    stream
        .clone()
        .into_iter()
        .map(|tree| match tree {
            TokenTree::Group(group) => {
                Node::Group(group.delimiter(), nodes_from_stream(&group.stream()))
            }
            other => Node::Atom(other.to_string()),
        })
        .collect()
}

fn atom(node: &Node) -> Option<&str> {
    match node {
        Node::Atom(value) => Some(value),
        Node::Group(_, _) => None,
    }
}

fn is_atom(nodes: &[Node], index: usize, value: &str) -> bool {
    nodes.get(index).and_then(atom) == Some(value)
}

fn is_path_atom(value: &str) -> bool {
    value == "Self"
        || value == "self"
        || value == "super"
        || value == "crate"
        || value.starts_with('$')
        || value
            .chars()
            .all(|character| character == '_' || character.is_ascii_alphanumeric())
}

/// One type declaration in a macro body.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct MacroType {
    path: String,
    scope: Vec<String>,
    name: String,
}

#[derive(Default)]
struct SourceIndex {
    macro_types: BTreeSet<MacroType>,
    imports: Vec<ImportBinding>,
    symbols: Vec<SymbolKey>,
    /// Where a crate root can name another item than its real crate.
    shadows: RootShadows,
    lookups: OnceCell<Lookups>,
}

impl SourceIndex {
    /// Indexes one parsed source file and the crate roots its tokens can bind.
    fn read_file(&mut self, items: &[syn::Item], path: &str, source: &str, scope: &[String]) {
        collect_source_items(items, path, scope, self);
        self.shadows.record_tokens(path, source);
    }

    /// The tables keyed for resolution, built at the first resolution.
    fn lookups(&self) -> &Lookups {
        let lookups = self.lookups.get_or_init(|| Lookups::new(self));
        assert!(
            lookups.indexes(self),
            "the source index changed after its first resolution"
        );
        lookups
    }

    /// Records every glob import whose target lies outside its own crate,
    /// which can bring in a module named by any crate root.
    fn record_foreign_globs(&mut self) {
        let foreign: Vec<(String, String)> = self
            .imports
            .iter()
            .filter(|import| import.glob)
            .filter(|import| {
                let first = import.target.segments.first().map_or("", String::as_str);
                import.target.absolute
                    || !(matches!(first, "crate" | "self" | "super")
                        || self.symbols.iter().any(|symbol| {
                            symbol.kind == SymbolKind::Module
                                && symbol.name == first
                                && crate_of(&symbol.path) == crate_of(&import.path)
                        }))
            })
            .map(|import| (import.path.clone(), import.target.segments.join("::")))
            .collect();
        for (path, target) in foreign {
            self.shadows.record_glob(&path, &target);
        }
    }
}

/// One lexical `use` binding. The target is retained as a syntax path so
/// aliases, re-exports, globs, and `self`/`super` can be resolved from the
/// scope in which the declaration appears.
#[derive(Debug, Clone)]
struct ImportBinding {
    path: String,
    scope: Vec<String>,
    alias: String,
    target: PathShape,
    glob: bool,
}

/// A type or module name declared in one source module. The census uses this
/// to distinguish a local `String`/`std`/`Box` from the standard-library name
/// with the same spelling.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum SymbolKind {
    Type,
    Module,
    Function,
    Value,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SymbolKey {
    path: String,
    scope: Vec<String>,
    name: String,
    kind: SymbolKind,
}

fn insert_macro_type(
    macro_types: &mut BTreeSet<MacroType>,
    path: &str,
    scope: &[String],
    name: String,
) {
    macro_types.insert(MacroType {
        path: path.to_owned(),
        scope: scope.to_owned(),
        name,
    });
}

fn insert_symbol(
    index: &mut SourceIndex,
    path: &str,
    scope: &[String],
    name: String,
    kind: SymbolKind,
) {
    index.symbols.push(SymbolKey {
        path: path.to_owned(),
        scope: scope.to_owned(),
        name,
        kind,
    });
}

/// Record every binding introduced by one `use` tree. The target remains
/// relative to the declaration's module so the resolver can follow aliases
/// and re-exports in their own lexical context.
fn collect_use_tree(
    tree: &syn::UseTree,
    prefix: &mut Vec<String>,
    absolute: bool,
    path: &str,
    scope: &[String],
    imports: &mut Vec<ImportBinding>,
) {
    match tree {
        syn::UseTree::Path(next) => {
            prefix.push(next.ident.to_string());
            collect_use_tree(next.tree.as_ref(), prefix, absolute, path, scope, imports);
            prefix.pop();
        }
        syn::UseTree::Name(name) => {
            let alias = name.ident.to_string();
            imports.push(ImportBinding {
                path: path.to_owned(),
                scope: scope.to_owned(),
                alias,
                target: PathShape {
                    segments: prefix
                        .iter()
                        .cloned()
                        .chain(std::iter::once(name.ident.to_string()))
                        .collect(),
                    absolute,
                },
                glob: false,
            });
        }
        syn::UseTree::Rename(rename) => {
            imports.push(ImportBinding {
                path: path.to_owned(),
                scope: scope.to_owned(),
                alias: rename.rename.to_string(),
                target: PathShape {
                    segments: prefix
                        .iter()
                        .cloned()
                        .chain(std::iter::once(rename.ident.to_string()))
                        .collect(),
                    absolute,
                },
                glob: false,
            });
        }
        syn::UseTree::Glob(_) => imports.push(ImportBinding {
            path: path.to_owned(),
            scope: scope.to_owned(),
            alias: String::new(),
            target: PathShape {
                segments: prefix.clone(),
                absolute,
            },
            glob: true,
        }),
        syn::UseTree::Group(group) => {
            for child in &group.items {
                collect_use_tree(child, prefix, absolute, path, scope, imports);
            }
        }
    }
}

fn macro_item_name(nodes: &[Node], index: usize) -> Option<(String, usize)> {
    if is_atom(nodes, index, "$") {
        let name = atom(nodes.get(index + 1)?)?;
        return is_path_atom(name).then(|| (format!("${name}"), index + 2));
    }
    let name = atom(nodes.get(index)?)?;
    is_path_atom(name).then(|| (name.to_owned(), index + 1))
}

/// Index types stated inside macro bodies.
fn collect_macro_types(
    nodes: &[Node],
    path: &str,
    scope: &[String],
    types: &mut BTreeSet<MacroType>,
) {
    let mut index = 0;
    while index < nodes.len() {
        if is_atom(nodes, index, "mod") {
            if let Some((module, after_name)) = macro_item_name(nodes, index + 1) {
                if let Some(Node::Group(Delimiter::Brace, children)) = nodes.get(after_name) {
                    let mut nested_scope = scope.to_owned();
                    nested_scope.push(module);
                    collect_macro_types(children, path, &nested_scope, types);
                    index = after_name + 1;
                    continue;
                }
            }
        }
        if is_atom(nodes, index, "struct") || is_atom(nodes, index, "enum") {
            if let Some((name, _)) = macro_item_name(nodes, index + 1) {
                insert_macro_type(types, path, scope, name);
            }
        }
        if let Some(Node::Group(_, children)) = nodes.get(index) {
            collect_macro_types(children, path, scope, types);
        }
        index += 1;
    }
}

fn collect_source_items(
    items: &[syn::Item],
    path: &str,
    scope: &[String],
    index: &mut SourceIndex,
) {
    for item in items {
        match item {
            syn::Item::ExternCrate(declaration) => {
                index.imports.push(ImportBinding {
                    path: path.to_owned(),
                    scope: scope.to_owned(),
                    alias: declaration.rename.as_ref().map_or_else(
                        || declaration.ident.to_string(),
                        |(_, alias)| alias.to_string(),
                    ),
                    target: PathShape {
                        segments: vec![declaration.ident.to_string()],
                        absolute: true,
                    },
                    glob: false,
                });
            }
            syn::Item::Use(use_item) => {
                let mut prefix = Vec::new();
                collect_use_tree(
                    &use_item.tree,
                    &mut prefix,
                    use_item.leading_colon.is_some(),
                    path,
                    scope,
                    &mut index.imports,
                );
            }
            syn::Item::Struct(declaration) => {
                insert_symbol(
                    index,
                    path,
                    scope,
                    declaration.ident.to_string(),
                    SymbolKind::Type,
                );
            }
            syn::Item::Enum(declaration) => {
                insert_symbol(
                    index,
                    path,
                    scope,
                    declaration.ident.to_string(),
                    SymbolKind::Type,
                );
            }
            syn::Item::Type(declaration) => {
                insert_symbol(
                    index,
                    path,
                    scope,
                    declaration.ident.to_string(),
                    SymbolKind::Type,
                );
            }
            syn::Item::Trait(declaration) => {
                insert_symbol(
                    index,
                    path,
                    scope,
                    declaration.ident.to_string(),
                    SymbolKind::Type,
                );
            }
            syn::Item::Union(declaration) => {
                insert_symbol(
                    index,
                    path,
                    scope,
                    declaration.ident.to_string(),
                    SymbolKind::Type,
                );
            }
            syn::Item::Const(declaration) => {
                insert_symbol(
                    index,
                    path,
                    scope,
                    declaration.ident.to_string(),
                    SymbolKind::Value,
                );
            }
            syn::Item::Static(declaration) => {
                insert_symbol(
                    index,
                    path,
                    scope,
                    declaration.ident.to_string(),
                    SymbolKind::Value,
                );
            }
            syn::Item::Fn(function) => {
                insert_symbol(
                    index,
                    path,
                    scope,
                    function.sig.ident.to_string(),
                    SymbolKind::Function,
                );
            }
            syn::Item::Mod(module) => {
                insert_symbol(
                    index,
                    path,
                    scope,
                    module.ident.to_string(),
                    SymbolKind::Module,
                );
                if is_test_module(module) {
                    continue;
                }
                if let Some((_, nested)) = &module.content {
                    let mut nested_scope = scope.to_owned();
                    nested_scope.push(module.ident.to_string());
                    collect_source_items(nested, path, &nested_scope, index);
                }
            }
            syn::Item::Macro(macro_item) => {
                let tokens = nodes_from_stream(&macro_item.mac.tokens);
                collect_macro_types(&tokens, path, scope, &mut index.macro_types);
            }
            _ => {}
        }
    }
}

/// Collect every implementation and scoped wire declaration in the three
/// source crates with one syntax traversal per file.
fn source_module_scope(source_root: &Path, file: &Path) -> Vec<String> {
    let relative = file
        .strip_prefix(source_root)
        .expect("a source file sits below its crate source root");
    let mut components: Vec<String> = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect();
    let file_name = components.pop().expect("a source path has a file name");
    let stem = file_name.strip_suffix(".rs").unwrap_or(&file_name);
    if stem != "lib" && stem != "main" && stem != "mod" {
        components.push(stem.to_owned());
    }
    components
}

fn source_index(root: &Path) -> SourceIndex {
    let mut index = SourceIndex::default();
    for source_root in [
        "crates/cadmpeg-ir/src",
        "crates/cadmpeg-core/src",
        "crates/cadmpeg-asm/src",
    ] {
        let mut files = Vec::new();
        collect_rust_sources(&root.join(source_root), &mut files)
            .unwrap_or_else(|error| panic!("cannot collect {source_root}: {error}"));
        files.sort();
        for file in files {
            let relative = file
                .strip_prefix(root)
                .expect("a collected source sits under the repository root")
                .to_string_lossy()
                .replace('\\', "/");
            if is_test_path(&relative) {
                continue;
            }
            let parsed_file = parse_source(&file, &relative);
            let (source, parsed) = (&parsed_file.0, &parsed_file.1);
            let scope = source_module_scope(&root.join(source_root), &file);
            index.read_file(&parsed.items, &relative, source, &scope);
        }
    }
    index.record_foreign_globs();
    index.shadows.record_manifests(
        root,
        &[
            "crates/cadmpeg-ir",
            "crates/cadmpeg-core",
            "crates/cadmpeg-asm",
        ],
    );
    index
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PathShape {
    segments: Vec<String>,
    absolute: bool,
}

/// The declarations of the three wire crates, resolved from a module by the
/// lexical resolution the reader routes use: imports, re-exports, globs and
/// `self`/`super`/`crate` paths are followed where each is written.
pub(super) struct DeclarationResolver {
    index: SourceIndex,
}

impl DeclarationResolver {
    pub(super) fn read(root: &Path) -> Self {
        Self {
            index: source_index(root),
        }
    }

    /// The type declaration `path` names from module `scope` of the file
    /// `file`, as `(file, module scope, name)`, or `None` when it names no
    /// one type declaration of the wire crates.
    pub(super) fn resolve(
        &self,
        file: &str,
        scope: &[String],
        path: &syn::Path,
    ) -> Option<(String, Vec<String>, String)> {
        match self.resolve_path(file, scope, path) {
            resolve::ResolvedPath::Local(symbol) if symbol.kind == SymbolKind::Type => {
                Some((symbol.path, symbol.scope, symbol.name))
            }
            resolve::ResolvedPath::Local(_)
            | resolve::ResolvedPath::External(_)
            | resolve::ResolvedPath::Unknown => None,
        }
    }

    /// Whether `path`, from module `scope` of the file `file`, names a
    /// standard or external path rather than a workspace declaration.
    pub(super) fn is_external(&self, file: &str, scope: &[String], path: &syn::Path) -> bool {
        self.external_path(file, scope, path).is_some()
    }

    /// The standard or external path `path` names from module `scope` of
    /// the file `file`, its root bound to its real crate there, or `None`.
    pub(super) fn external_path(
        &self,
        file: &str,
        scope: &[String],
        path: &syn::Path,
    ) -> Option<Vec<String>> {
        match self.resolve_path(file, scope, path) {
            resolve::ResolvedPath::External(segments) => Some(segments),
            resolve::ResolvedPath::Local(_) | resolve::ResolvedPath::Unknown => None,
        }
    }

    fn resolve_path(
        &self,
        file: &str,
        scope: &[String],
        path: &syn::Path,
    ) -> resolve::ResolvedPath {
        let shape = PathShape {
            segments: path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect(),
            absolute: path.leading_colon.is_some(),
        };
        resolve::resolve_path_from_context(file, scope, &shape, &self.index, &mut BTreeSet::new())
    }

    /// As [`Self::resolve`], and a path into another wire crate, spelled
    /// from that crate's name or through an explicit `use` of a path that
    /// is, is read from that crate's root.
    pub(super) fn resolve_across_crates(
        &self,
        file: &str,
        scope: &[String],
        path: &syn::Path,
    ) -> Option<(String, Vec<String>, String)> {
        self.resolve(file, scope, path)
            .or_else(|| self.resolve_in_wire_crate(file, scope, path, 0))
    }

    fn resolve_in_wire_crate(
        &self,
        file: &str,
        scope: &[String],
        path: &syn::Path,
        depth: usize,
    ) -> Option<(String, Vec<String>, String)> {
        let segments: Vec<String> = path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect();
        let (first, rest) = segments.split_first()?;
        if let Some(root) = wire_crate_root(first) {
            if self
                .index
                .shadows
                .failure(file, first, path.leading_colon.is_some())
                .is_some()
            {
                return None;
            }
            let rest: syn::Path = syn::parse_str(&format!("crate::{}", rest.join("::"))).ok()?;
            return self.resolve(&root, &[], &rest);
        }
        if depth > 8 || path.leading_colon.is_some() {
            return None;
        }
        let [position] = self.index.lookups().explicit(file, scope, first) else {
            return None;
        };
        let binding = &self.index.imports[*position];
        let target: Vec<String> = binding
            .target
            .segments
            .iter()
            .chain(rest)
            .cloned()
            .collect();
        let target: syn::Path = syn::parse_str(&target.join("::")).ok()?;
        self.resolve_in_wire_crate(file, scope, &target, depth + 1)
    }
}

/// The root source of the wire crate an extern crate name names.
fn wire_crate_root(name: &str) -> Option<String> {
    matches!(name, "cadmpeg_ir" | "cadmpeg_core" | "cadmpeg_asm")
        .then(|| format!("crates/{}/src/lib.rs", name.replace('_', "-")))
}

mod resolve;

#[cfg(test)]
mod tests {
    use super::{collect_macro_types, nodes_from_stream};
    use std::collections::BTreeSet;

    #[test]
    fn macro_type_resolution_does_not_depend_on_reader_attributes() {
        let tokens = "struct Wire { value: u8 }".parse().expect("type tokens");
        let mut types = BTreeSet::new();
        collect_macro_types(&nodes_from_stream(&tokens), "fixture.rs", &[], &mut types);
        assert!(types.iter().any(|item| item.name == "Wire"));
    }
}
