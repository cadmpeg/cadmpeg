// SPDX-License-Identifier: Apache-2.0
//! Core callback bounds defer their child obligations to concrete callers.
use crate::{external, Analysis};
use rustc_hir::Expr;
use rustc_middle::ty::{self, Ty, TyCtxt};
use rustc_span::def_id::DefId;

fn callback_trait(tcx: TyCtxt<'_>, id: DefId) -> bool {
    [tcx.lang_items().fn_trait(), tcx.lang_items().fn_mut_trait(), tcx.lang_items().fn_once_trait()].contains(&Some(id))
}
impl<'tcx> Analysis<'_, 'tcx> {
    fn core_callback_types(&self, definition: DefId, args: ty::GenericArgsRef<'tcx>) -> Vec<Ty<'tcx>> {
        if self.tcx.crate_name(definition.krate).as_str() != "cadmpeg_core" { return Vec::new(); }
        self.tcx.clauses_of(definition).instantiate(self.tcx, args).clauses.iter()
            .filter_map(|clause| match clause.kind().skip_binder() {
                ty::ClauseKind::Trait(predicate) if callback_trait(self.tcx, predicate.trait_ref.def_id) => Some(predicate.trait_ref.self_ty()),
                _ => None,
            }).collect()
    }
    pub(crate) fn core_callback_parameter(&self, value: Ty<'tcx>) -> bool {
        if !matches!(value.peel_refs().kind(), ty::Param(_)) { return false; }
        let owner = self.typeck.hir_owner.def_id.to_def_id();
        self.core_callback_types(owner, ty::GenericArgs::identity_for_item(self.tcx, owner)).contains(&value.peel_refs())
    }
    pub(crate) fn callback_boundary(&mut self, expression: &'tcx Expr<'tcx>) {
        let Some((definition, operands)) = self.call(expression) else { return; };
        let Some(args) = self.call_arguments(expression) else { return; };
        let callbacks = self.core_callback_types(definition, args);
        for operand in operands {
            let value = self.expr_ty(operand).peel_refs();
            if !callbacks.contains(&value) { continue; }
            let checked = match value.kind() {
                ty::Closure(id, _) => self.checked_body(*id),
                ty::FnDef(id, _) => matches!(self.tcx.def_kind(*id), rustc_hir::def::DefKind::Ctor(_, _)) || self.checked_body(*id) || external::summary(self.tcx, *id, None)
                    .is_some_and(|summary| summary.work == external::Work::Fixed
                        || matches!(self.tcx.item_name(definition).as_str(), "stable_sort_by" | "stable_sort_by_key" | "sort_unstable_by" | "sort_unstable_by_key")
                            && summary.work == external::Work::Comparison),
                ty::Param(_) => self.core_callback_parameter(value),
                _ => false,
            };
            if !checked {
                self.report(expression.span, "unproven_decode_charge", &format!(
                    "callback to DecodeContext::{} has no concrete checked body; admit child work in a concrete callback", self.tcx.item_name(definition)));
            }
        }
    }
}
