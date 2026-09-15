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
#[derive(Debug)]
struct HandImplSource {
    path: String,
    scope: Vec<String>,
    name: String,
    body: String,
    method_nodes: Vec<Node>,
    /// Parameter bindings that receive the serde deserializer. A token named
    /// `deserializer` elsewhere in the body is not evidence that a call reads
    /// the method input: local bindings may shadow it, and a helper may merely
    /// mention the spelling.
    deserializer_bindings: BTreeSet<String>,
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
                    deserializer_bindings: signature_deserializer_bindings(&method.sig),
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

fn path_before_angle(nodes: &[Node], open: usize) -> Option<Vec<String>> {
    let end = if open >= 2 && is_atom(nodes, open - 1, ":") && is_atom(nodes, open - 2, ":") {
        open - 2
    } else {
        open
    };
    path_tail(nodes, end)
}

#[derive(Debug, Clone)]
enum TypeShape {
    Path(Vec<String>),
    Generic(Vec<String>, Vec<Vec<Node>>),
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
        return Some(TypeShape::Generic(
            path,
            split_node_arguments(&nodes[open + 1..close]),
        ));
    }
    Some(TypeShape::Path(path_tail(nodes, nodes.len())?))
}

fn path_is_keyless(path: &[String]) -> bool {
    path.len() == 1
        && matches!(
            path[0].as_str(),
            "bool"
                | "char"
                | "str"
                | "String"
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
                | "Vec"
                | "VecDeque"
                | "LinkedList"
                | "BinaryHeap"
                | "HashSet"
                | "BTreeSet"
                | "ByteBuf"
                | "$raw"
        )
        || path == ["crate", "bytes"]
}

fn type_is_keyless(nodes: &[Node]) -> bool {
    match type_shape(nodes) {
        Some(TypeShape::KeylessAggregate) => true,
        Some(TypeShape::Path(path)) => path_is_keyless(&path),
        Some(TypeShape::Generic(path, arguments)) => {
            let Some(name) = path.last() else {
                return false;
            };
            if path.len() == 1 && name == "Vec" {
                // A sequence reader consumes an array before it can inspect
                // an element, so an object key cannot reach its element type.
                return true;
            }
            if path.len() == 1
                && matches!(
                    name.as_str(),
                    "Box"
                        | "Option"
                        | "Rc"
                        | "Arc"
                        | "Pin"
                        | "RefCell"
                        | "Cell"
                        | "Mutex"
                        | "RwLock"
                )
                && arguments.len() == 1
            {
                return type_is_keyless(&arguments[0]);
            }
            false
        }
        None => false,
    }
}

fn type_receiver(nodes: &[Node]) -> Option<Receiver> {
    match type_shape(nodes)? {
        TypeShape::Path(path) => Some(Receiver::Path(path)),
        TypeShape::Generic(path, arguments) => Some(Receiver::Generic { path, arguments }),
        TypeShape::KeylessAggregate => None,
    }
}

#[derive(Debug, Clone)]
enum Receiver {
    Path(Vec<String>),
    Generic {
        path: Vec<String>,
        arguments: Vec<Vec<Node>>,
    },
    QSelf {
        type_nodes: Vec<Node>,
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
                arguments: split_node_arguments(&nodes[open + 1..close]),
            });
        }
        return Some(Receiver::QSelf {
            type_nodes: qself_type(nodes, open, close)?,
        });
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
        .iter()
        .any(|argument| direct_binding(argument, bindings))
}

fn receiver_is_keyless(receiver: &Receiver) -> bool {
    match receiver {
        Receiver::Path(path) => path_is_keyless(path),
        Receiver::Generic { path, arguments } => {
            let Some(name) = path.last() else {
                return false;
            };
            if path.len() == 1 && name == "Vec" {
                return true;
            }
            path.len() == 1
                && matches!(
                    name.as_str(),
                    "Box"
                        | "Option"
                        | "Rc"
                        | "Arc"
                        | "Pin"
                        | "RefCell"
                        | "Cell"
                        | "Mutex"
                        | "RwLock"
                )
                && arguments.len() == 1
                && type_is_keyless(&arguments[0])
        }
        Receiver::QSelf { type_nodes } => type_is_keyless(type_nodes),
    }
}

