// SPDX-License-Identifier: Apache-2.0
//! Expression and block scanning for deserializer input routes.

use super::*;

#[derive(Debug, Clone, Default)]
pub(super) struct ScanEnvironment {
    pub(super) input_bindings: BTreeSet<String>,
    pub(super) validated_values: BTreeSet<String>,
    pub(super) version_check_shadowed: bool,
    pub(super) invalidated_values: BTreeSet<String>,
}

#[derive(Debug, Default)]
pub(super) struct RouteScan {
    pub(super) routes: Vec<InputRoute>,
    pub(super) version_gates: usize,
    pub(super) unsupported: Option<String>,
}

pub(super) fn unsupported(scan: &mut RouteScan, message: impl Into<String>) {
    if scan.unsupported.is_none() {
        scan.unsupported = Some(message.into());
    }
}

pub(super) fn invalidate_validated_value(environment: &mut ScanEnvironment, name: String) {
    if environment.validated_values.remove(&name) {
        environment.invalidated_values.insert(name);
    }
}

pub(super) fn merge_invalidated_values(
    environment: &mut ScanEnvironment,
    nested: &ScanEnvironment,
) {
    for name in &nested.invalidated_values {
        environment.invalidated_values.insert(name.clone());
        environment.validated_values.remove(name);
    }
}

pub(super) fn scan_isolated_expr(
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

pub(super) fn scan_borrowed_expr(
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

pub(super) fn pattern_bindings(pattern: &syn::Pat, found: &mut BTreeSet<String>) {
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

pub(super) fn call_route(
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

pub(super) fn method_route(method: &syn::ExprMethodCall, environment: &ScanEnvironment) -> bool {
    method.method == "deserialize_any"
        && expr_is_direct_binding(&method.receiver, &environment.input_bindings)
}

pub(super) fn expr_path_shape(path: &syn::ExprPath) -> Option<PathShape> {
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

pub(super) fn resolved_helper_route(resolved: &ResolvedPath) -> Option<InputRoute> {
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
                        && symbol.name == "deserialize_named")) =>
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

pub(super) fn path_matches(path: &[String], expected: &[&str]) -> bool {
    path.len() == expected.len()
        && path
            .iter()
            .zip(expected)
            .all(|(actual, wanted)| actual == wanted)
}

pub(super) fn helper_route(
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

pub(super) fn scan_expr(
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

pub(super) fn scan_block(
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
                    if let Some(diverge) = &init.diverge {
                        scan_expr(&diverge.1, route, index, &mut nested, scan, true, false);
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

pub(super) fn scan_isolated_block(
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

pub(super) fn local_pattern_name(pattern: &syn::Pat) -> Option<String> {
    match pattern {
        syn::Pat::Ident(pattern) if pattern.subpat.is_none() => Some(pattern.ident.to_string()),
        _ => None,
    }
}

pub(super) fn is_value_try_call(
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

pub(super) fn path_is_named_path(expression: &syn::Expr, name: &str) -> bool {
    let syn::Expr::Path(path) = expression else {
        return false;
    };
    path.qself.is_none() && path_is_named(&path.path, name)
}

pub(super) fn is_owned_version_check(
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
