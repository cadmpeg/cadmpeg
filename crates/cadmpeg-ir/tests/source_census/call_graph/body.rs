// SPDX-License-Identifier: Apache-2.0
//! Function-body scan: calls, callable values and constructor-position paths.

use std::collections::{BTreeMap, BTreeSet};

use proc_macro2::{TokenStream, TokenTree};
use syn::spanned::Spanned;
use syn::visit::{self, Visit};

use super::bindings::{Bindings, Value};
use super::{Call, CtorKind, CtorUse};

pub(super) type Aliases = BTreeMap<String, Vec<String>>;

pub(super) fn path(path: &syn::Path, aliases: &Aliases, owner: Option<&[String]>) -> Vec<String> {
    let result: Vec<_> = path
        .segments
        .iter()
        .map(|part| part.ident.to_string())
        .collect();
    resolve_names(result, aliases, owner)
}

pub(super) fn resolve_names(
    mut result: Vec<String>,
    aliases: &Aliases,
    owner: Option<&[String]>,
) -> Vec<String> {
    let mut seen = BTreeSet::new();
    while let Some(first) = result.first().cloned() {
        if !seen.insert(first.clone()) {
            break;
        }
        let prefix = if first == "Self" {
            owner.map(<[String]>::to_vec)
        } else {
            aliases.get(&first).cloned()
        };
        let Some(mut prefix) = prefix else { break };
        prefix.extend(result.into_iter().skip(1));
        result = prefix;
    }
    result
}

pub(super) fn import(tree: &syn::UseTree, prefix: &[String], aliases: &mut Aliases) {
    match tree {
        syn::UseTree::Path(part) => {
            let mut path = prefix.to_vec();
            path.push(part.ident.to_string());
            import(&part.tree, &path, aliases);
        }
        syn::UseTree::Name(name) => {
            if name.ident == "self" && !prefix.is_empty() {
                aliases.insert(prefix[prefix.len() - 1].clone(), prefix.to_vec());
            } else {
                let mut path = prefix.to_vec();
                path.push(name.ident.to_string());
                aliases.insert(name.ident.to_string(), path);
            }
        }
        syn::UseTree::Rename(rename) => {
            let mut path = prefix.to_vec();
            if rename.ident != "self" || prefix.is_empty() {
                path.push(rename.ident.to_string());
            }
            aliases.insert(rename.rename.to_string(), path);
        }
        syn::UseTree::Group(group) => {
            for item in &group.items {
                import(item, prefix, aliases);
            }
        }
        syn::UseTree::Glob(_) => {
            let names: &[&str] = match prefix.last().map(String::as_str) {
                Some("CodecError") => &[
                    "WrongFormat",
                    "Malformed",
                    "InvalidInput",
                    "Truncated",
                    "ResourceLimit",
                    "UnsupportedDialect",
                    "UnsupportedTarget",
                    "NotImplemented",
                    "Io",
                ],
                Some("EncodeError") => &[
                    "InvalidInput",
                    "TargetCapacity",
                    "UnsupportedTarget",
                    "ResourceLimit",
                    "Io",
                ],
                Some("TargetRefusalKind") => &["UnknownExplicit", "NoDefault"],
                _ => &[],
            };
            for name in names {
                let mut path = prefix.to_vec();
                path.push((*name).to_owned());
                aliases.insert((*name).to_owned(), path);
            }
        }
    }
}

pub(super) struct Body<'a> {
    pub aliases: Aliases,
    pub owner: Option<&'a [String]>,
    pub bindings: Bindings,
    pub calls: BTreeSet<Call>,
    /// Constructor-position path uses (§5.2 scan): struct literals, call
    /// callees, and unit paths used as values.
    pub ctor: Vec<CtorUse>,
    /// Nesting depth inside equality comparisons and comparison macros, whose
    /// path operands are compared, not constructed.
    comparison: usize,
}
impl<'a> Body<'a> {
    pub(super) fn new(aliases: Aliases, owner: Option<&'a [String]>) -> Self {
        Self {
            aliases,
            owner,
            bindings: Bindings::default(),
            calls: BTreeSet::new(),
            ctor: Vec::new(),
            comparison: 0,
        }
    }

    fn record_ctor(&mut self, parts: &[String], kind: CtorKind, span: proc_macro2::Span) {
        if self.comparison != 0 {
            return;
        }
        self.ctor.push(CtorUse {
            path: parts.to_vec(),
            kind,
            line: span.start().line,
        });
    }

