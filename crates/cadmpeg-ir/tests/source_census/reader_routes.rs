// SPDX-License-Identifier: Apache-2.0
/// Semantic route checks for hand-written source readers.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use proc_macro2::{Delimiter, TokenStream, TokenTree};
use syn::spanned::Spanned;

use super::{
    collect_rust_sources, deserialize_impl_target, is_test_module, is_test_path, skip_meta_value,
    HAND_IMPLS,
};

/// One route shape a hand-written reader is allowed to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum HandReaderClass {
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

fn atom_opt(node: Option<&Node>) -> Option<&str> {
    node.and_then(atom)
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

/// Render a token tree for a useful unresolved-route diagnostic.
fn node_text(node: &Node) -> String {
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

fn nodes_text(nodes: &[Node]) -> String {
    nodes.iter().map(node_text).collect()
}

/// Return the byte range covered by a proc-macro span.
///
/// `Span::line` and `Span::column` are byte positions. Slicing by complete
/// lines was previously used here; two adjacent items on one line then shared
/// the same source text and could contaminate one another's route.
fn source_span_range(source: &str, span: proc_macro2::Span) -> Option<(usize, usize)> {
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
fn source_span_text(source: &str, span: proc_macro2::Span) -> String {
    let (start, end) = source_span_range(source, span)
        .unwrap_or_else(|| panic!("source span is outside its parsed source: {span:?}"));
    source
        .get(start..end)
        .unwrap_or_else(|| panic!("source span is not on a UTF-8 boundary: {start}..{end}"))
        .to_owned()
}

/// Tokenize a function block and return the statements inside its outer braces.
fn tokenize_block_text(path: &str, name: &str, body: &str) -> Vec<Node> {
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
struct WireKey {
    path: String,
    scope: Vec<String>,
    name: String,
}

/// One source span that emits a hand-written reader.
struct HandImplSource {
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
struct SourceIndex {
    sources: Vec<HandImplSource>,
    denied: BTreeSet<WireKey>,
    imports: Vec<ImportBinding>,
    symbols: Vec<SymbolKey>,
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
fn serde_has_flag(attrs: &[syn::Attribute], name: &str) -> bool {
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

fn insert_wire(denied: &mut BTreeSet<WireKey>, path: &str, scope: &[String], name: String) {
    denied.insert(WireKey {
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

fn local_denied_types(block: &syn::Block) -> BTreeSet<String> {
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
fn macro_serde_deny_attribute(group: &[Node]) -> bool {
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

fn macro_item_name(nodes: &[Node], index: usize) -> Option<(String, usize)> {
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
fn macro_denied_declaration(nodes: &[Node], after_attribute: usize) -> Option<String> {
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
fn collect_macro_denied(
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
fn macro_parameter_bindings(nodes: &[Node]) -> BTreeSet<String> {
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
fn macro_route_at(nodes: &[Node], start: usize) -> Option<(String, Vec<Node>, BTreeSet<String>)> {
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

fn collect_macro_routes(
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
fn signature_deserializer_bindings(signature: &syn::Signature) -> BTreeSet<String> {
    let Some(syn::FnArg::Typed(argument)) = signature.inputs.first() else {
        return BTreeSet::new();
    };
    let syn::Pat::Ident(pattern) = argument.pat.as_ref() else {
        return BTreeSet::new();
    };
    BTreeSet::from([pattern.ident.to_string()])
}

fn collect_source_items(
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
fn path_tail(nodes: &[Node], end: usize) -> Option<Vec<String>> {
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

fn path_segment_at_end(nodes: &[Node], end: usize) -> Option<(usize, String)> {
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

fn matching_angle_open(nodes: &[Node], close: usize) -> Option<usize> {
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
fn split_node_arguments(nodes: &[Node]) -> Vec<Vec<Node>> {
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
struct PathShape {
    segments: Vec<String>,
    absolute: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TypeShape {
    Path(PathShape),
    Generic(PathShape, Vec<TypeShape>),
    KeylessAggregate,
}

/// Parse only the type shapes needed to classify a Deserialize receiver.
/// Unsupported syntax is deliberately left unresolved and therefore cannot
/// certify a keyless or closed route.
fn type_shape(nodes: &[Node]) -> Option<TypeShape> {
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

fn path_tail_shape(nodes: &[Node], end: usize) -> Option<PathShape> {
    let (start, segments) = path_tail_with_start(nodes, end)?;
    let absolute = start >= 2 && is_atom(nodes, start - 2, ":") && is_atom(nodes, start - 1, ":");
    Some(PathShape { segments, absolute })
}

fn path_tail_with_start(nodes: &[Node], end: usize) -> Option<(usize, Vec<String>)> {
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

fn path_before_angle(nodes: &[Node], open: usize) -> Option<PathShape> {
    let end = if open >= 2 && is_atom(nodes, open - 1, ":") && is_atom(nodes, open - 2, ":") {
        open - 2
    } else {
        open
    };
    path_tail_shape(nodes, end)
}

#[derive(Debug, Clone)]
enum Receiver {
    Path(PathShape),
    Generic {
        path: PathShape,
        arguments: Vec<TypeShape>,
    },
    QSelf {
        type_shape: TypeShape,
    },
}

fn qself_type(nodes: &[Node], open: usize, close: usize) -> Option<Vec<Node>> {
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

fn receiver_before(nodes: &[Node], end: usize) -> Option<Receiver> {
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
enum InputRoute {
    FreeForm,
    Value,
    Keyless,
    Wire(Receiver),
}

fn type_shape_from_syn(ty: &syn::Type) -> Option<TypeShape> {
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

fn path_shape_from_syn(path: &syn::Path) -> Option<TypeShape> {
    path_shape_from_syn_segments(path.leading_colon.is_some(), &path.segments)
}

fn path_shape_from_syn_segments(
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

fn receiver_from_expr_path(path: &syn::ExprPath) -> Option<Receiver> {
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

fn direct_binding(argument: &[Node], bindings: &BTreeSet<String>) -> bool {
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

fn nodes_atom_starts_with_lifetime(node: Option<&Node>) -> bool {
    node.and_then(atom)
        .is_some_and(|value| value.starts_with('\''))
}

fn call_uses_deserializer(arguments: &[Node], bindings: &BTreeSet<String>) -> bool {
    split_node_arguments(arguments)
        .first()
        .is_some_and(|argument| direct_binding(argument, bindings))
}

fn expr_is_direct_binding(expr: &syn::Expr, bindings: &BTreeSet<String>) -> bool {
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

fn expr_single_name(expr: &syn::Expr) -> Option<String> {
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

fn use_tree_shadows(tree: &syn::UseTree, name: &str) -> bool {
    match tree {
        syn::UseTree::Path(path) => use_tree_shadows(path.tree.as_ref(), name),
        syn::UseTree::Name(binding) => binding.ident == name,
        syn::UseTree::Rename(binding) => binding.rename == name,
        syn::UseTree::Glob(_) => true,
        syn::UseTree::Group(group) => group.items.iter().any(|item| use_tree_shadows(item, name)),
    }
}

fn item_shadows_version_check(item: &syn::Item) -> bool {
    match item {
        syn::Item::Fn(function) => function.sig.ident == "check_ir_version",
        syn::Item::Mod(module) => module.ident == "check_ir_version",
        syn::Item::Use(use_item) => use_tree_shadows(&use_item.tree, "check_ir_version"),
        syn::Item::Const(constant) => constant.ident == "check_ir_version",
        syn::Item::Static(static_item) => static_item.ident == "check_ir_version",
        _ => false,
    }
}

fn token_mentions_binding(nodes: &[Node], bindings: &BTreeSet<String>) -> bool {
    nodes.iter().any(|node| match node {
        Node::Atom(value) => bindings.contains(value),
        Node::Group(_, children) => token_mentions_binding(children, bindings),
    })
}

fn expr_check_binding(expr: &syn::Expr) -> Option<String> {
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

fn path_is_named(path: &syn::Path, name: &str) -> bool {
    path.segments.last().is_some_and(|segment| {
        segment.ident == name && matches!(segment.arguments, syn::PathArguments::None)
    })
}

#[derive(Debug, Clone, Default)]
struct ScanEnvironment {
    input_bindings: BTreeSet<String>,
    validated_values: BTreeSet<String>,
    version_check_shadowed: bool,
    invalidated_values: BTreeSet<String>,
}

#[derive(Debug, Default)]
struct RouteScan {
    routes: Vec<InputRoute>,
    version_gates: usize,
    unsupported: Option<String>,
}

fn unsupported(scan: &mut RouteScan, message: impl Into<String>) {
    if scan.unsupported.is_none() {
        scan.unsupported = Some(message.into());
    }
}

fn invalidate_validated_value(environment: &mut ScanEnvironment, name: String) {
    if environment.validated_values.remove(&name) {
        environment.invalidated_values.insert(name);
    }
}

fn merge_invalidated_values(environment: &mut ScanEnvironment, nested: &ScanEnvironment) {
    for name in &nested.invalidated_values {
        environment.invalidated_values.insert(name.clone());
        environment.validated_values.remove(name);
    }
}

fn scan_isolated_expr(
    expression: &syn::Expr,
    route: &HandImplSource,
    index: &SourceIndex,
    environment: &mut ScanEnvironment,
    scan: &mut RouteScan,
    control: bool,
    propagates: bool,
) {
    let mut nested = environment.clone();
    scan_expr(
        expression,
        route,
        index,
        &mut nested,
        scan,
        control,
        propagates,
    );
    merge_invalidated_values(environment, &nested);
}

fn scan_borrowed_expr(
    expression: &syn::Expr,
    route: &HandImplSource,
    index: &SourceIndex,
    environment: &mut ScanEnvironment,
    scan: &mut RouteScan,
    control: bool,
) {
    if expr_single_name(expression).is_some_and(|name| environment.validated_values.contains(&name))
    {
        return;
    }
    scan_expr(expression, route, index, environment, scan, control, false);
}

fn pattern_bindings(pattern: &syn::Pat, found: &mut BTreeSet<String>) {
    match pattern {
        syn::Pat::Ident(pattern) => {
            found.insert(pattern.ident.to_string());
            if let Some((_, subpattern)) = &pattern.subpat {
                pattern_bindings(subpattern, found);
            }
        }
        syn::Pat::Tuple(pattern) => {
            for element in &pattern.elems {
                pattern_bindings(element, found);
            }
        }
        syn::Pat::TupleStruct(pattern) => {
            for element in &pattern.elems {
                pattern_bindings(element, found);
            }
        }
        syn::Pat::Struct(pattern) => {
            for field in &pattern.fields {
                pattern_bindings(&field.pat, found);
            }
        }
        syn::Pat::Reference(pattern) => pattern_bindings(&pattern.pat, found),
        syn::Pat::Type(pattern) => pattern_bindings(&pattern.pat, found),
        syn::Pat::Slice(pattern) => {
            for element in &pattern.elems {
                pattern_bindings(element, found);
            }
        }
        syn::Pat::Or(pattern) => {
            for case in &pattern.cases {
                pattern_bindings(case, found);
            }
        }
        _ => {}
    }
}

fn call_route(
    call: &syn::ExprCall,
    route: &HandImplSource,
    index: &SourceIndex,
    environment: &ScanEnvironment,
) -> Option<InputRoute> {
    let syn::Expr::Path(path) = call.func.as_ref() else {
        return None;
    };
    if !call
        .args
        .first()
        .is_some_and(|argument| expr_is_direct_binding(argument, &environment.input_bindings))
    {
        return None;
    }
    if let Some(input_route) = helper_route(call, route, index, environment) {
        return Some(input_route);
    }
    if let Some(receiver) = receiver_from_expr_path(path) {
        return Some(if receiver_is_keyless(route, &receiver, index) {
            InputRoute::Keyless
        } else if receiver_is_value(route, &receiver, index) {
            InputRoute::Value
        } else {
            InputRoute::Wire(receiver)
        });
    }
    None
}

fn method_route(method: &syn::ExprMethodCall, environment: &ScanEnvironment) -> bool {
    method.method == "deserialize_any"
        && expr_is_direct_binding(&method.receiver, &environment.input_bindings)
}

fn expr_path_shape(path: &syn::ExprPath) -> Option<PathShape> {
    if path.qself.is_some() {
        return None;
    }
    let mut segments = Vec::with_capacity(path.path.segments.len());
    for segment in &path.path.segments {
        if !matches!(segment.arguments, syn::PathArguments::None) {
            return None;
        }
        segments.push(segment.ident.to_string());
    }
    Some(PathShape {
        segments,
        absolute: path.path.leading_colon.is_some(),
    })
}

fn resolved_helper_route(resolved: &ResolvedPath) -> Option<InputRoute> {
    match resolved {
        ResolvedPath::External(path)
            if path_matches(path, &["cadmpeg_core", "distinct_keys", "json_object"])
                || path_matches(path, &["cadmpeg_core", "distinct_keys", "btree_map"]) =>
        {
            Some(InputRoute::FreeForm)
        }
        ResolvedPath::External(path)
            if path_matches(path, &["cadmpeg_core", "bytes", "deserialize"]) =>
        {
            Some(InputRoute::Keyless)
        }
        ResolvedPath::Local(symbol)
            if symbol.kind == SymbolKind::Function
                && ((symbol.path.ends_with("/src/bytes.rs") && symbol.name == "deserialize")
                    || (symbol.path.ends_with("/src/units.rs")
                        && matches!(
                            symbol.name.as_str(),
                            "deserialize_named" | "deserialize_named_optional"
                        ))) =>
        {
            Some(InputRoute::Keyless)
        }
        ResolvedPath::Local(symbol)
            if symbol.kind == SymbolKind::Function
                && symbol.path.ends_with("/src/distinct_keys.rs")
                && matches!(symbol.name.as_str(), "json_object" | "btree_map") =>
        {
            Some(InputRoute::FreeForm)
        }
        _ => None,
    }
}

fn path_matches(path: &[String], expected: &[&str]) -> bool {
    path.len() == expected.len()
        && path
            .iter()
            .zip(expected)
            .all(|(actual, wanted)| actual == wanted)
}

fn helper_route(
    call: &syn::ExprCall,
    route: &HandImplSource,
    index: &SourceIndex,
    environment: &ScanEnvironment,
) -> Option<InputRoute> {
    if !call
        .args
        .first()
        .is_some_and(|argument| expr_is_direct_binding(argument, &environment.input_bindings))
    {
        return None;
    }
    let syn::Expr::Path(path) = call.func.as_ref() else {
        return None;
    };
    let shape = expr_path_shape(path)?;
    resolved_helper_route(&resolve_receiver_path(route, &shape, index))
}

fn scan_expr(
    expression: &syn::Expr,
    route: &HandImplSource,
    index: &SourceIndex,
    environment: &mut ScanEnvironment,
    scan: &mut RouteScan,
    control: bool,
    propagates: bool,
) {
    if scan.unsupported.is_some() {
        return;
    }
    match expression {
        syn::Expr::Call(call) => {
            if let Some(input_route) = call_route(call, route, index, environment) {
                if control {
                    unsupported(
                        scan,
                        "input route occurs only on a conditional/control-flow path",
                    );
                } else if !propagates {
                    unsupported(
                        scan,
                        "input route does not propagate its deserialization error",
                    );
                } else {
                    scan.routes.push(input_route);
                }
            } else if is_owned_version_check(call.func.as_ref(), route, index, environment)
                && propagates
                && !control
                && call.args.len() == 1
                && expr_check_binding(call.args.first().expect("one check argument"))
                    .is_some_and(|name| environment.validated_values.contains(&name))
            {
                scan.version_gates += 1;
            } else if call
                .args
                .iter()
                .any(|argument| expr_is_direct_binding(argument, &environment.input_bindings))
            {
                unsupported(
                    scan,
                    "unresolved function call receives the deserializer input",
                );
            }
            for argument in &call.args {
                scan_expr(argument, route, index, environment, scan, control, false);
            }
        }
        syn::Expr::MethodCall(method) => {
            if method_route(method, environment) {
                if control {
                    unsupported(
                        scan,
                        "open-map route occurs only on a conditional/control-flow path",
                    );
                } else if !propagates {
                    unsupported(
                        scan,
                        "open-map route does not propagate its deserialization error",
                    );
                } else {
                    scan.routes.push(InputRoute::FreeForm);
                }
                for argument in &method.args {
                    scan_expr(argument, route, index, environment, scan, control, false);
                }
            } else {
                if expr_is_direct_binding(&method.receiver, &environment.input_bindings)
                    || method.args.iter().any(|argument| {
                        expr_is_direct_binding(argument, &environment.input_bindings)
                    })
                {
                    unsupported(
                        scan,
                        "unresolved method call receives the deserializer input",
                    );
                    return;
                }
                let preserves = matches!(
                    method.method.to_string().as_str(),
                    "map" | "map_err" | "and_then"
                );
                scan_expr(
                    &method.receiver,
                    route,
                    index,
                    environment,
                    scan,
                    control,
                    propagates && preserves,
                );
                for argument in &method.args {
                    scan_expr(argument, route, index, environment, scan, control, false);
                }
            }
        }
        syn::Expr::Try(expression) => scan_expr(
            &expression.expr,
            route,
            index,
            environment,
            scan,
            control,
            true,
        ),
        syn::Expr::If(expression) => {
            scan_expr(
                &expression.cond,
                route,
                index,
                environment,
                scan,
                control,
                propagates,
            );
            scan_isolated_block(
                &expression.then_branch,
                route,
                index,
                environment,
                scan,
                true,
                false,
            );
            if let Some((_, otherwise)) = &expression.else_branch {
                scan_isolated_expr(otherwise, route, index, environment, scan, true, false);
            }
        }
        syn::Expr::Match(expression) => {
            scan_expr(
                &expression.expr,
                route,
                index,
                environment,
                scan,
                control,
                propagates,
            );
            for arm in &expression.arms {
                scan_isolated_expr(&arm.body, route, index, environment, scan, true, false);
            }
        }
        syn::Expr::ForLoop(expression) => {
            scan_expr(
                &expression.expr,
                route,
                index,
                environment,
                scan,
                control,
                propagates,
            );
            scan_isolated_block(
                &expression.body,
                route,
                index,
                environment,
                scan,
                true,
                false,
            );
        }
        syn::Expr::While(expression) => {
            scan_expr(
                &expression.cond,
                route,
                index,
                environment,
                scan,
                true,
                false,
            );
            scan_isolated_block(
                &expression.body,
                route,
                index,
                environment,
                scan,
                true,
                false,
            );
        }
        syn::Expr::Loop(expression) => scan_isolated_block(
            &expression.body,
            route,
            index,
            environment,
            scan,
            true,
            false,
        ),
        syn::Expr::Closure(expression) => {
            let mut nested = environment.clone();
            for input in &expression.inputs {
                let mut names = BTreeSet::new();
                pattern_bindings(input, &mut names);
                for name in names {
                    nested.input_bindings.remove(&name);
                    nested.validated_values.remove(&name);
                }
            }
            scan_expr(
                &expression.body,
                route,
                index,
                &mut nested,
                scan,
                true,
                false,
            );
            merge_invalidated_values(environment, &nested);
        }
        syn::Expr::Block(expression) => scan_isolated_block(
            &expression.block,
            route,
            index,
            environment,
            scan,
            control,
            propagates,
        ),
        syn::Expr::Unsafe(expression) => scan_isolated_block(
            &expression.block,
            route,
            index,
            environment,
            scan,
            true,
            false,
        ),
        syn::Expr::TryBlock(expression) => scan_isolated_block(
            &expression.block,
            route,
            index,
            environment,
            scan,
            true,
            false,
        ),
        syn::Expr::Return(expression) => {
            if let Some(value) = &expression.expr {
                scan_expr(value, route, index, environment, scan, control, true);
            }
        }
        syn::Expr::Break(expression) => {
            if let Some(value) = &expression.expr {
                scan_expr(value, route, index, environment, scan, true, false);
            }
        }
        syn::Expr::Assign(expression) => {
            if let syn::Expr::Path(path) = expression.left.as_ref() {
                if path.qself.is_none()
                    && path.path.segments.len() == 1
                    && path.path.segments.first().is_some_and(|segment| {
                        let name = segment.ident.to_string();
                        environment.input_bindings.contains(&name)
                            || environment.validated_values.contains(&name)
                    })
                {
                    let name = path
                        .path
                        .segments
                        .first()
                        .expect("one path segment")
                        .ident
                        .to_string();
                    environment.input_bindings.remove(&name);
                    invalidate_validated_value(environment, name);
                }
            }
            scan_expr(
                &expression.right,
                route,
                index,
                environment,
                scan,
                control,
                false,
            );
        }
        syn::Expr::Reference(expression) => {
            if expression.mutability.is_some() {
                scan_expr(
                    &expression.expr,
                    route,
                    index,
                    environment,
                    scan,
                    control,
                    false,
                );
            } else {
                scan_borrowed_expr(&expression.expr, route, index, environment, scan, control);
            }
        }
        syn::Expr::Paren(expression) => scan_expr(
            &expression.expr,
            route,
            index,
            environment,
            scan,
            control,
            propagates,
        ),
        syn::Expr::Group(expression) => scan_expr(
            &expression.expr,
            route,
            index,
            environment,
            scan,
            control,
            propagates,
        ),
        syn::Expr::Array(expression) => {
            for element in &expression.elems {
                scan_expr(element, route, index, environment, scan, control, false);
            }
        }
        syn::Expr::Tuple(expression) => {
            for element in &expression.elems {
                scan_expr(element, route, index, environment, scan, control, false);
            }
        }
        syn::Expr::Struct(expression) => {
            if let Some(rest) = &expression.rest {
                scan_expr(rest, route, index, environment, scan, control, false);
            }
            for field in &expression.fields {
                scan_expr(&field.expr, route, index, environment, scan, control, false);
            }
        }
        syn::Expr::Index(expression) => scan_expr(
            &expression.expr,
            route,
            index,
            environment,
            scan,
            control,
            false,
        ),
        syn::Expr::Field(expression) => scan_expr(
            &expression.base,
            route,
            index,
            environment,
            scan,
            control,
            false,
        ),
        syn::Expr::Unary(expression) => scan_expr(
            &expression.expr,
            route,
            index,
            environment,
            scan,
            control,
            false,
        ),
        syn::Expr::Cast(expression) => scan_expr(
            &expression.expr,
            route,
            index,
            environment,
            scan,
            control,
            false,
        ),
        syn::Expr::Await(expression) => scan_expr(
            &expression.base,
            route,
            index,
            environment,
            scan,
            control,
            false,
        ),
        syn::Expr::Repeat(expression) => scan_expr(
            &expression.expr,
            route,
            index,
            environment,
            scan,
            control,
            false,
        ),
        syn::Expr::Binary(expression) => {
            scan_expr(
                &expression.left,
                route,
                index,
                environment,
                scan,
                control,
                false,
            );
            scan_expr(
                &expression.right,
                route,
                index,
                environment,
                scan,
                control || matches!(expression.op, syn::BinOp::And(_) | syn::BinOp::Or(_)),
                false,
            );
        }
        syn::Expr::Range(expression) => {
            if let Some(from) = &expression.start {
                scan_expr(from, route, index, environment, scan, control, false);
            }
            if let Some(to) = &expression.end {
                scan_expr(to, route, index, environment, scan, control, false);
            }
        }
        syn::Expr::Macro(expression) => {
            let nodes = nodes_from_stream(&expression.mac.tokens);
            if token_mentions_binding(&nodes, &environment.input_bindings) {
                unsupported(
                    scan,
                    "input route is hidden inside an unproved macro invocation",
                );
            }
        }
        syn::Expr::Path(_) => {
            if let Some(name) = expr_single_name(expression) {
                invalidate_validated_value(environment, name);
            }
        }
        syn::Expr::Infer(_) | syn::Expr::Lit(_) => {}
        syn::Expr::Const(_) | syn::Expr::Verbatim(_) => unsupported(
            scan,
            format!(
                "unsupported opaque expression form in a hand-written reader at {:?}",
                expression.span().start()
            ),
        ),
        _ => unsupported(
            scan,
            format!(
                "unsupported expression form in a hand-written reader at {:?}",
                expression.span().start()
            ),
        ),
    }
}

fn scan_block(
    block: &syn::Block,
    route: &HandImplSource,
    index: &SourceIndex,
    environment: &mut ScanEnvironment,
    scan: &mut RouteScan,
    control: bool,
    propagates: bool,
) {
    let mut nested = environment.clone();
    if block.stmts.iter().any(
        |statement| matches!(statement, syn::Stmt::Item(item) if item_shadows_version_check(item)),
    ) {
        nested.version_check_shadowed = true;
    }
    let mut reachable = true;
    for statement in &block.stmts {
        if scan.unsupported.is_some() {
            return;
        }
        match statement {
            syn::Stmt::Local(local) => {
                let value_name = local_pattern_name(&local.pat).filter(|_| {
                    local
                        .init
                        .as_ref()
                        .is_some_and(|init| is_value_try_call(&init.expr, route, index, &nested))
                });
                if let Some(init) = &local.init {
                    scan_expr(
                        &init.expr,
                        route,
                        index,
                        &mut nested,
                        scan,
                        control || !reachable,
                        false,
                    );
                    if init.diverge.is_some() {
                        scan_expr(
                            &init.diverge.as_ref().expect("diverging initializer").1,
                            route,
                            index,
                            &mut nested,
                            scan,
                            true,
                            false,
                        );
                    }
                }
                let mut names = BTreeSet::new();
                pattern_bindings(&local.pat, &mut names);
                if names.contains("check_ir_version") {
                    nested.version_check_shadowed = true;
                }
                for name in names {
                    nested.input_bindings.remove(&name);
                    invalidate_validated_value(&mut nested, name);
                }
                if let Some(name) = value_name {
                    nested.validated_values.insert(name);
                }
            }
            syn::Stmt::Expr(expression, semi) => {
                scan_expr(
                    expression,
                    route,
                    index,
                    &mut nested,
                    scan,
                    control || !reachable,
                    semi.is_none() && reachable && propagates,
                );
                if matches!(
                    expression,
                    syn::Expr::Return(_) | syn::Expr::Break(_) | syn::Expr::Continue(_)
                ) {
                    reachable = false;
                }
            }
            syn::Stmt::Item(_) => {}
            syn::Stmt::Macro(statement) => {
                if token_mentions_binding(
                    &nodes_from_stream(&statement.mac.tokens),
                    &nested.input_bindings,
                ) {
                    unsupported(
                        scan,
                        "input route is hidden inside an unproved macro invocation",
                    );
                }
            }
        }
    }
    *environment = nested;
}

fn scan_isolated_block(
    block: &syn::Block,
    route: &HandImplSource,
    index: &SourceIndex,
    environment: &mut ScanEnvironment,
    scan: &mut RouteScan,
    control: bool,
    propagates: bool,
) {
    let mut nested = environment.clone();
    scan_block(block, route, index, &mut nested, scan, control, propagates);
    merge_invalidated_values(environment, &nested);
}

fn local_pattern_name(pattern: &syn::Pat) -> Option<String> {
    match pattern {
        syn::Pat::Ident(pattern) if pattern.subpat.is_none() => Some(pattern.ident.to_string()),
        _ => None,
    }
}

fn is_value_try_call(
    expression: &syn::Expr,
    route: &HandImplSource,
    index: &SourceIndex,
    environment: &ScanEnvironment,
) -> bool {
    let syn::Expr::Try(try_expression) = expression else {
        return false;
    };
    let syn::Expr::Call(call) = try_expression.expr.as_ref() else {
        return false;
    };
    matches!(
        call_route(call, route, index, environment),
        Some(InputRoute::Value)
    )
}

fn path_is_named_path(expression: &syn::Expr, name: &str) -> bool {
    let syn::Expr::Path(path) = expression else {
        return false;
    };
    path.qself.is_none() && path_is_named(&path.path, name)
}

fn is_owned_version_check(
    expression: &syn::Expr,
    route: &HandImplSource,
    index: &SourceIndex,
    environment: &ScanEnvironment,
) -> bool {
    if environment.version_check_shadowed {
        return false;
    }
    if !path_is_named_path(expression, "check_ir_version") {
        return false;
    }
    let syn::Expr::Path(path) = expression else {
        return false;
    };
    let Some(shape) = expr_path_shape(path) else {
        return false;
    };
    match resolve_receiver_path(route, &shape, index) {
        ResolvedPath::Local(symbol) => {
            symbol.kind == SymbolKind::Function
                && symbol.name == "check_ir_version"
                && symbol.path.ends_with("/src/document.rs")
        }
        ResolvedPath::External(_) | ResolvedPath::Unknown => false,
    }
}

#[derive(Debug, Clone)]
enum ResolvedPath {
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

fn dedup_symbols(symbols: impl IntoIterator<Item = SymbolKey>) -> Vec<SymbolKey> {
    let mut found = BTreeSet::new();
    symbols
        .into_iter()
        .filter(|symbol| found.insert(symbol.clone()))
        .collect()
}

fn module_scope_for(symbol: &SymbolKey) -> Vec<String> {
    let mut scope = symbol.scope.clone();
    scope.push(symbol.name.clone());
    scope
}

fn module_source_path(fallback: &str, module_scope: &[String], index: &SourceIndex) -> String {
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
        .map(|(path, _)| path.clone())
        .unwrap_or_else(|| fallback.to_owned())
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

fn resolve_path_from_context(
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

fn resolve_receiver_path(
    route: &HandImplSource,
    path: &PathShape,
    index: &SourceIndex,
) -> ResolvedPath {
    resolve_path_from_context(&route.path, &route.scope, path, index, &mut BTreeSet::new())
}

fn external_is_keyless(path: &[String]) -> bool {
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

fn external_is_value(path: &[String]) -> bool {
    path.len() == 2 && path[0] == "serde_json" && path[1] == "Value"
}

fn type_is_keyless(route: &HandImplSource, shape: &TypeShape, index: &SourceIndex) -> bool {
    match shape {
        TypeShape::KeylessAggregate => true,
        TypeShape::Path(path) => {
            if path.segments == ["$raw"] {
                return true;
            }
            matches!(
                resolve_path_from_context(
                    &route.path,
                    &route.scope,
                    path,
                    index,
                    &mut BTreeSet::new(),
                ),
                ResolvedPath::External(ref canonical) if external_is_keyless(canonical)
            )
        }
        TypeShape::Generic(path, arguments) => {
            let resolved = resolve_path_from_context(
                &route.path,
                &route.scope,
                path,
                index,
                &mut BTreeSet::new(),
            );
            let ResolvedPath::External(canonical) = resolved else {
                return false;
            };
            if canonical.len() == 3
                && canonical[0] == "std"
                && canonical[1] == "vec"
                && canonical[2] == "Vec"
            {
                // A sequence reader consumes an array before it can inspect
                // an element, so an object key cannot reach its element type.
                return true;
            }
            ((canonical.len() == 3
                && canonical[0] == "std"
                && canonical[1] == "boxed"
                && canonical[2] == "Box")
                || (canonical.len() == 3
                    && canonical[0] == "core"
                    && canonical[1] == "option"
                    && canonical[2] == "Option")
                || (canonical.len() == 3
                    && canonical[0] == "std"
                    && canonical[1] == "rc"
                    && canonical[2] == "Rc")
                || (canonical.len() == 3
                    && canonical[0] == "std"
                    && canonical[1] == "sync"
                    && canonical[2] == "Arc")
                || (canonical.len() == 3
                    && canonical[0] == "core"
                    && canonical[1] == "pin"
                    && canonical[2] == "Pin")
                || (canonical.len() == 3
                    && canonical[0] == "std"
                    && canonical[1] == "cell"
                    && canonical[2] == "RefCell")
                || (canonical.len() == 3
                    && canonical[0] == "std"
                    && canonical[1] == "cell"
                    && canonical[2] == "Cell")
                || (canonical.len() == 3
                    && canonical[0] == "std"
                    && canonical[1] == "sync"
                    && canonical[2] == "Mutex")
                || (canonical.len() == 3
                    && canonical[0] == "std"
                    && canonical[1] == "sync"
                    && canonical[2] == "RwLock"))
                && arguments.len() == 1
                && type_is_keyless(route, &arguments[0], index)
        }
    }
}

fn receiver_is_keyless(route: &HandImplSource, receiver: &Receiver, index: &SourceIndex) -> bool {
    match receiver {
        Receiver::Path(path) => {
            if path.segments.len() == 1 && path.segments[0] == "$raw" {
                return true;
            }
            matches!(
                resolve_receiver_path(route, path, index),
                ResolvedPath::External(ref canonical) if external_is_keyless(canonical)
            )
        }
        Receiver::Generic { path, arguments } => type_is_keyless(
            route,
            &TypeShape::Generic(path.clone(), arguments.clone()),
            index,
        ),
        Receiver::QSelf { type_shape } => type_is_keyless(route, type_shape, index),
    }
}

fn receiver_is_value(route: &HandImplSource, receiver: &Receiver, index: &SourceIndex) -> bool {
    let resolved = match receiver {
        Receiver::Path(path) => resolve_receiver_path(route, path, index),
        Receiver::Generic { .. } => return false,
        Receiver::QSelf { type_shape } => match type_shape {
            TypeShape::Path(path) => resolve_receiver_path(route, path, index),
            _ => return false,
        },
    };
    matches!(resolved, ResolvedPath::External(path) if external_is_value(&path))
}

fn deserialize_receiver_at(
    nodes: &[Node],
    index: usize,
    bindings: &BTreeSet<String>,
) -> Option<Receiver> {
    if !is_atom(nodes, index, "deserialize")
        || index < 2
        || !is_atom(nodes, index - 2, ":")
        || !is_atom(nodes, index - 1, ":")
    {
        return None;
    }
    let Some(Node::Group(Delimiter::Parenthesis, arguments)) = nodes.get(index + 1) else {
        return None;
    };
    if !call_uses_deserializer(arguments, bindings) {
        return None;
    }
    receiver_before(nodes, index - 2)
}

fn receiver_description(receiver: &Receiver) -> String {
    fn shape_description(shape: &TypeShape) -> String {
        match shape {
            TypeShape::KeylessAggregate => "aggregate".to_owned(),
            TypeShape::Path(path) => {
                let prefix = if path.absolute { "::" } else { "" };
                format!("{prefix}{}", path.segments.join("::"))
            }
            TypeShape::Generic(path, arguments) => {
                let prefix = if path.absolute { "::" } else { "" };
                format!(
                    "{prefix}{}<{}>",
                    path.segments.join("::"),
                    arguments
                        .iter()
                        .map(shape_description)
                        .collect::<Vec<_>>()
                        .join(",")
                )
            }
        }
    }
    match receiver {
        Receiver::Path(path) => {
            let prefix = if path.absolute { "::" } else { "" };
            format!("{prefix}{}", path.segments.join("::"))
        }
        Receiver::Generic { path, arguments } => format!(
            "{}{}<{}>",
            if path.absolute { "::" } else { "" },
            path.segments.join("::"),
            arguments
                .iter()
                .map(shape_description)
                .collect::<Vec<_>>()
                .join(",")
        ),
        Receiver::QSelf { type_shape } => format!("<{} as ...>", shape_description(type_shape)),
    }
}

fn same_crate(left: &str, right: &str) -> bool {
    match (
        left.split('/')
            .nth(1)
            .filter(|_| left.starts_with("crates/")),
        right
            .split('/')
            .nth(1)
            .filter(|_| right.starts_with("crates/")),
    ) {
        (Some(left), Some(right)) => left == right,
        (None, None) => left == right,
        _ => false,
    }
}

fn route_identity(route: &HandImplSource) -> (String, Vec<String>, String) {
    (route.path.clone(), route.scope.clone(), route.name.clone())
}

/// Resolve an object receiver against its declaration or against a manual
/// reader whose complete input route is closed. A target with a denied local
/// declaration is not enough: its consumed reader must itself classify as a
/// closed wire route.
fn receiver_is_closed(
    route: &HandImplSource,
    receiver: &Receiver,
    index: &SourceIndex,
    active: &mut BTreeSet<(String, Vec<String>, String)>,
) -> bool {
    fn target_for_shape(
        route: &HandImplSource,
        shape: &TypeShape,
        index: &SourceIndex,
    ) -> Option<ResolvedPath> {
        match shape {
            TypeShape::Path(path) => Some(resolve_receiver_path(route, path, index)),
            TypeShape::Generic(path, _) => Some(resolve_receiver_path(route, path, index)),
            TypeShape::KeylessAggregate => None,
        }
    }

    let resolved = match receiver {
        Receiver::Path(path) => {
            if path.segments.len() == 1 && route.local_denied.contains(&path.segments[0]) {
                return true;
            }
            if path
                .segments
                .first()
                .is_some_and(|segment| segment.starts_with('$'))
            {
                let mut scope = route.scope.clone();
                scope.extend(
                    path.segments
                        .iter()
                        .take(path.segments.len().saturating_sub(1))
                        .cloned(),
                );
                let Some(name) = path.segments.last() else {
                    return false;
                };
                if index
                    .denied
                    .iter()
                    .any(|key| key.path == route.path && key.scope == scope && key.name == *name)
                {
                    return true;
                }
                let targets: Vec<&HandImplSource> = index
                    .sources
                    .iter()
                    .filter(|target| {
                        target.path == route.path && target.scope == scope && target.name == *name
                    })
                    .collect();
                if targets.len() != 1 {
                    return false;
                }
                return matches!(
                    classify_route_active(targets[0], index, active),
                    Ok(HandReaderClass::Wire)
                );
            }
            resolve_receiver_path(route, path, index)
        }
        Receiver::Generic { path, arguments } => {
            let resolved = resolve_receiver_path(route, path, index);
            if is_wrapper_path(&resolved) {
                if arguments.len() != 1 {
                    return false;
                }
                let Some(inner) = receiver_from_shape(&arguments[0]) else {
                    return false;
                };
                return receiver_is_closed(route, &inner, index, active);
            }
            resolved
        }
        Receiver::QSelf { type_shape } => {
            let Some(resolved) = target_for_shape(route, type_shape, index) else {
                return false;
            };
            resolved
        }
    };
    let ResolvedPath::Local(symbol) = resolved else {
        return false;
    };
    let denied = index
        .denied
        .iter()
        .any(|key| key.path == symbol.path && key.scope == symbol.scope && key.name == symbol.name);
    if denied {
        return true;
    }
    let targets: Vec<&HandImplSource> = index
        .sources
        .iter()
        .filter(|target| {
            target.path == symbol.path && target.scope == symbol.scope && target.name == symbol.name
        })
        .collect();
    targets.len() == 1
        && matches!(
            classify_route_active(targets[0], index, active),
            Ok(HandReaderClass::Wire)
        )
}

fn is_wrapper_path(resolved: &ResolvedPath) -> bool {
    let ResolvedPath::External(path) = resolved else {
        return false;
    };
    path.len() == 3
        && ((path[0] == "std" && path[1] == "boxed" && path[2] == "Box")
            || (path[0] == "core" && path[1] == "option" && path[2] == "Option")
            || (path[0] == "std" && path[1] == "rc" && path[2] == "Rc")
            || (path[0] == "std" && path[1] == "sync" && path[2] == "Arc")
            || (path[0] == "core" && path[1] == "pin" && path[2] == "Pin")
            || (path[0] == "std" && path[1] == "cell" && path[2] == "RefCell")
            || (path[0] == "std" && path[1] == "cell" && path[2] == "Cell")
            || (path[0] == "std" && path[1] == "sync" && path[2] == "Mutex")
            || (path[0] == "std" && path[1] == "sync" && path[2] == "RwLock"))
}

fn receiver_from_shape(shape: &TypeShape) -> Option<Receiver> {
    match shape {
        TypeShape::Path(path) => Some(Receiver::Path(path.clone())),
        TypeShape::Generic(path, arguments) => Some(Receiver::Generic {
            path: path.clone(),
            arguments: arguments.clone(),
        }),
        TypeShape::KeylessAggregate => None,
    }
}

fn macro_helper_route_at(
    nodes: &[Node],
    position: usize,
    bindings: &BTreeSet<String>,
) -> Option<InputRoute> {
    let path = path_tail(nodes, position + 1)?;
    let function = path.last()?.as_str();
    let known = match path.as_slice() {
        [prefix, module, name]
            if matches!(prefix.as_str(), "$crate" | "crate")
                && module == "units"
                && matches!(
                    name.as_str(),
                    "deserialize_named" | "deserialize_named_optional"
                ) =>
        {
            Some(InputRoute::Keyless)
        }
        [prefix, module, name]
            if matches!(prefix.as_str(), "$crate" | "crate")
                && module == "distinct_keys"
                && matches!(name.as_str(), "json_object" | "btree_map") =>
        {
            Some(InputRoute::FreeForm)
        }
        _ => None,
    }?;
    if !matches!(
        function,
        "deserialize_named" | "deserialize_named_optional" | "json_object" | "btree_map"
    ) {
        return None;
    }
    let Some(Node::Group(Delimiter::Parenthesis, arguments)) = nodes.get(position + 1) else {
        return None;
    };
    call_uses_deserializer(arguments, bindings).then_some(known)
}

fn macro_input_routes(
    route: &HandImplSource,
    index: &SourceIndex,
) -> Result<Vec<InputRoute>, String> {
    let bindings = &route.deserializer_bindings;
    let nodes = &route.method_nodes;
    let mut routes = Vec::new();
    fn collect(
        nodes: &[Node],
        route: &HandImplSource,
        index: &SourceIndex,
        bindings: &BTreeSet<String>,
        routes: &mut Vec<InputRoute>,
        blocked: bool,
    ) -> Result<(), String> {
        for position in 0..nodes.len() {
            if let Some(Node::Group(_, children)) = nodes.get(position) {
                let preceding = position
                    .checked_sub(1)
                    .and_then(|index| atom_opt(nodes.get(index)));
                if preceding == Some("!") && token_mentions_binding(children, bindings) {
                    return Err(format!(
                        "{} {} hides the deserializer input inside an unknown macro: {}",
                        route.path, route.name, route.body
                    ));
                }
            }
            if let Some(receiver) = deserialize_receiver_at(nodes, position, bindings) {
                if blocked {
                    return Err(format!(
                        "{} {} hides an input route inside unsupported macro syntax: {}",
                        route.path, route.name, route.body
                    ));
                }
                if !is_atom(nodes, position + 2, "?") {
                    return Err(format!(
                        "{} {} has a macro route whose error is not propagated: {}",
                        route.path, route.name, route.body
                    ));
                }
                routes.push(if receiver_is_keyless(route, &receiver, index) {
                    InputRoute::Keyless
                } else if receiver_is_value(route, &receiver, index) {
                    InputRoute::Value
                } else {
                    InputRoute::Wire(receiver)
                });
            }

            if let Some(input_route) = macro_helper_route_at(nodes, position, bindings) {
                if !is_atom(nodes, position + 2, "?") {
                    return Err(format!(
                        "{} {} has a macro helper route whose error is not propagated: {}",
                        route.path, route.name, route.body
                    ));
                }
                routes.push(input_route);
            }
            if let Some(Node::Group(_, children)) = nodes.get(position) {
                let preceding = position
                    .checked_sub(1)
                    .and_then(|index| atom_opt(nodes.get(index)));
                let child_blocked = blocked
                    || matches!(nodes.get(position), Some(Node::Group(Delimiter::Brace, _)))
                    || matches!(
                        preceding,
                        Some("!" | "if" | "match" | "for" | "while" | "loop")
                    );
                collect(children, route, index, bindings, routes, child_blocked)?;
            }
        }
        Ok(())
    }
    collect(nodes, route, index, bindings, &mut routes, false)?;
    if routes.is_empty() {
        return Err(format!(
            "{} {} has no direct macro deserializer route: {}",
            route.path, route.name, route.body
        ));
    }
    Ok(routes)
}

fn classify_route(route: &HandImplSource, index: &SourceIndex) -> Result<HandReaderClass, String> {
    let mut active = BTreeSet::new();
    classify_route_active(route, index, &mut active)
}

fn classify_route_active(
    route: &HandImplSource,
    index: &SourceIndex,
    active: &mut BTreeSet<(String, Vec<String>, String)>,
) -> Result<HandReaderClass, String> {
    if !active.insert(route_identity(route)) {
        return Err(format!(
            "{} {} has a recursive reader route",
            route.path, route.name
        ));
    }

    let result = classify_route_body(route, index, active);
    active.remove(&route_identity(route));
    result
}

fn classify_route_body(
    route: &HandImplSource,
    index: &SourceIndex,
    active: &mut BTreeSet<(String, Vec<String>, String)>,
) -> Result<HandReaderClass, String> {
    let (routes, version_gates, unsupported_reason) = if let Some(block) = &route.method_block {
        let mut environment = ScanEnvironment {
            input_bindings: route.deserializer_bindings.clone(),
            validated_values: BTreeSet::new(),
            version_check_shadowed: false,
            invalidated_values: BTreeSet::new(),
        };
        let mut scan = RouteScan::default();
        scan_block(
            block,
            route,
            index,
            &mut environment,
            &mut scan,
            false,
            true,
        );
        (scan.routes, scan.version_gates, scan.unsupported)
    } else {
        match macro_input_routes(route, index) {
            Ok(routes) => (routes, 0, None),
            Err(reason) => (Vec::new(), 0, Some(reason)),
        }
    };
    if let Some(reason) = unsupported_reason {
        return Err(reason);
    }
    if routes.is_empty() {
        return Err(format!(
            "{} {} has no deserializer-consuming call: {}",
            route.path, route.name, route.body
        ));
    }

    if routes
        .iter()
        .any(|route| matches!(route, InputRoute::FreeForm))
    {
        if routes
            .iter()
            .all(|route| matches!(route, InputRoute::FreeForm))
        {
            return Ok(HandReaderClass::FreeForm);
        }
        return Err(format!(
            "{} {} mixes an open map call with another input route: {}",
            route.path, route.name, route.body
        ));
    }

    if routes
        .iter()
        .any(|route| matches!(route, InputRoute::Value))
    {
        if routes
            .iter()
            .all(|route| matches!(route, InputRoute::Value))
            && routes.len() == 1
            && version_gates == 1
        {
            return Ok(HandReaderClass::ValidatedValue);
        }
        return Err(format!(
            "{} {} reads a general JSON value without one propagated check_ir_version call: {}",
            route.path, route.name, route.body
        ));
    }

    if routes
        .iter()
        .any(|route| matches!(route, InputRoute::Keyless))
    {
        if routes
            .iter()
            .all(|route| matches!(route, InputRoute::Keyless))
        {
            return Ok(HandReaderClass::Keyless);
        }
        return Err(format!(
            "{} {} mixes a keyless input route with an object route: {}",
            route.path, route.name, route.body
        ));
    }

    for input_route in routes {
        let InputRoute::Wire(receiver) = input_route else {
            continue;
        };
        if !receiver_is_closed(route, &receiver, index, active) {
            return Err(format!(
                "{} {} reads unresolved or open object target {} in scope {:?}: {}",
                route.path,
                route.name,
                receiver_description(&receiver),
                route.scope,
                route.body
            ));
        }
    }
    Ok(HandReaderClass::Wire)
}

/// Classify a fixture body through the same call parser as a source route.
fn classify_hand_reader(
    path: &str,
    name: &str,
    body: &str,
    denied_types: &BTreeSet<String>,
    locally_closed: &BTreeSet<String>,
) -> Result<HandReaderClass, String> {
    classify_hand_reader_with_bindings(
        path,
        name,
        body,
        &BTreeSet::from(["deserializer".to_owned()]),
        denied_types,
        locally_closed,
    )
}

fn classify_hand_reader_with_bindings(
    path: &str,
    name: &str,
    body: &str,
    deserializer_bindings: &BTreeSet<String>,
    denied_types: &BTreeSet<String>,
    locally_closed: &BTreeSet<String>,
) -> Result<HandReaderClass, String> {
    let method_nodes = tokenize_block_text(path, name, &format!("{{{body}}}"));
    let mut index = SourceIndex::default();
    for denied in denied_types {
        insert_wire(&mut index.denied, path, &[], denied.clone());
    }
    let mut local_denied = locally_closed.clone();
    let block: syn::Block = syn::parse_str(&format!("{{{body}}}"))
        .map_err(|error| format!("fixture {path} {name} does not parse: {error}"))?;
    local_denied.extend(local_denied_types(&block));
    let route_path = if body.contains("check_ir_version") {
        // Unit fixtures do not live in the scanned crate. Give their
        // synthetic declaration the same owner path as the production helper
        // so the ownership predicate remains exact rather than accepting any
        // same-file function named `check_ir_version`.
        "crates/cadmpeg-ir/src/document.rs"
    } else {
        path
    };
    if body.contains("check_ir_version") {
        insert_symbol(
            &mut index,
            route_path,
            &[],
            "check_ir_version".to_owned(),
            SymbolKind::Function,
        );
    }
    let route = HandImplSource {
        path: route_path.to_owned(),
        scope: Vec::new(),
        name: name.to_owned(),
        body: body.to_owned(),
        method_nodes,
        method_block: Some(block.clone()),
        deserializer_bindings: deserializer_bindings.clone(),
        local_denied,
    };
    classify_route(&route, &index)
}

/// Compare each listed class to the route found in the implementation body.
///
/// This is the executable contract for the table: changing `NativeRecord`
/// from `free-form` to `wire`, removing a wire's denial attribute, or adding a
/// new reader shape makes this test fail at the changed source location.
pub(super) fn assert_hand_written_reader_routes() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the repository root sits two levels above the crate manifest")
        .to_path_buf();
    let index = source_index(&root);
    assert!(
        !index.sources.is_empty(),
        "the route census found no reader bodies"
    );
    assert!(
        !index.denied.is_empty(),
        "the route census found no denied wire declarations"
    );
    let mut found = Vec::new();
    for source in &index.sources {
        let class = classify_route(source, &index).unwrap_or_else(|reason| panic!("{reason}"));
        found.push((
            source.path.clone(),
            source.name.clone(),
            class.as_str().to_owned(),
        ));
    }
    let mut listed: Vec<(String, String, String)> = HAND_IMPLS
        .iter()
        .map(|(path, name, class)| ((*path).to_owned(), (*name).to_owned(), (*class).to_owned()))
        .collect();
    found.sort();
    listed.sort();
    assert_eq!(
        found, listed,
        "hand-written reader route classes disagree with HAND_IMPLS"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reader_route_contract_rejects_unclosed_object_routes() {
        let denied = BTreeSet::from(["ClosedWire".to_owned()]);
        let no_local_wire = BTreeSet::new();
        assert_eq!(
            classify_hand_reader(
                "fixture.rs",
                "Scalar",
                "String::deserialize(deserializer)?",
                &denied,
                &no_local_wire,
            ),
            Ok(HandReaderClass::Keyless)
        );
        assert_eq!(
            classify_hand_reader(
                "fixture.rs",
                "Closed",
                "ClosedWire::deserialize(deserializer)?",
                &denied,
                &no_local_wire,
            ),
            Ok(HandReaderClass::Wire)
        );
        assert_eq!(
            classify_hand_reader(
                "fixture.rs",
                "LocalClosed",
                "#[serde(deny_unknown_fields)] struct Wire {} Wire::deserialize(deserializer)?",
                &denied,
                &no_local_wire,
            ),
            Ok(HandReaderClass::Wire)
        );
        assert!(classify_hand_reader(
            "fixture.rs",
            "LocalOpen",
            "struct Wire {} Wire::deserialize(deserializer)?",
            &denied,
            &no_local_wire,
        )
        .is_err());
        assert!(classify_hand_reader(
            "fixture.rs",
            "CommentAndLiteral",
            "// ClosedWire::deserialize\nlet marker = \"deny_unknown_fields\";",
            &denied,
            &no_local_wire,
        )
        .is_err());
        assert!(classify_hand_reader(
            "fixture.rs",
            "LiteralRoute",
            "let marker = \"ClosedWire::deserialize\";",
            &denied,
            &no_local_wire,
        )
        .is_err());
        assert!(classify_hand_reader(
            "fixture.rs",
            "Open",
            "OpenWire::deserialize(deserializer)?",
            &denied,
            &no_local_wire,
        )
        .is_err());
        assert_eq!(
            classify_hand_reader(
                "fixture.rs",
                "OpenMap",
                "cadmpeg_core::distinct_keys::json_object(deserializer)?",
                &denied,
                &no_local_wire,
            ),
            Ok(HandReaderClass::FreeForm)
        );
        assert_eq!(
            classify_hand_reader(
                "fixture.rs",
                "Version",
                "let version = serde_json::Value::deserialize(deserializer)?; check_ir_version(Some(&version))?;",
                &denied,
                &no_local_wire,
            ),
            Ok(HandReaderClass::ValidatedValue)
        );
    }

    #[test]
    fn lexical_markers_without_owned_calls_do_not_prove_a_route() {
        let denied = BTreeSet::from(["Other".to_owned()]);
        let no_local_wire = BTreeSet::new();
        assert!(classify_hand_reader(
            "fixture.rs",
            "UnrelatedBox",
            "let _box = Box::<Thing>; OpenWire::deserialize(deserializer)?",
            &denied,
            &no_local_wire,
        )
        .is_err());
        assert!(classify_hand_reader(
            "fixture.rs",
            "UnattachedDeny",
            "let deny_unknown_fields = true; OpenWire::deserialize(deserializer)?",
            &denied,
            &no_local_wire,
        )
        .is_err());
        assert!(classify_hand_reader(
            "fixture.rs",
            "BareVersionCheck",
            "serde_json::Value::deserialize(deserializer)?; let _ = check_ir_version;",
            &denied,
            &no_local_wire,
        )
        .is_err());
        assert!(classify_hand_reader(
            "fixture.rs",
            "UnpropagatedVersionCheck",
            "serde_json::Value::deserialize(deserializer)?; check_ir_version(None);",
            &denied,
            &no_local_wire,
        )
        .is_err());
        assert!(classify_hand_reader(
            "fixture.rs",
            "DisconnectedGeneric",
            "let _ = Other::<Thing>; OpenWire::deserialize(deserializer)?",
            &denied,
            &no_local_wire,
        )
        .is_err());
    }

    #[test]
    fn receiver_shapes_and_input_bindings_are_load_bearing() {
        let denied = BTreeSet::new();
        let no_local_wire = BTreeSet::new();
        for (name, body) in [
            ("BoxObject", "Box::<Open>::deserialize(deserializer)?;"),
            (
                "QSelfObject",
                "<Open as Deserialize>::deserialize(deserializer)?;",
            ),
            (
                "QualifiedScalar",
                "hostile::String::deserialize(deserializer)?;",
            ),
        ] {
            assert!(
                classify_hand_reader("fixture.rs", name, body, &denied, &no_local_wire,).is_err(),
                "{name} must not inherit a keyless basename contract",
            );
        }

        assert_eq!(
            classify_hand_reader(
                "fixture.rs",
                "QSelfScalar",
                "<String as Deserialize>::deserialize(deserializer)?;",
                &denied,
                &no_local_wire,
            ),
            Ok(HandReaderClass::Keyless)
        );
        assert_eq!(
            classify_hand_reader(
                "fixture.rs",
                "QSelfArray",
                "<[f64; 2]>::deserialize(deserializer)?;",
                &denied,
                &no_local_wire,
            ),
            Ok(HandReaderClass::Keyless)
        );
        assert_eq!(
            classify_hand_reader(
                "fixture.rs",
                "BoxArray",
                "Box::<[u8; 3]>::deserialize(deserializer)?;",
                &denied,
                &no_local_wire,
            ),
            Ok(HandReaderClass::Keyless)
        );

        let input = BTreeSet::from(["input".to_owned()]);
        assert!(
            classify_hand_reader_with_bindings(
                "fixture.rs",
                "ShadowedInput",
                "let deserializer = make_input(); String::deserialize(deserializer)?;",
                &input,
                &denied,
                &no_local_wire,
            )
            .is_err(),
            "a local variable named deserializer is not the method input",
        );
        assert!(
            classify_hand_reader(
                "fixture.rs",
                "NestedInputMention",
                "String::deserialize(make_input(deserializer))?;",
                &denied,
                &no_local_wire,
            )
            .is_err(),
            "a nested expression is not a direct deserializer binding",
        );
    }

    #[test]
    fn version_check_must_consume_a_deserialized_value() {
        let denied = BTreeSet::new();
        let no_local_wire = BTreeSet::new();
        assert_eq!(
            classify_hand_reader(
                "fixture.rs",
                "Version",
                "let version = serde_json::Value::deserialize(deserializer)?; check_ir_version(Some(&version))?;",
                &denied,
                &no_local_wire,
            ),
            Ok(HandReaderClass::ValidatedValue)
        );
        assert!(
            classify_hand_reader(
                "fixture.rs",
                "UnrelatedVersion",
                "let _ = serde_json::Value::deserialize(deserializer)?; check_ir_version(Some(&serde_json::Value::from(6)))?;",
                &denied,
                &no_local_wire,
            )
            .is_err(),
            "a check over unrelated data does not validate the consumed value",
        );
        for body in [
            "let value = serde_json::Value::deserialize(deserializer)?; let moved = value; check_ir_version(Some(&value))?;",
            "let value = serde_json::Value::deserialize(deserializer)?; drop(value); check_ir_version(Some(&value))?;",
            "let value = serde_json::Value::deserialize(deserializer)?; inspect(value); check_ir_version(Some(&value))?;",
            "let first = serde_json::Value::deserialize(deserializer)?; let second = serde_json::Value::deserialize(deserializer)?; check_ir_version(Some(&first))?;",
            "fn check_ir_version<E: serde::de::Error>(_: Option<&serde_json::Value>) -> Result<(), E> { Ok(()) } let value = serde_json::Value::deserialize(deserializer)?; check_ir_version(Some(&value))?;",
            "let check_ir_version = replacement; let value = serde_json::Value::deserialize(deserializer)?; check_ir_version(Some(&value))?;",
        ] {
            assert!(
                classify_hand_reader("fixture.rs", "Reader", body, &denied, &no_local_wire)
                    .is_err(),
                "a moved, duplicated, or shadowed validation value does not prove a version gate: {body}",
            );
        }
    }

    #[test]
    fn reused_input_names_do_not_prove_which_value_was_read() {
        let denied = BTreeSet::new();
        let no_local_wire = BTreeSet::new();
        for body in [
            "let saved = deserializer; let deserializer = replacement(); String::deserialize(deserializer)?; read_open(saved)?;",
            "let saved = deserializer; let (deserializer,) = (replacement(),); String::deserialize(deserializer)?; read_open(saved)?;",
            "let saved = deserializer; let read = |deserializer| String::deserialize(deserializer); read(replacement())?; read_open(saved)?;",
        ] {
            assert!(
                classify_hand_reader("fixture.rs", "Reader", body, &denied, &no_local_wire)
                    .is_err(),
                "a reused parameter name does not identify the consumed input: {body}",
            );
        }
    }

    #[test]
    fn deserialization_errors_and_execution_paths_are_load_bearing() {
        let denied = BTreeSet::new();
        let no_local_wire = BTreeSet::new();
        for (name, body) in [
            (
                "AssignedIf",
                "let _ = if false { Some(u8::deserialize(deserializer)?) } else { None };",
            ),
            (
                "AssignedMatch",
                "let _ = match false { true => Some(u8::deserialize(deserializer)?), false => None };",
            ),
            (
                "EarlyReturn",
                "return Ok(Self); let _ = u8::deserialize(deserializer)?;",
            ),
            ("Discarded", "let _ = u8::deserialize(deserializer);"),
            (
                "DefaultOnError",
                "let _ = u8::deserialize(deserializer).unwrap_or_default();",
            ),
            (
                "ErrorToOption",
                "let _ = u8::deserialize(deserializer).ok();",
            ),
            (
                "ErrorToSuccess",
                "u8::deserialize(deserializer).or_else(|_| Ok(0)).map(|_| Self)",
            ),
            (
                "ShortCircuit",
                "let _ = true || bool::deserialize(deserializer)?;",
            ),
            (
                "UnknownInputCall",
                "u8::deserialize(deserializer)?; inspect(deserializer)?;",
            ),
            (
                "OpaqueInputMacro",
                "u8::deserialize(deserializer)?; inspect!(deserializer);",
            ),
        ] {
            assert!(
                classify_hand_reader("fixture.rs", name, body, &denied, &no_local_wire).is_err(),
                "{name} must not certify a route whose execution or error is unproved",
            );
        }
        assert!(
            classify_hand_reader(
                "fixture.rs",
                "DiscardedMacro",
                "discard!(u8::deserialize(deserializer)?);",
                &denied,
                &no_local_wire,
            )
            .is_err(),
            "a deserializer call passed to an opaque macro cannot certify a route",
        );

        assert_eq!(
            classify_hand_reader(
                "fixture.rs",
                "Propagated",
                "u8::deserialize(deserializer)?;",
                &denied,
                &no_local_wire,
            ),
            Ok(HandReaderClass::Keyless)
        );
        assert!(
            classify_hand_reader(
                "fixture.rs",
                "UnknownInputMethod",
                "u8::deserialize(deserializer)?; deserializer.deserialize_seq(visitor)?;",
                &denied,
                &no_local_wire,
            )
            .is_err(),
            "an unknown method receiving the deserializer cannot be ignored",
        );
        assert_eq!(
            classify_hand_reader(
                "fixture.rs",
                "Returned",
                "return u8::deserialize(deserializer)?;",
                &denied,
                &no_local_wire,
            ),
            Ok(HandReaderClass::Keyless)
        );
        assert_eq!(
            classify_hand_reader(
                "fixture.rs",
                "ForIterable",
                "for _ in Vec::<u8>::deserialize(deserializer)? {}",
                &denied,
                &no_local_wire,
            ),
            Ok(HandReaderClass::Keyless),
            "an unconditional for-loop iterable propagates its direct input route"
        );
    }

    fn classify_fixture_reader(source: &str) -> Result<HandReaderClass, String> {
        let parsed = syn::parse_file(source).map_err(|error| error.to_string())?;
        let mut index = SourceIndex::default();
        collect_source_items(&parsed.items, "fixture.rs", source, &[], &mut index);
        let reader = index
            .sources
            .iter()
            .find(|route| route.name == "Reader")
            .ok_or_else(|| "fixture has no Reader implementation".to_owned())?;
        classify_route(reader, &index)
    }

    fn open_reader_fixture(prefix: &str, route: &str) -> String {
        format!(
            r#"
                {prefix}
                struct Open;
                impl<'de> serde::Deserialize<'de> for Open {{
                    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {{
                        let _ = serde_json::Value::deserialize(d)?;
                        Ok(Self)
                    }}
                }}
                struct Reader;
                impl<'de> serde::Deserialize<'de> for Reader {{
                    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {{
                        {route}
                    }}
                }}
            "#
        )
    }

    #[test]
    fn lexical_imports_resolve_standard_and_hostile_names() {
        let positive = [
            "use std::string::String as Scalar;",
            "use std as standard;",
            "use std as standard; use standard::string::String as Scalar;",
        ];
        let routes = [
            "Scalar::deserialize(d)?;",
            "standard::string::String::deserialize(d)?;",
            "Scalar::deserialize(d)?;",
        ];
        for (prefix, route) in positive.into_iter().zip(routes) {
            let source = if route.starts_with("Scalar") && prefix == "use std as standard;" {
                open_reader_fixture(prefix, "standard::string::String::deserialize(d)?;")
            } else {
                open_reader_fixture(prefix, route)
            };
            assert_eq!(
                classify_fixture_reader(&source),
                Ok(HandReaderClass::Keyless),
                "standard import route must retain its keyless contract: {prefix} {route}"
            );
        }
        let absolute = open_reader_fixture("mod std {}", "::std::string::String::deserialize(d)?;");
        assert_eq!(
            classify_fixture_reader(&absolute),
            Ok(HandReaderClass::Keyless),
            "an absolute standard path is not shadowed by a local module"
        );
        let absolute_alias = open_reader_fixture(
            "use std::string::String as Hostile;",
            "::Hostile::deserialize(d)?;",
        );
        assert!(
            classify_fixture_reader(&absolute_alias).is_err(),
            "an absolute path cannot inherit a lexical alias from the current module"
        );

        let hostile = [
            ("use hostile::String as Scalar;", "Scalar::deserialize(d)?;"),
            ("use hostile::*;", "String::deserialize(d)?;"),
            ("use hostile::{*};", "String::deserialize(d)?;"),
            ("use hostile as std;", "std::String::deserialize(d)?;"),
            (
                "use hostile as std; use std::String as Scalar;",
                "Scalar::deserialize(d)?;",
            ),
        ];
        for (prefix, route) in hostile {
            let source = open_reader_fixture(
                &format!("mod hostile {{ pub use super::Open as String; }} {prefix}"),
                route,
            );
            assert!(
                classify_fixture_reader(&source).is_err(),
                "a hostile alias must not inherit the standard keyless basename: {prefix} {route}"
            );
        }

        let nested = open_reader_fixture(
            "mod hostile { pub mod string { pub use crate::Open as String; } } use hostile as std;",
            "std::string::String::deserialize(d)?;",
        );
        assert!(
            classify_fixture_reader(&nested).is_err(),
            "module aliases must resolve through their nested re-export"
        );
        let chained = open_reader_fixture(
            "mod hostile { pub mod string { pub use crate::Open as String; } } use hostile as std; use std::string::String as Scalar;",
            "Scalar::deserialize(d)?;",
        );
        assert!(
            classify_fixture_reader(&chained).is_err(),
            "alias chains must retain the hostile target identity"
        );
    }

    #[test]
    fn helper_routes_resolve_the_declared_function_owner() {
        for (prefix, route, class) in [
            (
                "use cadmpeg_core::bytes::deserialize as read_bytes;",
                "read_bytes(d)?;",
                HandReaderClass::Keyless,
            ),
            (
                "use cadmpeg_core::distinct_keys::json_object as read_object;",
                "read_object(d)?;",
                HandReaderClass::FreeForm,
            ),
        ] {
            assert_eq!(
                classify_fixture_reader(&open_reader_fixture(prefix, route)),
                Ok(class),
                "qualified helper imports retain their owned route: {prefix}"
            );
        }

        let hostile = open_reader_fixture(
            "mod hostile { pub fn deserialize<T>(_: T) -> Result<(), ()> { Ok(()) } } use hostile::deserialize as read;",
            "read(d)?;",
        );
        assert!(
            classify_fixture_reader(&hostile).is_err(),
            "a local function with a helper basename cannot inherit a codec route"
        );
    }

    #[test]
    fn version_gate_must_directly_check_the_consumed_value() {
        let denied = BTreeSet::new();
        let no_local_wire = BTreeSet::new();
        for body in [
            "let value = serde_json::Value::deserialize(deserializer)?; let _old = value; let value = replacement(); check_ir_version(Some(&value))?;",
            "let mut value = serde_json::Value::deserialize(deserializer)?; value = replacement(); check_ir_version(Some(&value))?;",
            "let value = serde_json::Value::deserialize(deserializer)?; check_ir_version((Some(&value), Some(&replacement())).1)?;",
            "let value = serde_json::Value::deserialize(deserializer)?; check_ir_version(Some(&{ drop(value); replacement() }))?;",
            "let value = serde_json::Value::deserialize(deserializer)?; let unchecked = || check_ir_version(Some(&value));",
            "let value = serde_json::Value::deserialize(deserializer)?; if false { check_ir_version(Some(&value))?; }",
            "let value = serde_json::Value::deserialize(deserializer)?; mutate(&mut value); check_ir_version(Some(&value))?;",
        ] {
            assert!(
                classify_hand_reader("fixture.rs", "Reader", body, &denied, &no_local_wire)
                    .is_err(),
                "a later mention does not prove validation of the consumed value: {body}",
            );
        }
    }

    #[test]
    fn manual_target_must_have_a_closed_consumed_route() {
        let source = r#"
            struct Open;
            impl<'de> serde::Deserialize<'de> for Open {
                fn deserialize<D: serde::Deserializer<'de>>(deserializer: D)
                    -> Result<Self, D::Error>
                {
                    #[serde(deny_unknown_fields)]
                    struct UnrelatedClosed;
                    let _ = serde_json::Value::deserialize(deserializer)?;
                    Ok(Self)
                }
            }
            struct Reader;
            impl<'de> serde::Deserialize<'de> for Reader {
                fn deserialize<D: serde::Deserializer<'de>>(deserializer: D)
                    -> Result<Self, D::Error>
                {
                    Open::deserialize(deserializer)?;
                    Ok(Self)
                }
            }
        "#;
        let parsed = syn::parse_file(source).expect("parse manual target fixture");
        let mut index = SourceIndex::default();
        collect_source_items(&parsed.items, "fixture.rs", source, &[], &mut index);
        let reader = index
            .sources
            .iter()
            .find(|route| route.name == "Reader")
            .expect("collect outer manual reader");
        assert!(
            classify_route(reader, &index).is_err(),
            "an unrelated local denied declaration cannot close an open target",
        );
    }

    #[test]
    fn scoped_wire_names_do_not_cross_module_boundaries() {
        let route = HandImplSource {
            path: "fixture.rs".to_owned(),
            scope: vec!["module_b".to_owned()],
            name: "Reader".to_owned(),
            body: "Wire::deserialize(deserializer)?".to_owned(),
            method_nodes: tokenize_block_text(
                "fixture.rs",
                "Reader",
                "{Wire::deserialize(deserializer)?}",
            ),
            method_block: None,
            deserializer_bindings: BTreeSet::from(["deserializer".to_owned()]),
            local_denied: BTreeSet::new(),
        };
        let mut index = SourceIndex::default();
        insert_wire(
            &mut index.denied,
            "fixture.rs",
            &["module_a".to_owned()],
            "Wire".to_owned(),
        );
        assert!(classify_route(&route, &index).is_err());
    }

    #[test]
    fn exact_spans_and_macro_routes_are_isolated() {
        let source = "impl<'de> serde::Deserialize<'de> for First { fn deserialize<D>(d: D) -> Result<Self, D::Error> { todo!() } } impl<'de> serde::Deserialize<'de> for Second { fn deserialize<D>(d: D) -> Result<Self, D::Error> { todo!() } }";
        let file: syn::File = syn::parse_str(source).expect("parse adjacent impls");
        let first = match &file.items[0] {
            syn::Item::Impl(item) => item,
            _ => panic!("first item is not an impl"),
        };
        let first_text = source_span_text(source, first.span());
        assert!(first_text.contains("First"));
        assert!(!first_text.contains("Second"));

        let macro_file: syn::File = syn::parse_str(
            r#"macro_rules! readers {
                ($name:ident) => {
                    impl Serialize for $name { fn serialize<S>(&self, s: S) -> Result<S::Ok, S::Error> { todo!() } }
                    impl<'de> serde::Deserialize<'de> for $name {
                        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> {
                            String::deserialize(deserializer)?
                        }
                    }
                };
            }"#,
        )
        .expect("parse macro route fixture");
        let syn::Item::Macro(item) = &macro_file.items[0] else {
            panic!("fixture item is not a macro");
        };
        let nodes = nodes_from_stream(&item.mac.tokens);
        let mut routes = Vec::new();
        collect_macro_routes(&nodes, "fixture.rs", &[], &mut routes);
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].name, "$name");
        assert!(!routes[0].body.contains("serialize<S>"));
        assert_eq!(
            classify_route(&routes[0], &SourceIndex::default()),
            Ok(HandReaderClass::Keyless)
        );

        let conditional_macro: syn::File = syn::parse_str(
            r#"macro_rules! conditional_reader {
                ($name:ident) => {
                    impl<'de> serde::Deserialize<'de> for $name {
                        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> {
                            if false {
                                String::deserialize(deserializer)?;
                            }
                            Ok(Self)
                        }
                    }
                };
            }"#,
        )
        .expect("parse conditional macro route fixture");
        let syn::Item::Macro(item) = &conditional_macro.items[0] else {
            panic!("conditional fixture item is not a macro");
        };
        let nodes = nodes_from_stream(&item.mac.tokens);
        let mut routes = Vec::new();
        collect_macro_routes(&nodes, "fixture.rs", &[], &mut routes);
        assert_eq!(routes.len(), 1);
        assert!(
            classify_route(&routes[0], &SourceIndex::default()).is_err(),
            "a route in a macro conditional cannot certify the generated reader"
        );

        let opaque_macro: syn::File = syn::parse_str(
            r#"macro_rules! opaque_reader {
                ($name:ident) => {
                    impl<'de> serde::Deserialize<'de> for $name {
                        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> {
                            inspect!(deserializer);
                            String::deserialize(deserializer)?
                        }
                    }
                };
            }"#,
        )
        .expect("parse opaque macro route fixture");
        let syn::Item::Macro(item) = &opaque_macro.items[0] else {
            panic!("opaque fixture item is not a macro");
        };
        let nodes = nodes_from_stream(&item.mac.tokens);
        let mut routes = Vec::new();
        collect_macro_routes(&nodes, "fixture.rs", &[], &mut routes);
        assert_eq!(routes.len(), 1);
        assert!(
            classify_route(&routes[0], &SourceIndex::default()).is_err(),
            "an unknown macro receiving the deserializer cannot be ignored"
        );
    }
}
