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
#[derive(Debug)]
struct HandImplSource {
    path: String,
    scope: Vec<String>,
    name: String,
    body: String,
    method_nodes: Vec<Node>,
    local_denied: BTreeSet<String>,
}

#[derive(Debug, Default)]
struct SourceIndex {
    sources: Vec<HandImplSource>,
    denied: BTreeSet<WireKey>,
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

/// Parse one macro `impl` and isolate its `Deserialize::deserialize` method.
fn macro_route_at(nodes: &[Node], start: usize) -> Option<(String, Vec<Node>)> {
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
    let Node::Group(Delimiter::Brace, body) = implementation.get(body_index)? else {
        return None;
    };
    Some((name, body.clone()))
}

fn collect_macro_routes(
    nodes: &[Node],
    path: &str,
    scope: &[String],
    found: &mut Vec<HandImplSource>,
) {
    let mut index = 0;
    while index < nodes.len() {
        if let Some((name, method_nodes)) = macro_route_at(nodes, index) {
            let body = nodes_text(&method_nodes);
            found.push(HandImplSource {
                path: path.to_owned(),
                scope: scope.to_owned(),
                name,
                body,
                method_nodes,
                local_denied: BTreeSet::new(),
            });
        }
        if let Some(Node::Group(_, children)) = nodes.get(index) {
            collect_macro_routes(children, path, scope, found);
        }
        index += 1;
    }
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
            syn::Item::Struct(declaration)
                if serde_has_flag(&declaration.attrs, "deny_unknown_fields") =>
            {
                insert_wire(
                    &mut index.denied,
                    path,
                    scope,
                    declaration.ident.to_string(),
                );
            }
            syn::Item::Enum(declaration)
                if serde_has_flag(&declaration.attrs, "deny_unknown_fields") =>
            {
                insert_wire(
                    &mut index.denied,
                    path,
                    scope,
                    declaration.ident.to_string(),
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
                    local_denied: local_denied_types(&method.block),
                });
            }
            syn::Item::Mod(module) => {
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
        match atom(nodes.get(index)?) {
            Some(">") => depth += 1,
            Some("<") => {
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

#[derive(Debug, Clone)]
enum Receiver {
    Path(Vec<String>),
    Generic(Vec<String>),
    QSelf,
}

fn receiver_before(nodes: &[Node], end: usize) -> Option<Receiver> {
    if end == 0 {
        return None;
    }
    if is_atom(nodes, end - 1, ">") {
        let open = matching_angle_open(nodes, end - 1)?;
        if open >= 2 && is_atom(nodes, open - 1, ":") && is_atom(nodes, open - 2, ":") {
            return Some(Receiver::Generic(path_tail(nodes, open - 2)?));
        }
        if open > 0 {
            if let Some(path) = path_tail(nodes, open) {
                return Some(Receiver::Generic(path));
            }
        }
        return Some(Receiver::QSelf);
    }
    Some(Receiver::Path(path_tail(nodes, end)?))
}

#[derive(Debug)]
enum InputRoute {
    FreeForm,
    Value,
    Keyless,
    Wire(Receiver),
}

fn contains_atom(nodes: &[Node], wanted: &str) -> bool {
    nodes.iter().any(|node| match node {
        Node::Atom(value) => value == wanted,
        Node::Group(_, children) => contains_atom(children, wanted),
    })
}

fn call_uses_deserializer(arguments: &[Node]) -> bool {
    contains_atom(arguments, "deserializer")
}

fn receiver_is_keyless(receiver: &Receiver) -> bool {
    match receiver {
        Receiver::QSelf => true,
        Receiver::Generic(path) | Receiver::Path(path) => path.last().is_some_and(|name| {
            matches!(
                name.as_str(),
                "String" | "f64" | "i64" | "u32" | "Vec" | "Box" | "$raw"
            ) || path.ends_with(&["crate".to_owned(), "bytes".to_owned()])
        }),
    }
}

fn receiver_is_value(receiver: &Receiver) -> bool {
    matches!(receiver, Receiver::Path(path) if path.ends_with(&["serde_json".to_owned(), "Value".to_owned()]))
}

fn collect_input_routes(nodes: &[Node], routes: &mut Vec<InputRoute>) {
    for index in 0..nodes.len() {
        if is_atom(nodes, index, "deserialize")
            && index >= 2
            && is_atom(nodes, index - 2, ":")
            && is_atom(nodes, index - 1, ":")
        {
            if let Some(Node::Group(Delimiter::Parenthesis, arguments)) = nodes.get(index + 1) {
                if call_uses_deserializer(arguments) {
                    let Some(receiver) = receiver_before(nodes, index - 2) else {
                        continue;
                    };
                    if receiver_is_keyless(&receiver) {
                        routes.push(InputRoute::Keyless);
                    } else if receiver_is_value(&receiver) {
                        routes.push(InputRoute::Value);
                    } else {
                        routes.push(InputRoute::Wire(receiver));
                    }
                }
            }
        }

        if is_atom(nodes, index, "deserialize_any")
            && index >= 2
            && is_atom(nodes, index - 1, ".")
            && is_atom(nodes, index - 2, "deserializer")
            && matches!(
                nodes.get(index + 1),
                Some(Node::Group(Delimiter::Parenthesis, _))
            )
        {
            routes.push(InputRoute::FreeForm);
        }

        if (is_atom(nodes, index, "json_object") || is_atom(nodes, index, "btree_map"))
            && matches!(
                nodes.get(index + 1),
                Some(Node::Group(Delimiter::Parenthesis, _))
            )
            && index >= 2
            && is_atom(nodes, index - 1, ":")
            && path_tail(nodes, index - 2)
                .is_some_and(|path| path.last().is_some_and(|name| name == "distinct_keys"))
            && matches!(nodes.get(index + 1), Some(Node::Group(_, arguments)) if call_uses_deserializer(arguments))
        {
            routes.push(InputRoute::FreeForm);
        }

        if (is_atom(nodes, index, "deserialize_named")
            || is_atom(nodes, index, "deserialize_named_optional"))
            && matches!(nodes.get(index + 1), Some(Node::Group(Delimiter::Parenthesis, arguments)) if call_uses_deserializer(arguments))
        {
            routes.push(InputRoute::Keyless);
        }

        if let Some(Node::Group(_, children)) = nodes.get(index) {
            collect_input_routes(children, routes);
        }
    }
}

fn propagated_version_check(nodes: &[Node]) -> (bool, bool) {
    let mut called = false;
    let mut propagated = false;
    for index in 0..nodes.len() {
        if is_atom(nodes, index, "check_ir_version") {
            if let Some(Node::Group(Delimiter::Parenthesis, _)) = nodes.get(index + 1) {
                called = true;
                propagated |= is_atom(nodes, index + 2, "?");
            }
        }
        if let Some(Node::Group(_, children)) = nodes.get(index) {
            let (nested_called, nested_propagated) = propagated_version_check(children);
            called |= nested_called;
            propagated |= nested_propagated;
        }
    }
    (called, propagated)
}

fn receiver_description(receiver: &Receiver) -> String {
    match receiver {
        Receiver::Path(path) | Receiver::Generic(path) => path.join("::"),
        Receiver::QSelf => "<qself>".to_owned(),
    }
}

fn same_crate(left: &str, right: &str) -> bool {
    left.split('/').nth(1) == right.split('/').nth(1)
}

fn wire_key_matches_route_scope(route: &HandImplSource, key: &WireKey) -> bool {
    same_crate(&route.path, &key.path) && key.scope == route.scope
}

/// Resolve an object receiver against its declaration or against a local
/// manual reader whose own route is closed. Qualified `crate::` references are
/// resolved by module scope; a basename in another crate or unrelated module
/// never supplies a proof. A re-export from a parent module is accepted only
/// when the qualified scope has one matching declaration/reader.
fn receiver_is_closed(route: &HandImplSource, receiver: &Receiver, index: &SourceIndex) -> bool {
    let (Receiver::Path(path) | Receiver::Generic(path)) = receiver else {
        return false;
    };
    let Some(name) = path.last() else {
        return false;
    };

    if path.len() == 1 && route.local_denied.contains(name) {
        return true;
    }

    if path.len() == 1 {
        return index.denied.iter().any(|key| {
            key.name == *name && wire_key_matches_route_scope(route, key) && key.path == route.path
        }) || index.sources.iter().any(|target| {
            target.name == *name
                && target.path == route.path
                && target.scope == route.scope
                && !target.local_denied.is_empty()
        });
    }

    // Macro-generated `$wire::Wire` modules live below the macro's lexical
    // module and have no Rust crate-qualified path.
    if path.first().is_some_and(|segment| segment.starts_with('$')) {
        let mut scope = route.scope.clone();
        scope.extend(path.iter().take(path.len() - 1).cloned());
        return index
            .denied
            .iter()
            .any(|key| key.path == route.path && key.scope == scope && key.name == *name);
    }

    if path.first().is_some_and(|segment| segment == "crate") {
        let module_scope = &path[1..path.len() - 1];
        let declarations: Vec<&WireKey> = index
            .denied
            .iter()
            .filter(|key| {
                same_crate(&route.path, &key.path)
                    && key.name == *name
                    && key.scope.starts_with(module_scope)
            })
            .collect();
        if declarations.len() == 1 {
            return true;
        }
        let readers: Vec<&HandImplSource> = index
            .sources
            .iter()
            .filter(|target| {
                same_crate(&route.path, &target.path)
                    && target.name == *name
                    && target.scope.starts_with(module_scope)
                    && !target.local_denied.is_empty()
            })
            .collect();
        return readers.len() == 1;
    }

    false
}

fn classify_route(route: &HandImplSource, index: &SourceIndex) -> Result<HandReaderClass, String> {
    let mut routes = Vec::new();
    collect_input_routes(&route.method_nodes, &mut routes);
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
        let (called, propagated) = propagated_version_check(&route.method_nodes);
        if routes
            .iter()
            .all(|route| matches!(route, InputRoute::Value))
            && called
            && propagated
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
        if !receiver_is_closed(route, &receiver, index) {
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
    let method_nodes = tokenize_block_text(path, name, &format!("{{{body}}}"));
    let mut index = SourceIndex::default();
    for denied in denied_types {
        insert_wire(&mut index.denied, path, &[], denied.clone());
    }
    let mut local_denied = locally_closed.clone();
    let block: syn::Block = syn::parse_str(&format!("{{{body}}}"))
        .map_err(|error| format!("fixture {path} {name} does not parse: {error}"))?;
    local_denied.extend(local_denied_types(&block));
    let route = HandImplSource {
        path: path.to_owned(),
        scope: Vec::new(),
        name: name.to_owned(),
        body: body.to_owned(),
        method_nodes,
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
    }
}
