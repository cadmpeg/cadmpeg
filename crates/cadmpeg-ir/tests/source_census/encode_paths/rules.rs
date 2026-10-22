// SPDX-License-Identifier: Apache-2.0
//! Expression construction sites, including constructor function values.

use std::collections::{BTreeMap, BTreeSet};

use proc_macro2::{Delimiter, TokenStream, TokenTree};
use syn::spanned::Spanned;
use syn::visit::{self, Visit};

use super::{Call, Site};

pub(super) const CAPACITY: &str =
    "EncodeError::TargetCapacity outside its declared CAPACITY_FIELDS helper";
pub(super) type Aliases = BTreeMap<String, Vec<String>>;

pub(super) fn path(path: &syn::Path, aliases: &Aliases, owner: Option<&str>) -> Vec<String> {
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
            owner.map(|name| vec![name.to_owned()])
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
    pub owner: Option<&'a str>,
    pub calls: BTreeSet<Call>,
    pub sites: Vec<Site>,
}
impl<'a> Body<'a> {
    pub(super) fn new(aliases: Aliases, owner: Option<&'a str>) -> Self {
        Self {
            aliases,
            owner,
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
                        } else {
                            self.calls.insert(Call::Function(names));
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
        self.calls.insert(Call::Function(names));
        visit::visit_expr_path(self, value);
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
        self.tokens(value.tokens.clone());
    }
}
