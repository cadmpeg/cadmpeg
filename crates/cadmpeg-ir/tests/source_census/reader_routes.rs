// SPDX-License-Identifier: Apache-2.0
/// Semantic route checks for hand-written source readers.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use proc_macro2::{Delimiter, TokenStream, TokenTree};
use syn::spanned::Spanned;

use super::{
    collect_rust_sources, deserialize_impl_target, is_test_module, is_test_path, skip_meta_value,
};

/// One route shape a hand-written reader is allowed to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum HandReaderClass {
    /// A closed object adapter whose wire declaration refuses unknown keys.
    Wire,
    /// A scalar, byte string, fixed array, or sequence adapter.
    Keyless,
    /// An intentionally open map or canonical JSON value adapter.
    FreeForm,
    /// A general JSON value adapter with an explicit version gate.
    ValidatedValue,
}

impl HandReaderClass {
    /// The spelling used by the source census table and its diagnostics.
    const fn as_str(self) -> &'static str {
        match self {
            Self::Wire => "wire",
            Self::Keyless => "keyless",
            Self::FreeForm => "free-form",
            Self::ValidatedValue => "validated-value",
        }
    }
}

/// A token tree with delimiter boundaries retained.
///
/// A flattened token list cannot distinguish `Box::<T>::deserialize` from an
/// unrelated `Box::<T>` followed by a later `>::deserialize`. Keeping groups
/// also makes calls and attributes belong to the syntax item that owns them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Node {
    Atom(String),
    Group(Delimiter, Vec<Node>),
}

pub(super) fn nodes_from_stream(stream: &TokenStream) -> Vec<Node> {
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

pub(super) fn atom(node: &Node) -> Option<&str> {
    match node {
        Node::Atom(value) => Some(value),
        Node::Group(_, _) => None,
    }
}

pub(super) fn atom_opt(node: Option<&Node>) -> Option<&str> {
    node.and_then(atom)
}

pub(super) fn is_atom(nodes: &[Node], index: usize, value: &str) -> bool {
    nodes.get(index).and_then(atom) == Some(value)
}

pub(super) fn is_path_atom(value: &str) -> bool {
    value == "Self"
        || value == "self"
        || value == "super"
        || value == "crate"
        || value.starts_with('$')
        || value
            .chars()
            .all(|character| character == '_' || character.is_ascii_alphanumeric())
}

/// Render a token tree for a useful unresolved-route diagnostic.
pub(super) fn node_text(node: &Node) -> String {
    match node {
        Node::Atom(value) => value.clone(),
        Node::Group(delimiter, children) => {
            let (open, close) = match delimiter {
                Delimiter::Parenthesis => ('(', ')'),
                Delimiter::Brace => ('{', '}'),
                Delimiter::Bracket => ('[', ']'),
                Delimiter::None => (' ', ' '),
            };
            let mut text = String::new();
            if open != ' ' {
                text.push(open);
            }
            for child in children {
                text.push_str(&node_text(child));
            }
            if close != ' ' {
                text.push(close);
            }
            text
        }
    }
}

pub(super) fn nodes_text(nodes: &[Node]) -> String {
    nodes.iter().map(node_text).collect()
}

/// Return the byte range covered by a proc-macro span.
///
/// `Span::line` and `Span::column` are byte positions. Slicing by complete
/// lines was previously used here; two adjacent items on one line then shared
/// the same source text and could contaminate one another's route.
pub(super) fn source_span_range(source: &str, span: proc_macro2::Span) -> Option<(usize, usize)> {
    let mut line_starts = vec![0];
    for (index, byte) in source.bytes().enumerate() {
        if byte == b'\n' {
            line_starts.push(index + 1);
        }
    }
    let start = span.start();
    let end = span.end();
    let start_line = line_starts.get(start.line.checked_sub(1)?)?;
    let end_line = line_starts.get(end.line.checked_sub(1)?)?;
    let start = start_line.checked_add(start.column)?;
    let end = end_line.checked_add(end.column)?;
    (start <= end && end <= source.len()).then_some((start, end))
}

/// Extract the exact source text covered by a syntax item.
pub(super) fn source_span_text(source: &str, span: proc_macro2::Span) -> String {
    let (start, end) = source_span_range(source, span)
        .unwrap_or_else(|| panic!("source span is outside its parsed source: {span:?}"));
    source
        .get(start..end)
        .unwrap_or_else(|| panic!("source span is not on a UTF-8 boundary: {start}..{end}"))
        .to_owned()
}

/// Tokenize a function block and return the statements inside its outer braces.
pub(super) fn tokenize_block_text(path: &str, name: &str, body: &str) -> Vec<Node> {
    let token_stream = body
        .parse::<TokenStream>()
        .unwrap_or_else(|error| panic!("{path} {name} block does not tokenize: {error}"));
    let nodes = nodes_from_stream(&token_stream);
    let Some(Node::Group(Delimiter::Brace, children)) = nodes.first() else {
        panic!("{path} {name} block has no brace group: {body}");
    };
    children.clone()
}

/// One source declaration whose serde wire shape refuses unknown fields.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct WireKey {
    path: String,
    scope: Vec<String>,
    name: String,
}

