// SPDX-License-Identifier: Apache-2.0
//! Expression construction sites, including constructor function values.

use std::collections::{BTreeMap, BTreeSet};

use proc_macro2::{Delimiter, TokenStream, TokenTree};
use syn::spanned::Spanned;
use syn::visit::{self, Visit};

use super::bindings::{Bindings, Value};
use super::{Call, Site};

pub(super) const CAPACITY: &str =
    "EncodeError::TargetCapacity outside its declared CAPACITY_FIELDS helper";
pub(super) type Aliases = BTreeMap<String, Vec<String>>;

pub(super) fn path(path: &syn::Path, aliases: &Aliases, owner: Option<&[String]>) -> Vec<String> {
    let mut result: Vec<_> = path
        .segments
        .iter()
        .map(|part| part.ident.to_string())
        .collect();
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
            let mut path = prefix.to_vec();
            path.push(name.ident.to_string());
            aliases.insert(name.ident.to_string(), path);
        }
        syn::UseTree::Rename(rename) => {
            let mut path = prefix.to_vec();
            path.push(rename.ident.to_string());
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
                    "UnsupportedContent",
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
                Some("TargetRefusalKind") => &[
                    "ExplicitUnavailable",
                    "InheritedUnavailable",
                    "DefaultUnavailable",
                    "UnrecordedSource",
                ],
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
    pub sites: Vec<Site>,
}
impl<'a> Body<'a> {
    pub(super) fn new(aliases: Aliases, owner: Option<&'a [String]>) -> Self {
        Self {
            aliases,
            owner,
            bindings: Bindings::default(),
            calls: BTreeSet::new(),
            sites: Vec::new(),
        }
    }

    fn check(&mut self, parts: &[String], span: proc_macro2::Span, construction: bool) {
        let Some(name) = parts.last().map(String::as_str) else {
            return;
        };
        let parent = parts.iter().rev().nth(1).map(String::as_str);
        let mut rules = BTreeSet::new();
        if construction && parent == Some("CodecError") {
            rules.insert("reachable CodecError construction candidate (fidelity-to-synthesis catch boundary not proved)");
        }
        if matches!(
            name,
            "NotImplemented" | "UnsupportedContent" | "Malformed" | "malformed" | "Truncated"
        ) {
            rules.insert("forbidden encode-path error spelling");
        }
        if (construction && name == "ResourceLimit")
            || (parent == Some("ResourceLimit") && name == "new")
        {
            // EncodeError::ResourceLimit wraps a session-issued value; it does
            // not construct the core budget finding.
            if parent != Some("EncodeError") && parent != Some("CodecError") {
                rules.insert("manufactured ResourceLimit");
            }
        }
        if parent == Some("ErrorKind") {
            rules.insert("manufactured io::ErrorKind");
        }
        if parent == Some("Error")
            && parts.iter().any(|part| part == "io")
            && matches!(name, "new" | "other" | "from")
        {
            rules.insert("manufactured io::Error");
        }
        if parent == Some("EncodeError") && construction {
            if name == "Io" {
                rules.insert("explicit EncodeError::Io (only Write/Seek ? may construct Io)");
            }
            if name == "TargetCapacity" {
                rules.insert(CAPACITY);
            }
        }
        if parent == Some("TargetRefusalKind")
            && matches!(
                name,
                "ExplicitUnavailable"
                    | "InheritedUnavailable"
                    | "DefaultUnavailable"
                    | "UnrecordedSource"
            )
        {
            rules.insert("removed TargetRefusalKind variant use");
        }
        for rule in rules {
            let at = span.start();
            self.sites.push(Site {
                line: at.line,
                column: at.column + 1,
                rule: rule.to_owned(),
            });
        }
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
            Value::Unknown if invoked => {
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
            Value::Unknown => {}
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
                    let construction = names.last().is_none_or(|name| name != "ResourceLimit")
                        || names
                            .iter()
                            .rev()
                            .nth(1)
                            .is_some_and(|name| name == "CodecError" || name == "EncodeError")
                        || matches!(following, Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Brace);
                    self.check(&names, tokens[start].span(), construction);
                    if matches!(following, Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Parenthesis)
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
    fn visit_expr_path(&mut self, value: &'ast syn::ExprPath) {
        let names = path(&value.path, &self.aliases, self.owner);
        self.check(&names, value.span(), true);
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
        self.check(
            &path(&value.path, &self.aliases, self.owner),
            value.span(),
            true,
        );
        visit::visit_expr_struct(self, value);
    }
    fn visit_expr_method_call(&mut self, value: &'ast syn::ExprMethodCall) {
        self.check(&[value.method.to_string()], value.method.span(), false);
        self.calls.insert(Call::Method(value.method.to_string()));
        for argument in &value.args {
            self.callable(argument, false);
        }
        visit::visit_expr_method_call(self, value);
    }
    fn visit_pat(&mut self, value: &'ast syn::Pat) {
        if let syn::Pat::Path(value) = value {
            self.check(
                &path(&value.path, &self.aliases, self.owner),
                value.span(),
                false,
            );
        } else {
            visit::visit_pat(self, value);
        }
    }
    fn visit_pat_struct(&mut self, value: &'ast syn::PatStruct) {
        self.check(
            &path(&value.path, &self.aliases, self.owner),
            value.span(),
            false,
        );
        visit::visit_pat_struct(self, value);
    }
    fn visit_pat_tuple_struct(&mut self, value: &'ast syn::PatTupleStruct) {
        self.check(
            &path(&value.path, &self.aliases, self.owner),
            value.span(),
            false,
        );
        visit::visit_pat_tuple_struct(self, value);
    }
    fn visit_macro(&mut self, value: &'ast syn::Macro) {
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
    }
}
