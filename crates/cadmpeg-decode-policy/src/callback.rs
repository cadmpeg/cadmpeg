// SPDX-License-Identifier: Apache-2.0
//! Core and container callbacks defer child obligations to concrete callers.
use crate::{external, types, Analysis};
use rustc_hir::Expr;
use rustc_middle::ty::{self, Instance, Ty, TyCtxt};
use rustc_span::def_id::DefId;

fn callback_trait(tcx: TyCtxt<'_>, id: DefId) -> bool {
    [
        tcx.lang_items().fn_trait(),
        tcx.lang_items().fn_mut_trait(),
        tcx.lang_items().fn_once_trait(),
    ]
    .contains(&Some(id))
}
fn reader_traits(tcx: TyCtxt<'_>, id: DefId, seen: &mut Vec<DefId>, result: &mut Vec<DefId>) {
    if seen.contains(&id) {
        return;
    }
    seen.push(id);
    if types::standard(tcx, id) && matches!(tcx.item_name(id).as_str(), "Read" | "Seek") {
        result.push(id);
        return;
    }
    for entry in tcx.explicit_super_clauses_of(id).iter_identity_copied() {
        let (clause, _) = entry.skip_norm_wip();
        if let ty::ClauseKind::Trait(predicate) = clause.kind().skip_binder() {
            reader_traits(tcx, predicate.trait_ref.def_id, seen, result);
        }
    }
}

