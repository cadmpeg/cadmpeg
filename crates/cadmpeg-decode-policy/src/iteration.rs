// SPDX-License-Identifier: Apache-2.0
//! Generic core collectors defer source-step obligations to their callers.
use crate::{external, types, Analysis};
use rustc_hir::Expr;
use rustc_middle::ty::{self, Ty};

impl<'tcx> Analysis<'_, 'tcx> {
    fn core_iterator_types(&self, definition: rustc_span::def_id::DefId, args: ty::GenericArgsRef<'tcx>) -> Vec<Ty<'tcx>> {
        if self.tcx.crate_name(definition.krate).as_str() != "cadmpeg_core" { return Vec::new(); }
        self.tcx.clauses_of(definition).instantiate(self.tcx, args).clauses.iter().filter_map(|clause| {
            match clause.kind().skip_binder() {
                ty::ClauseKind::Trait(predicate) if types::standard(self.tcx, predicate.trait_ref.def_id)
                    && matches!(self.tcx.item_name(predicate.trait_ref.def_id).as_str(), "Iterator" | "IntoIterator") => Some(predicate.trait_ref.self_ty()),
                _ => None,
            }
        }).collect()
    }

    fn core_iterator_parameter(&self, value: Ty<'tcx>) -> bool {
        let owner = self.typeck.hir_owner.def_id.to_def_id();
        let sources = self.core_iterator_types(owner, ty::GenericArgs::identity_for_item(self.tcx, owner));
        let value = value.peel_refs();
        sources.contains(&value) || match value.kind() {
            ty::Alias(_, alias) => match alias.kind {
                ty::AliasTyKind::Projection { def_id } if self.tcx.item_name(def_id).as_str() == "IntoIter" => alias.args.types().next().is_some_and(|source| sources.contains(&source)),
                _ => false,
            },
            _ => false,
        }
    }

    fn checked_iterator_callback(&self, callback: Ty<'tcx>) -> bool {
        match callback.peel_refs().kind() {
            ty::Closure(id, _) => self.checked_body(*id),
            ty::FnDef(id, _) => matches!(self.tcx.def_kind(*id), rustc_hir::def::DefKind::Ctor(_, _)) || self.checked_body(*id) || external::summary(self.tcx, *id, None).is_some_and(|cost| cost.work == external::Work::Fixed),
            ty::Param(_) => self.core_callback_parameter(callback),
            _ => false,
        }
    }

    fn bounded_iterator_steps(&self, value: Ty<'tcx>) -> bool {
        let value = value.peel_refs();
        if self.core_iterator_parameter(value) { return true; }
        match value.kind() {
            ty::Array(..) | ty::Slice(_) => return true,
            _ => (),
        }
        let ty::Adt(owner, args) = value.kind() else { return false; };
        if !types::standard(self.tcx, owner.did()) {
            return types::admitted_iterator(self.tcx, value);
        }
        let name = self.tcx.item_name(owner.did());
        let path = self.tcx.def_path_str(owner.did());
        let mut arguments = args.types();
        match name.as_str() {
            "Vec" => true,
            "IntoIter" if path.contains("vec::") || path.contains("array::") => true,
            "Iter" | "IterMut" if path.contains("slice::") => true,
            "Chunks" | "ChunksExact" | "Windows" | "Bytes" | "Chars" => true,
            "Range" | "RangeInclusive" | "RangeFrom" => arguments.next().is_some_and(|element| matches!(element.kind(), ty::Uint(_) | ty::Int(_) | ty::Char)),
            "Map" | "Inspect" => arguments.next().is_some_and(|source| self.bounded_iterator_steps(source)) && arguments.next().is_some_and(|callback| self.checked_iterator_callback(callback)),
            "Enumerate" | "Rev" | "Copied" | "Peekable" | "Fuse" => arguments.next().is_some_and(|source| self.bounded_iterator_steps(source)),
            "Filter" | "FilterMap" | "TakeWhile" | "SkipWhile" | "MapWhile" | "FlatMap" | "Scan" => types::admitted_iterator(self.tcx, value) && args.types().last().is_some_and(|callback| self.checked_iterator_callback(callback)),
            _ => types::admitted_iterator(self.tcx, value),
        }
    }

    pub(crate) fn iterator_boundary(&mut self, expression: &'tcx Expr<'tcx>) {
        let Some((definition, operands)) = self.call(expression) else { return; };
        let Some(args) = self.call_arguments(expression) else { return; };
        let sources = self.core_iterator_types(definition, args);
        for operand in operands {
            let value = self.expr_ty(operand).peel_refs();
            if sources.contains(&value) && !self.bounded_iterator_steps(value) {
                self.report(expression.span, "unproven_decode_charge", &format!("DecodeContext::{} source can scan before yielding; use DecodeContext::admit_iter on its base before applying adapters", self.tcx.item_name(definition)));
            }
        }
    }

    pub(crate) fn core_iterator_metadata(&self, expression: &'tcx Expr<'tcx>, definition: rustc_span::def_id::DefId) -> bool {
        if !types::standard(self.tcx, definition) || !matches!(self.tcx.item_name(definition).as_str(), "into_iter" | "size_hint") { return false; }
        self.call(expression).and_then(|(_, args)| args.first().copied()).is_some_and(|receiver| self.core_iterator_parameter(self.expr_ty(receiver)))
    }

    pub(crate) fn core_iterator_next(&mut self, expression: &'tcx Expr<'tcx>) -> bool {
        let Some((definition, operands)) = self.call(expression) else { return false; };
        if !types::standard(self.tcx, definition) || self.tcx.item_name(definition).as_str() != "next" { return false; }
        if !operands.first().is_some_and(|receiver| self.core_iterator_parameter(self.expr_ty(receiver))) { return false; }
        let required = self.flow.iterations;
        let Some(term) = self.flow.work.iter_mut().filter(|credit| !credit.opaque).flat_map(|credit| &mut credit.extents)
            .find(|term| term.factors.is_empty() && term.coefficient >= required) else { return false; };
        term.coefficient -= required;
        self.record_key_work_proof(expression);
        true
    }
}
