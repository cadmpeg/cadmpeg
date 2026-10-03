// SPDX-License-Identifier: Apache-2.0
//! Closed scalar parsers allocate no input-sized result.
use crate::Analysis;
use rustc_hir::Expr;
use rustc_middle::ty;

impl<'tcx> Analysis<'_, 'tcx> {
    pub(crate) fn closed_scalar_parse(&self, expression: &'tcx Expr<'tcx>) -> bool {
        let Some((definition, _)) = self.call(expression) else { return false; };
        if self.tcx.item_name(definition).as_str() != "parse" { return false; }
        let Some(target) = self.call_arguments(expression).and_then(|args| args.types().next()) else { return false; };
        let owner = self.typeck.hir_owner.def_id;
        self.tcx.clauses_of(owner).instantiate_identity(self.tcx).clauses.iter().any(|clause| {
            matches!(clause.kind().skip_binder(), ty::ClauseKind::Trait(predicate)
                if predicate.trait_ref.self_ty() == target
                    && self.tcx.crate_name(predicate.trait_ref.def_id.krate).as_str() == "cadmpeg_core"
                    && matches!(self.tcx.def_path_str(predicate.trait_ref.def_id).as_str(),
                        "decode::text::TextScalar" | "cadmpeg_core::decode::text::TextScalar"))
        })
    }
}