    fn value(&self, expression: &syn::Expr) -> Value {
        match expression {
            syn::Expr::Path(value) => {
                if let Some(name) = value.path.get_ident() {
                    if let Some(value) = self.bindings.get(&name.to_string()) {
                        return value.clone();
                    }
                }
                Value::Functions(BTreeSet::from([path(
                    &value.path,
                    &self.aliases,
                    self.owner,
                )]))
            }
            syn::Expr::Call(value) => {
                if let syn::Expr::Path(callee) = value.func.as_ref() {
                    let parts = path(&callee.path, &self.aliases, self.owner);
                    if parts.len() >= 2 {
                        return Value::Constructed {
                            constructor: parts,
                            unwrapped: false,
                        };
                    }
                }
                Value::Unknown
            }
            syn::Expr::Try(value) => match self.value(&value.expr) {
                Value::Constructed { constructor, .. } => Value::Constructed {
                    constructor,
                    unwrapped: true,
                },
                other => other,
            },
            syn::Expr::Paren(value) => self.value(&value.expr),
            syn::Expr::Group(value) => self.value(&value.expr),
            syn::Expr::Reference(value) => self.value(&value.expr),
            _ => Value::Unknown,
        }
    }

    fn callable(&mut self, expression: &syn::Expr, invoked: bool) {
        match self.value(expression) {
            Value::Functions(paths) => {
                for path in paths {
                    self.calls.insert(Call::Function(path));
                }
            }
            Value::Unknown | Value::Constructed { .. } if invoked => {
                let label = match expression {
                    syn::Expr::Path(path) => path
                        .path
                        .segments
                        .iter()
                        .map(|part| part.ident.to_string())
                        .collect::<Vec<_>>()
                        .join("::"),
                    _ => format!(
                        "indirect expression at line {}",
                        expression.span().start().line
                    ),
                };
                self.calls.insert(Call::UnresolvedValue(label));
            }
            Value::Unknown | Value::Constructed { .. } => {}
        }
    }

    pub(super) fn tokens(&mut self, tokens: TokenStream) {
        let tokens: Vec<_> = tokens.into_iter().collect();
        let mut index = 0;
        while index < tokens.len() {
            if let TokenTree::Ident(first) = &tokens[index] {
                let start = index;
                let mut names = vec![first.to_string()];
                while matches!(tokens.get(index + 1), Some(TokenTree::Punct(p)) if p.as_char() == ':')
                    && matches!(tokens.get(index + 2), Some(TokenTree::Punct(p)) if p.as_char() == ':')
                {
                    let Some(TokenTree::Ident(next)) = tokens.get(index + 3) else {
                        break;
                    };
                    names.push(next.to_string());
                    index += 3;
                }
                let text = names.join("::");
                if let Ok(parsed) = syn::parse_str::<syn::Path>(&text) {
                    let names = path(&parsed, &self.aliases, self.owner);
                    let following = tokens.get(index + 1);
                    let comparison_adjacent = matches!(tokens.get(start.wrapping_sub(1)), Some(TokenTree::Punct(p)) if matches!(p.as_char(), '=' | '!' | '<' | '>'))
                        || matches!(following, Some(TokenTree::Punct(p)) if matches!(p.as_char(), '=' | '!'));
                    if names.len() >= 2 && !comparison_adjacent {
                        self.record_ctor(&names, CtorKind::Unit, tokens[start].span());
                    }
                    if matches!(following, Some(TokenTree::Group(group)) if group.delimiter() == proc_macro2::Delimiter::Parenthesis)
                    {
                        let method = start
                            .checked_sub(1)
                            .and_then(|at| tokens.get(at))
                            .is_some_and(
                                |token| matches!(token, TokenTree::Punct(p) if p.as_char() == '.'),
                            );
                        if method {
                            self.calls.insert(Call::Method(first.to_string()));
                        } else if start
                            .checked_sub(1)
                            .and_then(|at| tokens.get(at))
                            .is_some_and(
                                |token| matches!(token, TokenTree::Punct(p) if p.as_char() == '$'),
                            )
                        {
                            self.calls.insert(Call::UnresolvedValue(format!(
                                "${first} (macro parameter)"
                            )));
                        } else if let Ok(expression) = syn::parse_str::<syn::Expr>(&text) {
                            self.callable(&expression, true);
                        }
                    } else if matches!(following, Some(TokenTree::Punct(p)) if p.as_char() == '!') {
                        self.calls.insert(Call::Macro(first.to_string()));
                    }
                }
            }
            if let TokenTree::Group(group) = &tokens[index] {
                self.tokens(group.stream());
            }
            index += 1;
        }
    }
}

