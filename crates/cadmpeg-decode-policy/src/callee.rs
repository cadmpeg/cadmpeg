// SPDX-License-Identifier: Apache-2.0
use crate::{types, Analysis};
use rustc_hir::{Expr, ExprKind};
use rustc_middle::ty::{self, Instance};
use rustc_span::def_id::DefId;

impl<'tcx> Analysis<'_, 'tcx> {
    pub(crate) fn call_arguments(
        &self,
        expression: &'tcx Expr<'tcx>,
    ) -> Option<ty::GenericArgsRef<'tcx>> {
        let arguments = match expression.kind {
            ExprKind::Call(callee, _) => match self.expr_ty(callee).kind() {
                ty::FnDef(_, arguments) => arguments.no_bound_vars()?,
                _ => return None,
            },
            _ => self.substitute(self.typeck.node_args(expression.hir_id)),
        };
        self.tcx
            .try_normalize_erasing_regions(self.typing_env(), ty::Unnormalized::new_wip(arguments))
            .ok()
    }

    pub(crate) fn resolved_instance(
        &self,
        expression: &'tcx Expr<'tcx>,
        definition: DefId,
    ) -> Option<Instance<'tcx>> {
        if !matches!(
            self.tcx.def_kind(definition),
            rustc_hir::def::DefKind::Fn
                | rustc_hir::def::DefKind::AssocFn
                | rustc_hir::def::DefKind::Ctor(_, rustc_hir::def::CtorKind::Fn)
        ) {
            return None;
        }
        let args = self.call_arguments(expression)?;
        if args.len() != self.tcx.generics_of(definition).count() {
            return None;
        }
        Instance::try_resolve(self.tcx, self.typing_env(), definition, args)
            .ok()
            .flatten()
    }

    pub(crate) fn implementation(
        &self,
        expression: &'tcx Expr<'tcx>,
        definition: DefId,
    ) -> Option<DefId> {
        let instance = self.resolved_instance(expression, definition)?;
        if matches!(instance.def, ty::InstanceKind::Virtual(_, _)) {
            None
        } else {
            Some(instance.def_id())
        }
    }

    pub(crate) fn checked_body(&self, definition: DefId) -> bool {
        crate::production(self.tcx, definition)
            && match definition.as_local() {
                Some(local) => self.tcx.hir_maybe_body_owned_by(local).is_some(),
                None => self.tcx.is_mir_available(definition),
            }
    }

    pub(crate) fn checked_call(&self, expression: &'tcx Expr<'tcx>, definition: DefId) -> bool {
        if self
            .implementation(expression, definition)
            .is_some_and(|id| self.checked_body(id))
        {
            return true;
        }
        if self.call_arguments(expression).is_some_and(|arguments| {
            arguments
                .types()
                .any(|value| matches!(value.peel_refs().kind(), ty::Dynamic(..)))
        }) {
            return false;
        }
        let Some(trait_id) = self.tcx.trait_of_assoc(definition) else {
            return false;
        };
        if types::cost_trait(self.tcx, trait_id) {
            return self.implementation(expression, definition).is_none();
        }
        if !trait_id.is_local() || self.tcx.visibility(trait_id).is_public() {
            return false;
        }
        let mut implementations = self.tcx.all_impls(trait_id).peekable();
        implementations.peek().is_some()
            && implementations.all(|id| {
                self.tcx
                    .associated_items(id)
                    .in_definition_order()
                    .find(|item| item.name() == self.tcx.item_name(definition))
                    .map_or_else(
                        || self.checked_body(definition),
                        |item| self.checked_body(item.def_id),
                    )
            })
    }

    pub(crate) fn custom_trait(
        &self,
        expression: &'tcx Expr<'tcx>,
        definition: DefId,
    ) -> Option<DefId> {
        let mut implementation = self.implementation(expression, definition)?;
        if types::standard(self.tcx, implementation)
            && self.tcx.trait_of_assoc(definition).is_some_and(|id| {
                matches!(
                    self.tcx.item_name(id).as_str(),
                    "Clone"
                        | "PartialEq"
                        | "PartialOrd"
                        | "Ord"
                        | "Hash"
                        | "From"
                        | "Into"
                        | "ToOwned"
                        | "AsRef"
                        | "AsMut"
                )
            })
        {
            let args = self.call_arguments(expression)?;
            let peeled: Vec<_> = args
                .iter()
                .map(|argument| match argument.kind() {
                    ty::GenericArgKind::Type(value) => value.peel_refs().into(),
                    _ => argument,
                })
                .collect();
            if let Ok(Some(instance)) = Instance::try_resolve(
                self.tcx,
                self.typing_env(),
                definition,
                match self.tcx.try_normalize_erasing_regions(
                    self.typing_env(),
                    ty::Unnormalized::new_wip(self.tcx.mk_args(&peeled)),
                ) {
                    Ok(arguments) => arguments,
                    Err(_) => return None,
                },
            ) {
                implementation = instance.def_id();
            }
        }
        if !types::standard(self.tcx, implementation) {
            Some(implementation)
        } else {
            None
        }
    }

    pub(crate) fn indirect(&mut self, expression: &'tcx Expr<'tcx>) {
        let ExprKind::Call(callee, _) = expression.kind else {
            return;
        };
        if matches!(self.expr_ty(callee).kind(), ty::FnDef(_, _)) {
            return;
        }
        if self.core_callback_parameter(self.expr_ty(callee)) {
            return;
        }
        if let ty::Closure(definition, _) = self.expr_ty(callee).peel_refs().kind() {
            if self.checked_body(*definition) {
                return;
            }
        }
        self.report(expression.span, "unproven_decode_charge", "indirect call: callback allocation and work charges cannot be established; use a concrete context-taking callee or explicit admission inside the callback");
    }
}