impl<'tcx> Analysis<'_, 'tcx> {
    fn provider_callback_types(
        &self,
        definition: DefId,
        args: ty::GenericArgsRef<'tcx>,
    ) -> Vec<Ty<'tcx>> {
        if !matches!(
            self.tcx.crate_name(definition.krate).as_str(),
            "cadmpeg_core" | "cadmpeg_container"
        ) {
            return Vec::new();
        }
        self.tcx
            .clauses_of(definition)
            .instantiate(self.tcx, args)
            .clauses
            .iter()
            .filter_map(|clause| match clause.kind().skip_binder() {
                ty::ClauseKind::Trait(predicate)
                    if callback_trait(self.tcx, predicate.trait_ref.def_id) =>
                {
                    Some(predicate.trait_ref.self_ty())
                }
                _ => None,
            })
            .collect()
    }
    fn provider_reader_types(
        &self,
        definition: DefId,
        args: ty::GenericArgsRef<'tcx>,
    ) -> Vec<(Ty<'tcx>, DefId)> {
        if !matches!(
            self.tcx.crate_name(definition.krate).as_str(),
            "cadmpeg_core" | "cadmpeg_container"
        ) {
            return Vec::new();
        }
        let mut result = Vec::new();
        for clause in self
            .tcx
            .clauses_of(definition)
            .instantiate(self.tcx, args)
            .clauses
            .iter()
        {
            let ty::ClauseKind::Trait(predicate) = clause.kind().skip_binder() else {
                continue;
            };
            let mut traits = Vec::new();
            reader_traits(
                self.tcx,
                predicate.trait_ref.def_id,
                &mut Vec::new(),
                &mut traits,
            );
            for id in traits {
                result.push((predicate.trait_ref.self_ty().peel_refs(), id));
            }
        }
        result
    }

    fn audited_standard_reader(&self, value: Ty<'tcx>, trait_id: DefId) -> bool {
        let byte_sequence = |value: Ty<'tcx>| {
            matches!(value.peel_refs().kind(), ty::Slice(element) | ty::Array(element, _)
            if matches!(element.kind(), ty::Uint(ty::UintTy::U8)))
        };
        if self.tcx.item_name(trait_id).as_str() == "Read" && byte_sequence(value) {
            return true;
        }
        let ty::Adt(owner, args) = value.peel_refs().kind() else {
            return false;
        };
        if !types::standard(self.tcx, owner.did()) {
            return false;
        }
        match self.tcx.item_name(owner.did()).as_str() {
            "File" => true,
            "Cursor" => args.types().next().is_some_and(|value| byte_sequence(value)
                || matches!(value.peel_refs().kind(), ty::Adt(owner, args) if types::standard(self.tcx, owner.did())
                    && self.tcx.item_name(owner.did()).as_str() == "Vec"
                    && args.types().next().is_some_and(|element| matches!(element.kind(), ty::Uint(ty::UintTy::U8))))),
            _ => false,
        }
    }

    fn checked_reader(&self, value: Ty<'tcx>, trait_id: DefId) -> bool {
        if matches!(value.kind(), ty::Param(_)) {
            return self.provider_callback_parameter(value);
        }
        if self.audited_standard_reader(value, trait_id) {
            return true;
        }
        let names: &[&str] = if self.tcx.item_name(trait_id).as_str() == "Read" {
            &["read"]
        } else {
            &["seek", "rewind"]
        };
        names.iter().all(|name| {
            let Some(method) = self
                .tcx
                .associated_items(trait_id)
                .in_definition_order()
                .find(|item| item.name().as_str() == *name)
            else {
                return false;
            };
            Instance::try_resolve(
                self.tcx,
                self.typing_env(),
                method.def_id,
                self.tcx.mk_args(&[value.into()]),
            )
            .ok()
            .flatten()
            .is_some_and(|instance| {
                self.checked_body(instance.def_id())
                    || *name == "rewind"
                        && instance.def_id() == method.def_id
                        && types::standard(self.tcx, method.def_id)
            })
        })
    }

    pub(crate) fn provider_reader_call(
        &self,
        expression: &'tcx Expr<'tcx>,
        definition: DefId,
    ) -> bool {
        let Some(trait_id) = self.tcx.trait_of_assoc(definition) else {
            return false;
        };
        if !types::standard(self.tcx, trait_id)
            || !matches!(self.tcx.item_name(trait_id).as_str(), "Read" | "Seek")
        {
            return false;
        }
        if !matches!(
            self.tcx.item_name(definition).as_str(),
            "read" | "seek" | "rewind"
        ) {
            return false;
        }
        self.call(expression)
            .and_then(|(_, args)| args.first().copied())
            .is_some_and(|receiver| self.provider_callback_parameter(self.expr_ty(receiver)))
    }

    pub(crate) fn provider_callback_parameter(&self, value: Ty<'tcx>) -> bool {
        if !matches!(value.peel_refs().kind(), ty::Param(_)) {
            return false;
        }
        let owner = self.typeck.hir_owner.def_id.to_def_id();
        let args = ty::GenericArgs::identity_for_item(self.tcx, owner);
        self.provider_callback_types(owner, args)
            .contains(&value.peel_refs())
            || self
                .provider_reader_types(owner, args)
                .iter()
                .any(|(reader, _)| *reader == value.peel_refs())
    }
    pub(crate) fn callback_boundary(&mut self, expression: &'tcx Expr<'tcx>) {
        let Some((definition, operands)) = self.call(expression) else {
            return;
        };
        let Some(args) = self.call_arguments(expression) else {
            return;
        };
        let callbacks = self.provider_callback_types(definition, args);
        for operand in operands {
            let value = self.expr_ty(operand).peel_refs();
            if !callbacks.contains(&value) {
                continue;
            }
            let checked = match value.kind() {
                ty::Closure(id, _) => self.checked_body(*id),
                ty::FnDef(id, _) => {
                    matches!(self.tcx.def_kind(*id), rustc_hir::def::DefKind::Ctor(_, _))
                        || self.checked_body(*id)
                        || external::summary(self.tcx, *id, None).is_some_and(|summary| {
                            summary.work == external::Work::Fixed
                                || matches!(
                                    self.tcx.item_name(definition).as_str(),
                                    "stable_sort_by"
                                        | "stable_sort_by_key"
                                        | "sort_unstable_by"
                                        | "sort_unstable_by_key"
                                ) && summary.work == external::Work::Comparison
                        })
                }
                ty::Param(_) => self.provider_callback_parameter(value),
                _ => false,
            };
            if !checked {
                self.report(expression.span, "unproven_decode_charge", &format!(
                    "callback to {} has no concrete checked body; admit child work in a concrete callback", self.tcx.def_path_str(definition)));
            }
        }
        for (reader, trait_id) in self.provider_reader_types(definition, args) {
            if !self.checked_reader(reader, trait_id) {
                self.report(expression.span, "unproven_decode_charge", &format!(
                    "reader callback to {} has no concrete checked {} implementation; use a concrete reader with admitted child work",
                    self.tcx.def_path_str(definition), self.tcx.item_name(trait_id)));
            }
        }
    }
}
