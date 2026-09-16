// SPDX-License-Identifier: Apache-2.0
//! Route classification for one hand-written reader.

use super::*;

pub(super) fn type_is_keyless(
    route: &HandImplSource,
    shape: &TypeShape,
    index: &SourceIndex,
) -> bool {
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

pub(super) fn receiver_is_keyless(
    route: &HandImplSource,
    receiver: &Receiver,
    index: &SourceIndex,
) -> bool {
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

pub(super) fn receiver_is_value(
    route: &HandImplSource,
    receiver: &Receiver,
    index: &SourceIndex,
) -> bool {
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

pub(super) fn deserialize_receiver_at(
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

pub(super) fn receiver_description(receiver: &Receiver) -> String {
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

pub(super) fn same_crate(left: &str, right: &str) -> bool {
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

pub(super) fn route_identity(route: &HandImplSource) -> (String, Vec<String>, String) {
    (route.path.clone(), route.scope.clone(), route.name.clone())
}

/// Resolve an object receiver against its declaration or against a manual
/// reader whose complete input route is closed. A target with a denied local
/// declaration is not enough: its consumed reader must itself classify as a
/// closed wire route.
pub(super) fn receiver_is_closed(
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

pub(super) fn is_wrapper_path(resolved: &ResolvedPath) -> bool {
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

pub(super) fn receiver_from_shape(shape: &TypeShape) -> Option<Receiver> {
    match shape {
        TypeShape::Path(path) => Some(Receiver::Path(path.clone())),
        TypeShape::Generic(path, arguments) => Some(Receiver::Generic {
            path: path.clone(),
            arguments: arguments.clone(),
        }),
        TypeShape::KeylessAggregate => None,
    }
}

pub(super) fn macro_helper_route_at(
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

pub(super) fn macro_input_routes(
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

pub(super) fn classify_route(
    route: &HandImplSource,
    index: &SourceIndex,
) -> Result<HandReaderClass, String> {
    let mut active = BTreeSet::new();
    classify_route_active(route, index, &mut active)
}

pub(super) fn classify_route_active(
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

pub(super) fn classify_route_body(
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
pub(super) fn classify_hand_reader(
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

pub(super) fn classify_hand_reader_with_bindings(
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
pub(crate) fn assert_hand_written_reader_routes() {
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