impl<'ast> Visit<'ast> for Body<'_> {
    fn visit_item(&mut self, _item: &'ast syn::Item) {}
    fn visit_expr(&mut self, value: &'ast syn::Expr) {
        if let syn::Expr::Binary(binary) = value {
            if matches!(binary.op, syn::BinOp::Eq(_) | syn::BinOp::Ne(_)) {
                self.comparison += 1;
                self.visit_expr(&binary.left);
                self.visit_expr(&binary.right);
                self.comparison -= 1;
                return;
            }
        }
        visit::visit_expr(self, value);
    }
    fn visit_expr_path(&mut self, value: &'ast syn::ExprPath) {
        if value.path.get_ident().is_some_and(|name| {
            matches!(
                self.bindings.get(&name.to_string()),
                Some(Value::Unknown | Value::Constructed { .. })
            )
        }) {
            return;
        }
        let names = path(&value.path, &self.aliases, self.owner);
        if names.len() >= 2 {
            self.record_ctor(&names, CtorKind::Unit, value.span());
        }
        visit::visit_expr_path(self, value);
    }
    fn visit_block(&mut self, block: &'ast syn::Block) {
        self.bindings.push();
        visit::visit_block(self, block);
        self.bindings.pop();
    }
    fn visit_local(&mut self, local: &'ast syn::Local) {
        if let Some(init) = &local.init {
            self.visit_expr(&init.expr);
        }
        let value = local
            .init
            .as_ref()
            .map_or(Value::Unknown, |init| self.value(&init.expr));
        self.bindings.bind(&local.pat, value);
        if let Some(diverge) = local.init.as_ref().and_then(|init| init.diverge.as_ref()) {
            self.visit_expr(&diverge.1);
        }
    }
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(callee) = call.func.as_ref() {
            let names = path(&callee.path, &self.aliases, self.owner);
            self.record_ctor(&names, CtorKind::Call, callee.span());
        }
        self.callable(&call.func, true);
        for argument in &call.args {
            self.callable(argument, false);
        }
        visit::visit_expr_call(self, call);
    }
    fn visit_expr_assign(&mut self, assignment: &'ast syn::ExprAssign) {
        if let syn::Expr::Path(name) = assignment.left.as_ref() {
            if let Some(name) = name.path.get_ident() {
                self.bindings
                    .assign(&name.to_string(), self.value(&assignment.right));
            }
        }
        visit::visit_expr_assign(self, assignment);
    }
    fn visit_arm(&mut self, arm: &'ast syn::Arm) {
        self.bindings.push();
        self.bindings.bind(&arm.pat, Value::Unknown);
        visit::visit_arm(self, arm);
        self.bindings.pop();
    }
    fn visit_expr_closure(&mut self, closure: &'ast syn::ExprClosure) {
        self.bindings.push();
        for input in &closure.inputs {
            self.bindings.bind(input, Value::Unknown);
        }
        visit::visit_expr_closure(self, closure);
        self.bindings.pop();
    }
    fn visit_expr_let(&mut self, value: &'ast syn::ExprLet) {
        self.visit_expr(&value.expr);
        self.bindings.bind(&value.pat, Value::Unknown);
    }
    fn visit_expr_if(&mut self, value: &'ast syn::ExprIf) {
        self.bindings.push();
        self.visit_expr(&value.cond);
        self.visit_block(&value.then_branch);
        self.bindings.pop();
        if let Some((_, branch)) = &value.else_branch {
            self.visit_expr(branch);
        }
    }
    fn visit_expr_while(&mut self, value: &'ast syn::ExprWhile) {
        self.bindings.push();
        self.visit_expr(&value.cond);
        self.visit_block(&value.body);
        self.bindings.pop();
    }
    fn visit_expr_for_loop(&mut self, value: &'ast syn::ExprForLoop) {
        self.visit_expr(&value.expr);
        self.bindings.push();
        self.bindings.bind(&value.pat, Value::Unknown);
        self.visit_block(&value.body);
        self.bindings.pop();
    }
    fn visit_expr_struct(&mut self, value: &'ast syn::ExprStruct) {
        let names = path(&value.path, &self.aliases, self.owner);
        self.record_ctor(&names, CtorKind::Struct, value.span());
        visit::visit_expr_struct(self, value);
    }
    fn visit_expr_method_call(&mut self, value: &'ast syn::ExprMethodCall) {
        let method = value.method.to_string();
        self.calls.insert(Call::Method(method.clone()));
        if let syn::Expr::Path(receiver) = value.receiver.as_ref() {
            if let Some(name) = receiver.path.get_ident() {
                if name == "self" {
                    self.calls.insert(Call::SelfMethod(method.clone()));
                } else if let Some(Value::Constructed {
                    constructor,
                    unwrapped,
                }) = self.bindings.get(&name.to_string())
                {
                    self.calls.insert(Call::MethodOnConstructor {
                        constructor: constructor.clone(),
                        method,
                        unwrapped: *unwrapped,
                    });
                }
            }
        }
        for argument in &value.args {
            self.callable(argument, false);
        }
        visit::visit_expr_method_call(self, value);
    }
    fn visit_pat(&mut self, value: &'ast syn::Pat) {
        if !matches!(value, syn::Pat::Path(_)) {
            visit::visit_pat(self, value);
        }
    }
    fn visit_macro(&mut self, value: &'ast syn::Macro) {
        let compared = value.path.segments.last().is_some_and(|name| {
            matches!(
                name.ident.to_string().as_str(),
                "matches" | "assert_eq" | "assert_ne"
            )
        });
        if compared {
            self.comparison += 1;
        }
        if let Some(name) = value.path.segments.last() {
            self.calls.insert(Call::Macro(name.ident.to_string()));
        }
        if let Ok(arguments) = value.parse_body_with(
            syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated,
        ) {
            for argument in arguments {
                self.callable(&argument, false);
            }
        }
        self.tokens(value.tokens.clone());
        if compared {
            self.comparison -= 1;
        }
    }
}