fn receiver_is_value(receiver: &Receiver) -> bool {
    match receiver {
        Receiver::Path(path) => path == &["serde_json".to_owned(), "Value".to_owned()],
        Receiver::Generic { .. } => false,
        Receiver::QSelf { type_nodes } => matches!(
            type_shape(type_nodes),
            Some(TypeShape::Path(path))
                if path == ["serde_json".to_owned(), "Value".to_owned()]
        ),
    }
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

fn collect_input_routes(nodes: &[Node], bindings: &BTreeSet<String>, routes: &mut Vec<InputRoute>) {
    for index in 0..nodes.len() {
        if let Some(receiver) = deserialize_receiver_at(nodes, index, bindings) {
            if receiver_is_keyless(&receiver) {
                routes.push(InputRoute::Keyless);
            } else if receiver_is_value(&receiver) {
                routes.push(InputRoute::Value);
            } else {
                routes.push(InputRoute::Wire(receiver));
            }
        }

        if is_atom(nodes, index, "deserialize_any")
            && index >= 2
            && is_atom(nodes, index - 1, ".")
            && bindings.contains(atom_opt(nodes.get(index - 2)).unwrap_or_default())
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
            && matches!(nodes.get(index + 1), Some(Node::Group(_, arguments)) if call_uses_deserializer(arguments, bindings))
        {
            routes.push(InputRoute::FreeForm);
        }

        if (is_atom(nodes, index, "deserialize_named")
            || is_atom(nodes, index, "deserialize_named_optional"))
            && matches!(nodes.get(index + 1), Some(Node::Group(Delimiter::Parenthesis, arguments)) if call_uses_deserializer(arguments, bindings))
        {
            routes.push(InputRoute::Keyless);
        }

        if let Some(Node::Group(_, children)) = nodes.get(index) {
            collect_input_routes(children, bindings, routes);
        }
    }
}

/// Count uses in the complete method body, including local binding patterns.
fn atom_count(nodes: &[Node], wanted: &str) -> usize {
    nodes
        .iter()
        .map(|node| match node {
            Node::Atom(value) => usize::from(value == wanted),
            Node::Group(_, children) => atom_count(children, wanted),
        })
        .sum()
}

/// Admit only the direct value/check sequence used by the version-field reader.
/// A name mentioned in a later expression does not establish binding identity.
fn has_direct_version_gate(nodes: &[Node], bindings: &BTreeSet<String>) -> bool {
    for index in 0..nodes.len() {
        let Some(receiver) = deserialize_receiver_at(nodes, index, bindings) else {
            continue;
        };
        if !receiver_is_value(&receiver) || index < 9 {
            continue;
        }
        // `let name = serde_json::Value::deserialize(input)?;`
        let start = index - 9;
        if (start != 0 && !is_atom(nodes, start - 1, ";"))
            || !is_atom(nodes, start, "let")
            || !is_atom(nodes, start + 2, "=")
            || !is_atom(nodes, index + 2, "?")
            || !is_atom(nodes, index + 3, ";")
        {
            continue;
        }
        let Some(name) = atom_opt(nodes.get(start + 1)) else {
            continue;
        };
        if name == "_"
            || !is_atom(nodes, index + 4, "check_ir_version")
            || !is_atom(nodes, index + 6, "?")
        {
            continue;
        }
        let Some(Node::Group(Delimiter::Parenthesis, arguments)) = nodes.get(index + 5) else {
            continue;
        };
        let [Node::Atom(some), Node::Group(Delimiter::Parenthesis, value)] = arguments.as_slice()
        else {
            continue;
        };
        if some == "Some" && value.len() == 2 && is_atom(value, 0, "&") && is_atom(value, 1, name) {
            return true;
        }
    }
    false
}

fn receiver_description(receiver: &Receiver) -> String {
    match receiver {
        Receiver::Path(path) => path.join("::"),
        Receiver::Generic { path, arguments } => format!(
            "{}<{}>",
            path.join("::"),
            arguments
                .iter()
                .map(|argument| nodes_text(argument))
                .collect::<Vec<_>>()
                .join(",")
        ),
        Receiver::QSelf { type_nodes } => format!("<{} as ...>", nodes_text(type_nodes)),
    }
}

fn same_crate(left: &str, right: &str) -> bool {
    left.split('/').nth(1) == right.split('/').nth(1)
}

fn wire_key_matches_route_scope(route: &HandImplSource, key: &WireKey) -> bool {
    same_crate(&route.path, &key.path) && key.scope == route.scope
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
    let path = match receiver {
        Receiver::Generic { path, arguments } => {
            let Some(name) = path.last() else {
                return false;
            };
            if path.len() == 1
                && matches!(
                    name.as_str(),
                    "Box"
                        | "Option"
                        | "Rc"
                        | "Arc"
                        | "Pin"
                        | "RefCell"
                        | "Cell"
                        | "Mutex"
                        | "RwLock"
                )
            {
                if arguments.len() != 1 {
                    return false;
                }
                let Some(inner) = type_receiver(&arguments[0]) else {
                    return false;
                };
                return receiver_is_closed(route, &inner, index, active);
            }
            // Generic user types such as `PatternTransform<C>` still resolve
            // through their declaration. Only the known delegating wrappers
            // above inspect their argument instead of their own wire shape.
            return receiver_is_closed(route, &Receiver::Path(path.clone()), index, active);
        }
        Receiver::QSelf { type_nodes } => {
            let Some(inner) = type_receiver(type_nodes) else {
                return false;
            };
            return receiver_is_closed(route, &inner, index, active);
        }
        Receiver::Path(path) => path,
    };
    let Some(name) = path.last() else {
        return false;
    };

    if path.len() == 1 && route.local_denied.contains(name) {
        return true;
    }

    let classify_target =
        |target: &HandImplSource, active: &mut BTreeSet<(String, Vec<String>, String)>| -> bool {
            matches!(
                classify_route_active(target, index, active),
                Ok(HandReaderClass::Wire)
            )
        };

    if path.len() == 1 {
        if index.denied.iter().any(|key| {
            key.name == *name && wire_key_matches_route_scope(route, key) && key.path == route.path
        }) {
            return true;
        }
        let targets: Vec<&HandImplSource> = index
            .sources
            .iter()
            .filter(|target| {
                target.name == *name && target.path == route.path && target.scope == route.scope
            })
            .collect();
        return targets.len() == 1 && classify_target(targets[0], active);
    }

    // Macro-generated `$wire::Wire` modules live below the macro's lexical
    // module and have no Rust crate-qualified path.
    if path.first().is_some_and(|segment| segment.starts_with('$')) {
        let mut scope = route.scope.clone();
        scope.extend(path.iter().take(path.len() - 1).cloned());
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
        return targets.len() == 1 && classify_target(targets[0], active);
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
        let targets: Vec<&HandImplSource> = index
            .sources
            .iter()
            .filter(|target| {
                same_crate(&route.path, &target.path)
                    && target.name == *name
                    && target.scope.starts_with(module_scope)
            })
            .collect();
        return targets.len() == 1 && classify_target(targets[0], active);
    }

    false
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
    let mut routes = Vec::new();
    // A direct input proof has one use of its parameter in the method body.
    // Aliases, shadowing and branch-specific bindings require deeper analysis.
    let bindings = route
        .deserializer_bindings
        .iter()
        .filter(|binding| atom_count(&route.method_nodes, binding) == 1)
        .cloned()
        .collect();
    collect_input_routes(&route.method_nodes, &bindings, &mut routes);
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
            && has_direct_version_gate(&route.method_nodes, &bindings)
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
    let route = HandImplSource {
        path: path.to_owned(),
        scope: Vec::new(),
        name: name.to_owned(),
        body: body.to_owned(),
        method_nodes,
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
    fn version_gate_must_directly_check_the_consumed_value() {
        let denied = BTreeSet::new();
        let no_local_wire = BTreeSet::new();
        for body in [
            "let value = serde_json::Value::deserialize(deserializer)?; let _old = value; let value = replacement(); check_ir_version(Some(&value))?;",
            "let value = serde_json::Value::deserialize(deserializer)?; check_ir_version((Some(&value), Some(&replacement())).1)?;",
            "let value = serde_json::Value::deserialize(deserializer)?; check_ir_version(Some(&{ drop(value); replacement() }))?;",
            "let value = serde_json::Value::deserialize(deserializer)?; let unchecked = || check_ir_version(Some(&value));",
            "let value = serde_json::Value::deserialize(deserializer)?; if false { check_ir_version(Some(&value))?; }",
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
    }
}
