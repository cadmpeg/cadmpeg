// SPDX-License-Identifier: Apache-2.0
//! Where a wire source writes a type with generic arguments: the uses each
//! declaration, signature, const, static, impl and trait item states, and the
//! items a const block or a `macro_rules!` body declares.

use proc_macro2::TokenStream;
use syn::spanned::Spanned;

use super::{positional_arguments, Declaration, GenericUse, TraitIndex, UseSite};

impl TraitIndex {
    /// Records each type with generic arguments that `ty` writes, at any
    /// depth, as written at `site`: in a path's arguments, a function
    /// pointer's signature, and the bounds of an `impl Trait` or `dyn Trait`.
    pub(super) fn record_uses(&mut self, ty: &syn::Type, site: usize) {
        match ty {
            syn::Type::Path(typed) => {
                if let Some(qself) = &typed.qself {
                    self.record_uses(&qself.ty, site);
                    self.record_arguments(&typed.path, site);
                } else {
                    self.record_path(&typed.path, site);
                }
            }
            syn::Type::Reference(inner) => self.record_uses(&inner.elem, site),
            syn::Type::Array(inner) => self.record_uses(&inner.elem, site),
            syn::Type::Slice(inner) => self.record_uses(&inner.elem, site),
            syn::Type::Paren(inner) => self.record_uses(&inner.elem, site),
            syn::Type::Group(inner) => self.record_uses(&inner.elem, site),
            syn::Type::Ptr(inner) => self.record_uses(&inner.elem, site),
            syn::Type::Tuple(tuple) => {
                for inner in &tuple.elems {
                    self.record_uses(inner, site);
                }
            }
            syn::Type::FnPtr(function) => {
                for input in &function.inputs {
                    self.record_uses(&input.ty, site);
                }
                if let syn::ReturnType::Type(_, output) = &function.output {
                    self.record_uses(output, site);
                }
            }
            syn::Type::ImplTrait(bounded) => self.record_bounds(&bounded.bounds, site),
            syn::Type::TraitObject(object) => self.record_bounds(&object.bounds, site),
            _ => {}
        }
    }

    /// Records the uses every segment of `path` writes in its arguments.
    fn record_arguments(&mut self, path: &syn::Path, site: usize) {
        for segment in &path.segments {
            match &segment.arguments {
                syn::PathArguments::AngleBracketed(arguments) => {
                    for argument in &arguments.args {
                        match argument {
                            syn::GenericArgument::Type(inner) => self.record_uses(inner, site),
                            syn::GenericArgument::AssocType(bound) => {
                                self.record_uses(&bound.ty, site);
                            }
                            syn::GenericArgument::Constraint(constraint) => {
                                self.record_bounds(&constraint.bounds, site);
                            }
                            _ => {}
                        }
                    }
                }
                syn::PathArguments::Parenthesized(arguments) => {
                    for input in &arguments.inputs {
                        self.record_uses(&input.ty, site);
                    }
                    if let syn::ReturnType::Type(_, output) = &arguments.output {
                        self.record_uses(output, site);
                    }
                }
                syn::PathArguments::None => {}
            }
        }
    }

    /// Records the uses `path` writes, and `path` itself when its last
    /// segment states arguments.
    pub(super) fn record_path(&mut self, path: &syn::Path, site: usize) {
        self.record_arguments(path, site);
        let Some(last) = path.segments.last() else {
            return;
        };
        if !matches!(last.arguments, syn::PathArguments::AngleBracketed(_)) {
            return;
        }
        let mut bare = path.clone();
        if let Some(segment) = bare.segments.last_mut() {
            segment.arguments = syn::PathArguments::None;
        }
        self.uses.push(GenericUse {
            site,
            line: last.ident.span().start().line,
            name: last.ident.to_string(),
            path: bare,
            arguments: positional_arguments(&last.arguments)
                .into_iter()
                .map(Option::<&syn::Type>::cloned)
                .collect(),
        });
    }