impl<'tcx> Analysis<'_, 'tcx> {
    pub(crate) fn iterator_item(&self, value: ty::Ty<'tcx>) -> Option<ty::Ty<'tcx>> {
        let iterator = self.tcx.lang_items().iterator_trait()?;
        let item = self
            .tcx
            .associated_items(iterator)
            .in_definition_order()
            .find(|item| item.name() == rustc_span::sym::Item)?;
        let projection = ty::Ty::new_projection(self.tcx, ty::IsRigid::No, item.def_id, [value]);
        self.tcx
            .try_normalize_erasing_regions(self.typing_env(), ty::Unnormalized::new_wip(projection))
            .ok()
    }
}

impl<'tcx> Analysis<'_, 'tcx> {
    pub(crate) fn clone_shape(&self, value: ty::Ty<'tcx>) -> types::Shape {
        let shape = types::heap(self.tcx, value, &mut Vec::new());
        if shape == types::Shape::Fixed {
            return shape;
        }
        let Some(clone) = self.tcx.lang_items().clone_trait() else {
            return types::Shape::Unknown;
        };
        let Some(method) = self
            .tcx
            .associated_items(clone)
            .in_definition_order()
            .find(|item| item.name() == rustc_span::sym::clone)
        else {
            return types::Shape::Unknown;
        };
        let Ok(args) = self.tcx.try_normalize_erasing_regions(
            self.typing_env(),
            ty::Unnormalized::new_wip(self.tcx.mk_args(&[value.into()])),
        ) else {
            return types::Shape::Unknown;
        };
        match Instance::try_resolve(self.tcx, self.typing_env(), method.def_id, args) {
            Ok(Some(instance)) => {
                let id = instance.def_id();
                if types::derived(self.tcx, id) {
                    if let ty::Adt(definition, arguments) = value.kind() {
                        definition
                            .all_fields()
                            .fold(types::Shape::Fixed, |shape, field| {
                                shape.join(
                                    self.clone_shape(field.ty(self.tcx, arguments).skip_norm_wip()),
                                )
                            })
                    } else {
                        types::Shape::Unknown
                    }
                } else if types::standard(self.tcx, id) {
                    shape
                } else if self.checked_body(id) {
                    types::Shape::Fixed
                } else {
                    types::Shape::Unknown
                }
            }
            _ => types::Shape::Unknown,
        }
    }
}

impl<'tcx> Analysis<'_, 'tcx> {
    pub(crate) fn trait_method(
        &self,
        value: ty::Ty<'tcx>,
        trait_name: &str,
        method: &str,
    ) -> Option<DefId> {
        let trait_id = self
            .tcx
            .get_diagnostic_item(rustc_span::Symbol::intern(trait_name))?;
        let item = self
            .tcx
            .associated_items(trait_id)
            .in_definition_order()
            .find(|item| item.name().as_str() == method)?;
        Instance::try_resolve(
            self.tcx,
            self.typing_env(),
            item.def_id,
            self.tcx.mk_args(&[value.into()]),
        )
        .ok()
        .flatten()
        .map(|instance| instance.def_id())
    }

    pub(crate) fn default_shape(
        &self,
        value: ty::Ty<'tcx>,
        seen: &mut Vec<ty::Ty<'tcx>>,
    ) -> types::Shape {
        if seen.contains(&value) {
            return types::Shape::Unknown;
        }
        let Some(method) = self.trait_method(value, "Default", "default") else {
            return types::Shape::Unknown;
        };
        if types::standard(self.tcx, method) || self.checked_body(method) {
            return types::Shape::Fixed;
        }
        if !types::derived(self.tcx, method) {
            return types::Shape::Unknown;
        }
        seen.push(value);
        let shape = match value.kind() {
            ty::Adt(owner, arguments) => {
                owner
                    .all_fields()
                    .fold(types::Shape::Fixed, |shape, field| {
                        shape.join(
                            self.default_shape(field.ty(self.tcx, arguments).skip_norm_wip(), seen),
                        )
                    })
            }
            _ => types::Shape::Unknown,
        };
        seen.pop();
        shape
    }
}