/// One source span that emits a hand-written reader.
pub(super) struct HandImplSource {
    path: String,
    scope: Vec<String>,
    name: String,
    body: String,
    method_nodes: Vec<Node>,
    /// The parsed method block for ordinary Rust implementations. Macro
    /// declarations are token-only and keep this as `None`; their route is
    /// checked by the conservative macro scanner below.
    method_block: Option<syn::Block>,
    /// Parameter bindings that receive the serde deserializer. A token named
    /// `deserializer` elsewhere in the body is not evidence that a call reads
    /// the method input: local bindings may shadow it, and a helper may merely
    /// mention the spelling.
    deserializer_bindings: BTreeSet<String>,
    local_denied: BTreeSet<String>,
}

#[derive(Default)]
pub(super) struct SourceIndex {
    sources: Vec<HandImplSource>,
    denied: BTreeSet<WireKey>,
    imports: Vec<ImportBinding>,
    symbols: Vec<SymbolKey>,
}

/// One lexical `use` binding. The target is retained as a syntax path so
/// aliases, re-exports, globs, and `self`/`super` can be resolved from the
/// scope in which the declaration appears.
#[derive(Debug, Clone)]
pub(super) struct ImportBinding {
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
pub(super) enum SymbolKind {
    Type,
    Module,
    Function,
    Value,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct SymbolKey {
    path: String,
    scope: Vec<String>,
    name: String,
    kind: SymbolKind,
}

/// Return the elements in `left` that do not have a matching occurrence in
/// `right`, preserving duplicate entries. A set loses the two generic
/// `Provenance` readers and the two `$name` macro readers, which is precisely
/// the traversal gap this census must catch.
pub(super) fn multiset_difference<T: Clone + Ord>(left: &[T], right: &[T]) -> Vec<T> {
    let mut available = BTreeMap::<T, usize>::new();
    for item in right {
        *available.entry(item.clone()).or_default() += 1;
    }
    let mut difference = Vec::new();
    for item in left {
        let Some(count) = available.get_mut(item) else {
            difference.push(item.clone());
            continue;
        };
        *count -= 1;
        if *count == 0 {
            available.remove(item);
        }
    }
    difference
}

/// Whether one source attribute states a bare serde flag.
pub(super) fn serde_has_flag(attrs: &[syn::Attribute], name: &str) -> bool {
    let mut stated = false;
    for attribute in attrs {
        if !attribute.path().is_ident("serde") {
            continue;
        }
        attribute
            .parse_nested_meta(|meta| {
                if meta.path.is_ident(name) {
                    stated = true;
                }
                skip_meta_value(&meta)?;
                Ok(())
            })
            .expect("every serde attribute in the wire crates parses as a nested meta list");
    }
    stated
}

pub(super) fn insert_wire(
    denied: &mut BTreeSet<WireKey>,
    path: &str,
    scope: &[String],
    name: String,
) {
    denied.insert(WireKey {
        path: path.to_owned(),
        scope: scope.to_owned(),
        name,
    });
}

pub(super) fn insert_symbol(
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
pub(super) fn collect_use_tree(
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

pub(super) fn local_denied_types(block: &syn::Block) -> BTreeSet<String> {
    let mut denied = BTreeSet::new();
    for statement in &block.stmts {
        let syn::Stmt::Item(item) = statement else {
            continue;
        };
        match item {
            syn::Item::Struct(declaration)
                if serde_has_flag(&declaration.attrs, "deny_unknown_fields") =>
            {
                denied.insert(declaration.ident.to_string());
            }
            syn::Item::Enum(declaration)
                if serde_has_flag(&declaration.attrs, "deny_unknown_fields") =>
            {
                denied.insert(declaration.ident.to_string());
            }
            _ => {}
        }
    }
    denied
}

/// Whether a bracket group is exactly a serde attribute carrying the denial.
pub(super) fn macro_serde_deny_attribute(group: &[Node]) -> bool {
    if !is_atom(group, 0, "serde") {
        return false;
    }
    group.iter().skip(1).any(|node| {
        let Node::Group(Delimiter::Parenthesis, children) = node else {
            return false;
        };
        children
            .iter()
            .any(|child| atom(child) == Some("deny_unknown_fields"))
    })
}

pub(super) fn macro_item_name(nodes: &[Node], index: usize) -> Option<(String, usize)> {
    if is_atom(nodes, index, "$") {
        let name = atom(nodes.get(index + 1)?)?;
        return is_path_atom(name).then(|| (format!("${name}"), index + 2));
    }
    let name = atom(nodes.get(index)?)?;
    is_path_atom(name).then(|| (name.to_owned(), index + 1))
}

/// Find the declaration immediately following a serde deny attribute.
///
/// The scan skips only more attributes and visibility. It cannot bind an
/// arbitrary identifier or a later declaration merely because a token with the
/// right spelling appears somewhere in the enclosing macro.
pub(super) fn macro_denied_declaration(nodes: &[Node], after_attribute: usize) -> Option<String> {
    let mut index = after_attribute;
    loop {
        if is_atom(nodes, index, "#")
            && matches!(
                nodes.get(index + 1),
                Some(Node::Group(Delimiter::Bracket, _))
            )
        {
            index += 2;
            continue;
        }
        if is_atom(nodes, index, "pub") {
            index += 1;
            if matches!(
                nodes.get(index),
                Some(Node::Group(Delimiter::Parenthesis, _))
            ) {
                index += 1;
            }
            continue;
        }
        break;
    }
    if !is_atom(nodes, index, "struct") && !is_atom(nodes, index, "enum") {
        return None;
    }
    macro_item_name(nodes, index + 1).map(|(name, _)| name)
}

/// Collect denied declarations from one macro token stream with module scope.
pub(super) fn collect_macro_denied(
    nodes: &[Node],
    path: &str,
    scope: &[String],
    denied: &mut BTreeSet<WireKey>,
) {
    let mut index = 0;
    while index < nodes.len() {
        if is_atom(nodes, index, "mod") {
            if let Some((module, after_name)) = macro_item_name(nodes, index + 1) {
                if let Some(Node::Group(Delimiter::Brace, children)) = nodes.get(after_name) {
                    let mut nested_scope = scope.to_owned();
                    nested_scope.push(module);
                    collect_macro_denied(children, path, &nested_scope, denied);
                    index = after_name + 1;
                    continue;
                }
            }
        }

        if is_atom(nodes, index, "#") {
            if let Some(Node::Group(Delimiter::Bracket, attribute)) = nodes.get(index + 1) {
                if macro_serde_deny_attribute(attribute) {
                    if let Some(name) = macro_denied_declaration(nodes, index + 2) {
                        insert_wire(denied, path, scope, name);
                    }
                }
            }
        }

        if let Some(Node::Group(_, children)) = nodes.get(index) {
            collect_macro_denied(children, path, scope, denied);
        }
        index += 1;
    }
}

/// Return the bindings in the first typed parameter of a macro method.
///
/// The recognized declaration macros all emit a conventional
/// `deserialize: D` parameter. Keep the parser conservative: if the pattern
/// is not a single identifier (optionally preceded by `mut`), no call can be
/// certified as consuming the method input.
pub(super) fn macro_parameter_bindings(nodes: &[Node]) -> BTreeSet<String> {
    let mut bindings = BTreeSet::new();
    let Some(first) = split_node_arguments(nodes).into_iter().next() else {
        return bindings;
    };
    let Some(colon) = first.iter().position(|node| atom(node) == Some(":")) else {
        return bindings;
    };
    let pattern = &first[..colon];
    let mut index = 0;
    if is_atom(pattern, index, "mut") {
        index += 1;
    }
    if is_atom(pattern, index, "$") {
        if let Some(name) = atom_opt(pattern.get(index + 1)) {
            if is_path_atom(name) {
                bindings.insert(format!("${name}"));
            }
        }
    } else if let Some(name) = atom_opt(pattern.get(index)) {
        if is_path_atom(name) {
            bindings.insert(name.to_owned());
        }
    }
    bindings
}

/// Parse one macro `impl` and isolate its `Deserialize::deserialize` method.
pub(super) fn macro_route_at(
    nodes: &[Node],
    start: usize,
) -> Option<(String, Vec<Node>, BTreeSet<String>)> {
    if !is_atom(nodes, start, "impl") {
        return None;
    }
    let body_index = (start + 1..nodes.len())
        .find(|index| matches!(nodes.get(*index), Some(Node::Group(Delimiter::Brace, _))))?;
    let header = &nodes[start + 1..body_index];
    let deserialize_index = header
        .iter()
        .position(|node| atom(node) == Some("Deserialize"))?;
    let for_index =
        (deserialize_index + 1..header.len()).find(|index| atom(&header[*index]) == Some("for"))?;
    let (name, _) = macro_item_name(header, for_index + 1)?;
    let Node::Group(Delimiter::Brace, implementation) = nodes.get(body_index)? else {
        return None;
    };
    let method_index = (0..implementation.len()).find(|index| {
        is_atom(implementation, *index, "fn") && is_atom(implementation, index + 1, "deserialize")
    })?;
    let parameters = (method_index + 2..implementation.len()).find(|index| {
        matches!(
            implementation.get(*index),
            Some(Node::Group(Delimiter::Parenthesis, _))
        )
    })?;
    let body_index = (parameters + 1..implementation.len()).find(|index| {
        matches!(
            implementation.get(*index),
            Some(Node::Group(Delimiter::Brace, _))
        )
    })?;
    let Node::Group(Delimiter::Parenthesis, parameter_nodes) = implementation.get(parameters)?
    else {
        return None;
    };
    let deserializer_bindings = macro_parameter_bindings(parameter_nodes);
    let Node::Group(Delimiter::Brace, body) = implementation.get(body_index)? else {
        return None;
    };
    Some((name, body.clone(), deserializer_bindings))
}

pub(super) fn collect_macro_routes(
    nodes: &[Node],
    path: &str,
    scope: &[String],
    found: &mut Vec<HandImplSource>,
) {
    let mut index = 0;
    while index < nodes.len() {
        if let Some((name, method_nodes, deserializer_bindings)) = macro_route_at(nodes, index) {
            let body = nodes_text(&method_nodes);
            found.push(HandImplSource {
                path: path.to_owned(),
                scope: scope.to_owned(),
                name,
                body,
                method_nodes,
                method_block: None,
                deserializer_bindings,
                local_denied: BTreeSet::new(),
            });
        }
        if let Some(Node::Group(_, children)) = nodes.get(index) {
            collect_macro_routes(children, path, scope, found);
        }
        index += 1;
    }
}

/// Return the binding names of the first typed argument in a parsed reader.
///
/// A hand-written serde implementation has one input parameter in this
/// workspace. Requiring a simple identifier keeps the route proof tied to the
/// actual parameter rather than to a conventional spelling found in the
/// method body.
pub(super) fn signature_deserializer_bindings(signature: &syn::Signature) -> BTreeSet<String> {
    let Some(syn::FnArg::Typed(argument)) = signature.inputs.first() else {
        return BTreeSet::new();
    };
    let syn::Pat::Ident(pattern) = argument.pat.as_ref() else {
        return BTreeSet::new();
    };
    BTreeSet::from([pattern.ident.to_string()])
}

pub(super) fn collect_source_items(
    items: &[syn::Item],
    path: &str,
    source: &str,
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
                if serde_has_flag(&declaration.attrs, "deny_unknown_fields") {
                    insert_wire(
                        &mut index.denied,
                        path,
                        scope,
                        declaration.ident.to_string(),
                    );
                }
            }
            syn::Item::Enum(declaration) => {
                insert_symbol(
                    index,
                    path,
                    scope,
                    declaration.ident.to_string(),
                    SymbolKind::Type,
                );
                if serde_has_flag(&declaration.attrs, "deny_unknown_fields") {
                    insert_wire(
                        &mut index.denied,
                        path,
                        scope,
                        declaration.ident.to_string(),
                    );
                }
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
            syn::Item::Impl(implementation) => {
                let Some(name) = deserialize_impl_target(implementation) else {
                    continue;
                };
                let Some(method) = implementation.items.iter().find_map(|item| match item {
                    syn::ImplItem::Fn(method) if method.sig.ident == "deserialize" => Some(method),
                    _ => None,
                }) else {
                    panic!("{path} {name} has no parsed deserialize method");
                };
                let block_text = source_span_text(source, method.block.span());
                let method_nodes = tokenize_block_text(path, &name, &block_text);
                index.sources.push(HandImplSource {
                    path: path.to_owned(),
                    scope: scope.to_owned(),
                    name,
                    body: source_span_text(source, method.span()),
                    method_nodes,
                    method_block: Some(method.block.clone()),
                    deserializer_bindings: signature_deserializer_bindings(&method.sig),
                    local_denied: local_denied_types(&method.block),
                });
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
                    collect_source_items(nested, path, source, &nested_scope, index);
                }
            }
            syn::Item::Macro(macro_item) => {
                let tokens = nodes_from_stream(&macro_item.mac.tokens);
                collect_macro_denied(&tokens, path, scope, &mut index.denied);
                // Macro declarations are collected in the same source pass as
                // their routes. The argument is read only to keep the ownership
                // explicit at this call site; route resolution uses the index
                // after the file walk completes.
                collect_macro_routes(&tokens, path, scope, &mut index.sources);
            }
            _ => {}
        }
    }
}

/// Collect every implementation and scoped wire declaration in the three
/// source crates with one syntax traversal per file.
pub(super) fn source_module_scope(source_root: &Path, file: &Path) -> Vec<String> {
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

pub(super) fn source_index(root: &Path) -> SourceIndex {
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
            let source = std::fs::read_to_string(&file).expect("read source");
            let parsed = syn::parse_file(&source)
                .map_err(|error| format!("{relative} does not parse: {error}"))
                .expect("every source file in the wire crates parses");
            let scope = source_module_scope(&root.join(source_root), &file);
            collect_source_items(&parsed.items, &relative, &source, &scope, &mut index);
        }
    }
    index
}