    /// Records the uses the trait bounds `bounds` write.
    pub(super) fn record_bounds<'a>(
        &mut self,
        bounds: impl IntoIterator<Item = &'a syn::TypeParamBound>,
        site: usize,
    ) {
        for bound in bounds {
            if let syn::TypeParamBound::Trait(traited) = bound {
                self.record_path(&traited.path, site);
            }
        }
    }

    /// Records the uses `generics` writes: each parameter's bounds and
    /// default, and each where-clause predicate.
    pub(super) fn record_generics(&mut self, generics: &syn::Generics, site: usize) {
        for parameter in generics.type_params() {
            self.record_bounds(&parameter.bounds, site);
            if let Some((_, default)) = &parameter.default {
                self.record_uses(default, site);
            }
        }
        if let Some(clause) = &generics.where_clause {
            for predicate in &clause.predicates {
                if let syn::WherePredicate::Type(predicate) = predicate {
                    self.record_uses(&predicate.bounded_ty, site);
                    self.record_bounds(&predicate.bounds, site);
                }
            }
        }
    }

    /// Records the uses a function signature writes: its inputs, its
    /// return and its generics.
    pub(super) fn record_signature(&mut self, signature: &syn::Signature, site: usize) {
        for input in &signature.inputs {
            match input {
                syn::FnArg::Typed(typed) => self.record_uses(&typed.ty, site),
                syn::FnArg::Receiver(receiver) => {
                    if let syn::ReceiverKind::Typed(_, ty) = &receiver.kind {
                        self.record_uses(ty, site);
                    }
                }
            }
        }
        if let syn::ReturnType::Type(_, output) = &signature.output {
            self.record_uses(output, site);
        }
        self.record_generics(&signature.generics, site);
    }

    /// A site that instantiates no declaration's parameters of its own: a
    /// function, const, static or trait item, whose `generics` a document
    /// read does not fill, so a use that forwards one of them states nothing.
    pub(super) fn signature_site(
        &mut self,
        relative: &str,
        scope: &[String],
        generics: syn::Generics,
        implementation: Option<(&[String], &syn::Type)>,
    ) -> usize {
        let site = self.sites.len();
        self.sites.push(UseSite {
            path: relative.to_owned(),
            scope: scope.to_owned(),
            owner: None,
            generics,
            impl_parameters: implementation.map(|(parameters, _)| parameters.to_vec()),
            self_type: implementation.map(|(_, self_type)| self_type.clone()),
            read: false,
        });
        site
    }

    /// Records a const or static item: its type, the items its initializer
    /// declares, and the initializer's tokens, read in place.
    pub(super) fn record_initialized(
        &mut self,
        relative: &str,
        scope: &[String],
        generics: &syn::Generics,
        ty: &syn::Type,
        expression: &syn::Expr,
        implementation: Option<(&[String], &syn::Type)>,
    ) {
        let site = self.signature_site(relative, scope, generics.clone(), implementation);
        self.record_uses(ty, site);
        self.record_generics(generics, site);
        self.pending_initializers
            .push((scope.to_owned(), expression.span()));
        self.collect(&initializer_items(expression), relative, scope);
    }

    /// Records the uses a struct, enum or alias writes in its fields, its
    /// parameter defaults and `types`.
    pub(super) fn record_declaration_uses<'a>(
        &mut self,
        owner: Declaration,
        generics: &syn::Generics,
        read: bool,
        types: impl IntoIterator<Item = &'a syn::Type>,
    ) {
        let site = self.sites.len();
        self.sites.push(UseSite {
            path: owner.path.clone(),
            scope: owner.scope.clone(),
            owner: Some(owner),
            generics: generics.clone(),
            impl_parameters: None,
            self_type: None,
            read,
        });
        self.record_generics(generics, site);
        for ty in types {
            self.record_uses(ty, site);
        }
    }
}

/// The items a const or static initializer declares in its blocks, at any
/// depth of nested blocks.
pub(crate) fn initializer_items(expression: &syn::Expr) -> Vec<syn::Item> {
    let block = match expression {
        syn::Expr::Block(block) => &block.block,
        syn::Expr::Const(block) => &block.block,
        syn::Expr::Unsafe(block) => &block.block,
        _ => return Vec::new(),
    };
    block
        .stmts
        .iter()
        .flat_map(|statement| match statement {
            syn::Stmt::Item(item) => vec![item.clone()],
            syn::Stmt::Expr(inner, _) => initializer_items(inner),
            _ => Vec::new(),
        })
        .collect()
}

/// The items every arm of a `macro_rules!` body writes, or `None` when an
/// arm does not parse as items, as one that spells a fragment does not.
pub(super) fn transcribed_items(body: &TokenStream) -> Option<Vec<syn::Item>> {
    let trees: Vec<proc_macro2::TokenTree> = body.clone().into_iter().collect();
    let mut items = Vec::new();
    let mut index = 0;
    while index < trees.len() {
        let (
            Some(proc_macro2::TokenTree::Group(_)),
            Some(proc_macro2::TokenTree::Punct(equals)),
            Some(proc_macro2::TokenTree::Punct(arrow)),
            Some(proc_macro2::TokenTree::Group(transcriber)),
        ) = (
            trees.get(index),
            trees.get(index + 1),
            trees.get(index + 2),
            trees.get(index + 3),
        )
        else {
            return None;
        };
        if equals.as_char() != '=' || arrow.as_char() != '>' {
            return None;
        }
        items.extend(syn::parse2::<syn::File>(transcriber.stream()).ok()?.items);
        index += 4;
        if matches!(trees.get(index), Some(proc_macro2::TokenTree::Punct(semicolon)) if semicolon.as_char() == ';')
        {
            index += 1;
        }
    }
    Some(items)
}

/// The text of `text` that `span` covers.
pub(super) fn span_text(text: &str, span: proc_macro2::Span) -> Option<&str> {
    let offset = |at: proc_macro2::LineColumn| {
        let start: usize = text
            .split_inclusive('\n')
            .take(at.line.checked_sub(1)?)
            .map(str::len)
            .sum();
        let line = text.get(start..)?.split('\n').next()?;
        Some(
            start
                + line
                    .char_indices()
                    .nth(at.column)
                    .map_or(line.len(), |(byte, _)| byte),
        )
    };
    text.get(offset(span.start())?..offset(span.end())?)
}
