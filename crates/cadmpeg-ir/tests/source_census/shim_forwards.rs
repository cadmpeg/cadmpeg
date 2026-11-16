// SPDX-License-Identifier: Apache-2.0
//! Field-deserializer helpers that forward to another helper of their file.

/// The helper `function` forwards to, when its body is one call of a helper
/// named without a path on its one parameter, followed by nothing but `.map`
/// calls.
///
/// Such a function refuses whatever its callee refuses: the callee returns a
/// `Result`, and `Result::map` rewrites only `Ok`, so an error the callee
/// raises for `null` is the function's own error.
pub(super) fn forwarded_shim(function: &syn::ItemFn) -> Option<String> {
    let mut parameters = function.sig.inputs.iter();
    let (Some(syn::FnArg::Typed(parameter)), None) = (parameters.next(), parameters.next()) else {
        return None;
    };
    let syn::Pat::Ident(parameter) = parameter.pat.as_ref() else {
        return None;
    };
    let [syn::Stmt::Expr(body, None)] = function.block.stmts.as_slice() else {
        return None;
    };
    let mut expression = body;
    while let syn::Expr::MethodCall(call) = expression {
        if call.method != "map" || call.args.len() != 1 {
            return None;
        }
        expression = &call.receiver;
    }
    let syn::Expr::Call(call) = expression else {
        return None;
    };
    let syn::Expr::Path(callee) = call.func.as_ref() else {
        return None;
    };
    let callee = callee.path.get_ident()?;
    let mut arguments = call.args.iter();
    let (Some(syn::Expr::Path(argument)), None) = (arguments.next(), arguments.next()) else {
        return None;
    };
    (argument.path.get_ident()? == &parameter.ident).then(|| callee.to_string())
}

#[test]
fn a_forward_is_one_call_on_the_parameter_and_only_maps() {
    let forward =
        |source: &str| forwarded_shim(&syn::parse_str(source).expect("parse forwarding function"));
    assert_eq!(
        forward("fn f<'de, D: Deserializer<'de>>(d: D) -> R { shim(d) }"),
        Some("shim".into())
    );
    assert_eq!(
        forward("fn f(d: D) -> R { shim(d).map(|x| x.map(|S(set)| set)).map(|x| x) }"),
        Some("shim".into())
    );
    for refused in [
        "fn f(d: D) -> R { shim(d).or_else(|_| Ok(None)) }",
        "fn f(d: D) -> R { shim(d).map_err(|e| e) }",
        "fn f(d: D) -> R { Ok(shim(d).unwrap_or(None)) }",
        "fn f(d: D) -> R { inner::shim(d) }",
        "fn f(d: D) -> R { shim(d, 1) }",
        "fn f(d: D) -> R { shim(other) }",
        "fn f(d: D, e: E) -> R { shim(d) }",
        "fn f(d: D) -> R { let x = 1; shim(d) }",
        "fn f(d: D) -> R { shim(d); }",
    ] {
        assert_eq!(forward(refused), None, "{refused}");
    }
}