/// The path immediately before an associated call's `::deserialize`.
pub(super) fn path_tail(nodes: &[Node], end: usize) -> Option<Vec<String>> {
    let (mut start, segment) = path_segment_at_end(nodes, end)?;
    let mut segments = vec![segment];
    while start >= 2 && is_atom(nodes, start - 2, ":") && is_atom(nodes, start - 1, ":") {
        let (previous_start, previous) = path_segment_at_end(nodes, start - 2)?;
        segments.push(previous);
        start = previous_start;
    }
    segments.reverse();
    Some(segments)
}

pub(super) fn path_segment_at_end(nodes: &[Node], end: usize) -> Option<(usize, String)> {
    if end == 0 {
        return None;
    }
    let name = atom(nodes.get(end - 1)?)?;
    if is_path_atom(name) {
        if end >= 2 && is_atom(nodes, end - 2, "$") {
            return Some((end - 2, format!("${name}")));
        }
        return Some((end - 1, name.to_owned()));
    }
    None
}

pub(super) fn matching_angle_open(nodes: &[Node], close: usize) -> Option<usize> {
    let mut depth = 0usize;
    for index in (0..=close).rev() {
        let Some(value) = atom_opt(nodes.get(index)) else {
            // Bracketed, parenthesized, and braced generic arguments are
            // opaque groups. They cannot contain the angle delimiter that
            // pairs with this close token at the current level.
            continue;
        };
        match value {
            ">" => depth += 1,
            "<" => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

/// Split a token sequence at top-level commas. Delimited groups are already
/// represented as one node; angle brackets remain tokens and need a small
/// depth counter for generic arguments.
pub(super) fn split_node_arguments(nodes: &[Node]) -> Vec<Vec<Node>> {
    let mut arguments = Vec::new();
    let mut current = Vec::new();
    let mut angle_depth = 0usize;
    for node in nodes {
        match atom(node) {
            Some("<") => angle_depth += 1,
            Some(">") if angle_depth > 0 => angle_depth -= 1,
            Some(",") if angle_depth == 0 => {
                arguments.push(std::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        current.push(node.clone());
    }
    arguments.push(current);
    arguments
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PathShape {
    segments: Vec<String>,
    absolute: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum TypeShape {
    Path(PathShape),
    Generic(PathShape, Vec<TypeShape>),
    KeylessAggregate,
}

/// Parse only the type shapes needed to classify a Deserialize receiver.
/// Unsupported syntax is deliberately left unresolved and therefore cannot
/// certify a keyless or closed route.
pub(super) fn type_shape(nodes: &[Node]) -> Option<TypeShape> {
    if nodes.is_empty() {
        return None;
    }
    let mut start = 0;
    if is_atom(nodes, start, "&") {
        start += 1;
        if nodes
            .get(start)
            .and_then(atom)
            .is_some_and(|value| value.starts_with('\''))
        {
            start += 1;
        }
        if is_atom(nodes, start, "mut") {
            start += 1;
        }
    }
    let nodes = nodes.get(start..)?;
    if nodes.len() == 1
        && matches!(
            nodes.first(),
            Some(Node::Group(Delimiter::Bracket | Delimiter::Parenthesis, _))
        )
    {
        return Some(TypeShape::KeylessAggregate);
    }
    if is_atom(nodes, nodes.len().checked_sub(1)?, ">") {
        let close = nodes.len() - 1;
        let open = matching_angle_open(nodes, close)?;
        let path = path_before_angle(nodes, open)?;
        let arguments = split_node_arguments(&nodes[open + 1..close])
            .into_iter()
            .map(|argument| type_shape(&argument))
            .collect::<Option<Vec<_>>>()?;
        return Some(TypeShape::Generic(path, arguments));
    }
    Some(TypeShape::Path(path_tail_shape(nodes, nodes.len())?))
}

pub(super) fn path_tail_shape(nodes: &[Node], end: usize) -> Option<PathShape> {
    let (start, segments) = path_tail_with_start(nodes, end)?;
    let absolute = start >= 2 && is_atom(nodes, start - 2, ":") && is_atom(nodes, start - 1, ":");
    Some(PathShape { segments, absolute })
}

pub(super) fn path_tail_with_start(nodes: &[Node], end: usize) -> Option<(usize, Vec<String>)> {
    let (mut start, segment) = path_segment_at_end(nodes, end)?;
    let mut segments = vec![segment];
    while start >= 2 && is_atom(nodes, start - 2, ":") && is_atom(nodes, start - 1, ":") {
        let (previous_start, previous) = path_segment_at_end(nodes, start - 2)?;
        segments.push(previous);
        start = previous_start;
    }
    segments.reverse();
    Some((start, segments))
}

pub(super) fn path_before_angle(nodes: &[Node], open: usize) -> Option<PathShape> {
    let end = if open >= 2 && is_atom(nodes, open - 1, ":") && is_atom(nodes, open - 2, ":") {
        open - 2
    } else {
        open
    };
    path_tail_shape(nodes, end)
}

#[derive(Debug, Clone)]
pub(super) enum Receiver {
    Path(PathShape),
    Generic {
        path: PathShape,
        arguments: Vec<TypeShape>,
    },
    QSelf {
        type_shape: TypeShape,
    },
}

pub(super) fn qself_type(nodes: &[Node], open: usize, close: usize) -> Option<Vec<Node>> {
    let interior = &nodes[open + 1..close];
    let mut angle_depth = 0usize;
    let as_index = interior.iter().position(|node| match atom(node) {
        Some("<") => {
            angle_depth += 1;
            false
        }
        Some(">") => {
            angle_depth = angle_depth.saturating_sub(1);
            false
        }
        Some("as") => angle_depth == 0,
        _ => false,
    });
    let type_nodes = as_index.map_or(interior, |index| &interior[..index]);
    (!type_nodes.is_empty()).then(|| type_nodes.to_vec())
}

pub(super) fn receiver_before(nodes: &[Node], end: usize) -> Option<Receiver> {
    if end == 0 {
        return None;
    }
    if is_atom(nodes, end - 1, ">") {
        let close = end - 1;
        let open = matching_angle_open(nodes, close)?;
        if let Some(path) = path_before_angle(nodes, open) {
            return Some(Receiver::Generic {
                path,
                arguments: split_node_arguments(&nodes[open + 1..close])
                    .into_iter()
                    .map(|argument| type_shape(&argument))
                    .collect::<Option<Vec<_>>>()?,
            });
        }
        let type_nodes = qself_type(nodes, open, close)?;
        return Some(Receiver::QSelf {
            type_shape: type_shape(&type_nodes)?,
        });
    }
    Some(Receiver::Path(path_tail_shape(nodes, end)?))
}

#[derive(Debug)]
pub(super) enum InputRoute {
    FreeForm,
    Value,
    Keyless,
    Wire(Receiver),
}

pub(super) fn type_shape_from_syn(ty: &syn::Type) -> Option<TypeShape> {
    match ty {
        syn::Type::Path(path) if path.qself.is_none() => path_shape_from_syn(&path.path),
        syn::Type::Array(_) | syn::Type::Slice(_) | syn::Type::Tuple(_) => {
            Some(TypeShape::KeylessAggregate)
        }
        syn::Type::Paren(paren) => type_shape_from_syn(&paren.elem),
        syn::Type::Group(group) => type_shape_from_syn(&group.elem),
        syn::Type::Reference(reference) => type_shape_from_syn(&reference.elem),
        _ => None,
    }
}

pub(super) fn path_shape_from_syn(path: &syn::Path) -> Option<TypeShape> {
    path_shape_from_syn_segments(path.leading_colon.is_some(), &path.segments)
}

pub(super) fn path_shape_from_syn_segments(
    absolute: bool,
    segments: &syn::punctuated::Punctuated<syn::PathSegment, syn::Token![::]>,
) -> Option<TypeShape> {
    let mut names = Vec::with_capacity(segments.len());
    let mut arguments = None;
    for (index, segment) in segments.iter().enumerate() {
        names.push(segment.ident.to_string());
        match &segment.arguments {
            syn::PathArguments::None => {}
            syn::PathArguments::AngleBracketed(args) if index + 1 == segments.len() => {
                let mut types = Vec::new();
                for argument in &args.args {
                    let syn::GenericArgument::Type(ty) = argument else {
                        return None;
                    };
                    types.push(type_shape_from_syn(ty)?);
                }
                arguments = Some(types);
            }
            syn::PathArguments::Parenthesized(_) => return None,
            syn::PathArguments::AngleBracketed(_) => return None,
        }
    }
    let path = PathShape {
        segments: names,
        absolute,
    };
    match arguments {
        Some(arguments) => Some(TypeShape::Generic(path, arguments)),
        None => Some(TypeShape::Path(path)),
    }
}

pub(super) fn receiver_from_expr_path(path: &syn::ExprPath) -> Option<Receiver> {
    if path.path.segments.last()?.ident != "deserialize"
        || !matches!(
            path.path.segments.last()?.arguments,
            syn::PathArguments::None
        )
    {
        return None;
    }
    if let Some(qself) = &path.qself {
        return Some(Receiver::QSelf {
            type_shape: type_shape_from_syn(&qself.ty)?,
        });
    }
    let mut segments = path.path.segments.clone();
    segments.pop()?;
    match path_shape_from_syn_segments(path.path.leading_colon.is_some(), &segments)? {
        TypeShape::Path(path) => Some(Receiver::Path(path)),
        TypeShape::Generic(path, arguments) => Some(Receiver::Generic { path, arguments }),
        TypeShape::KeylessAggregate => None,
    }
}

pub(super) fn direct_binding(argument: &[Node], bindings: &BTreeSet<String>) -> bool {
    let mut start = 0;
    if is_atom(argument, start, "&") {
        start += 1;
        if nodes_atom_starts_with_lifetime(argument.get(start)) {
            start += 1;
        }
        if is_atom(argument, start, "mut") {
            start += 1;
        }
    }
    argument.get(start..).is_some_and(|tail| {
        tail.len() == 1 && atom_opt(tail.first()).is_some_and(|name| bindings.contains(name))
    })
}

pub(super) fn nodes_atom_starts_with_lifetime(node: Option<&Node>) -> bool {
    node.and_then(atom)
        .is_some_and(|value| value.starts_with('\''))
}

pub(super) fn call_uses_deserializer(arguments: &[Node], bindings: &BTreeSet<String>) -> bool {
    split_node_arguments(arguments)
        .first()
        .is_some_and(|argument| direct_binding(argument, bindings))
}

pub(super) fn expr_is_direct_binding(expr: &syn::Expr, bindings: &BTreeSet<String>) -> bool {
    match expr {
        syn::Expr::Path(path) => {
            path.qself.is_none()
                && path.path.segments.len() == 1
                && matches!(
                    path.path.segments.first().map(|segment| &segment.arguments),
                    Some(syn::PathArguments::None)
                )
                && path
                    .path
                    .segments
                    .first()
                    .is_some_and(|segment| bindings.contains(&segment.ident.to_string()))
        }
        syn::Expr::Reference(reference) => expr_is_direct_binding(&reference.expr, bindings),
        syn::Expr::Paren(paren) => expr_is_direct_binding(&paren.expr, bindings),
        syn::Expr::Group(group) => expr_is_direct_binding(&group.expr, bindings),
        _ => false,
    }
}

pub(super) fn expr_single_name(expr: &syn::Expr) -> Option<String> {
    let syn::Expr::Path(path) = expr else {
        return None;
    };
    (path.qself.is_none()
        && path.path.segments.len() == 1
        && matches!(
            path.path.segments.first().map(|segment| &segment.arguments),
            Some(syn::PathArguments::None)
        ))
    .then(|| {
        path.path
            .segments
            .first()
            .map(|segment| segment.ident.to_string())
    })
    .flatten()
}

pub(super) fn use_tree_shadows(tree: &syn::UseTree, name: &str) -> bool {
    match tree {
        syn::UseTree::Path(path) => use_tree_shadows(path.tree.as_ref(), name),
        syn::UseTree::Name(binding) => binding.ident == name,
        syn::UseTree::Rename(binding) => binding.rename == name,
        syn::UseTree::Glob(_) => true,
        syn::UseTree::Group(group) => group.items.iter().any(|item| use_tree_shadows(item, name)),
    }
}

pub(super) fn item_shadows_version_check(item: &syn::Item) -> bool {
    match item {
        syn::Item::Fn(function) => function.sig.ident == "check_ir_version",
        syn::Item::Mod(module) => module.ident == "check_ir_version",
        syn::Item::Use(use_item) => use_tree_shadows(&use_item.tree, "check_ir_version"),
        syn::Item::Const(constant) => constant.ident == "check_ir_version",
        syn::Item::Static(static_item) => static_item.ident == "check_ir_version",
        _ => false,
    }
}

pub(super) fn token_mentions_binding(nodes: &[Node], bindings: &BTreeSet<String>) -> bool {
    nodes.iter().any(|node| match node {
        Node::Atom(value) => bindings.contains(value),
        Node::Group(_, children) => token_mentions_binding(children, bindings),
    })
}

pub(super) fn expr_check_binding(expr: &syn::Expr) -> Option<String> {
    let syn::Expr::Call(call) = expr else {
        return None;
    };
    let syn::Expr::Path(path) = call.func.as_ref() else {
        return None;
    };
    if path.qself.is_some()
        || path.path.segments.len() != 1
        || path.path.segments.first()?.ident != "Some"
        || !matches!(
            path.path.segments.first()?.arguments,
            syn::PathArguments::None
        )
        || call.args.len() != 1
    {
        return None;
    }
    let argument = call.args.first()?;
    let syn::Expr::Reference(reference) = argument else {
        return None;
    };
    if reference.mutability.is_some() {
        return None;
    }
    let syn::Expr::Path(path) = reference.expr.as_ref() else {
        return None;
    };
    (path.qself.is_none()
        && path.path.segments.len() == 1
        && matches!(
            path.path.segments.first()?.arguments,
            syn::PathArguments::None
        ))
    .then(|| {
        path.path
            .segments
            .first()
            .map(|segment| segment.ident.to_string())
    })
    .flatten()
}

pub(super) fn path_is_named(path: &syn::Path, name: &str) -> bool {
    path.segments.last().is_some_and(|segment| {
        segment.ident == name && matches!(segment.arguments, syn::PathArguments::None)
    })
}

mod classify;
mod resolve;
mod scan;
#[cfg(test)]
mod tests;

pub(crate) use classify::assert_hand_written_reader_routes;
