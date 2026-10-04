// SPDX-License-Identifier: Apache-2.0
//! Admission receipts for the shared identity grammars.
use crate::{flow::ExtentTerm, storage, types, Analysis};
use rustc_hir::Expr;
use rustc_middle::ty::{self, Ty};
use rustc_span::def_id::DefId;

struct GrammarConstructor {
    owner: &'static str,
    method: &'static str,
    source_count: usize,
}

const GRAMMAR_CONSTRUCTORS: [GrammarConstructor; 4] = [
    GrammarConstructor { owner: "IdentityComponent", method: "try_new", source_count: 1 },
    GrammarConstructor { owner: "IdentityKey", method: "try_new", source_count: 1 },
    GrammarConstructor { owner: "IdentityKeyTail", method: "try_new", source_count: 1 },
    GrammarConstructor { owner: "IdentityNamespace", method: "new", source_count: 3 },
];

impl<'tcx> Analysis<'_, 'tcx> {
    fn ir_type(&self, value: Ty<'tcx>, name: &str) -> bool {
        matches!(value.kind(), ty::Adt(owner, _)
            if types::physical_item_path(self.tcx, owner.did(), "cadmpeg_ir", &["ids", name]))
    }

    fn identity_grammar_result(&self, value: Ty<'tcx>, constructor: &GrammarConstructor) -> bool {
        let ty::Adt(result, arguments) = value.kind() else {
            return false;
        };
        if !types::physical_item_path(self.tcx, result.did(), "core", &["result", "Result"])
        {
            return false;
        }
        let mut types = arguments.types();
        types
            .next()
            .is_some_and(|identity| self.ir_type(identity, constructor.owner))
            && types
                .next()
                .is_some_and(|error| self.ir_type(error, "IdentityError"))
            && types.next().is_none()
    }

    /// Each owned source is scanned once for Unicode whitespace and once for
    /// separators. Consume its receipt at this call, before either scan.
    pub(crate) fn shared_identity_grammar_paid(
        &mut self,
        expression: &'tcx Expr<'tcx>,
        definition: DefId,
    ) -> Option<Option<bool>> {
        if self.tcx.def_kind(definition) != rustc_hir::def::DefKind::AssocFn {
            return None;
        }
        let constructor = GRAMMAR_CONSTRUCTORS
            .iter()
            .find(|constructor| types::physical_inherent_method(
                self.tcx, definition, "cadmpeg_ir", &["ids", constructor.owner], constructor.method,
            ))?;

        let Some((called, operands)) = self.call(expression) else {
            return Some(None);
        };
        if called != definition
            || self.implementation(expression, definition) != Some(definition)
            || !self.identity_grammar_result(self.expr_ty(expression), constructor)
        {
            return Some(None);
        }
        if operands.len() != constructor.source_count {
            return Some(None);
        }
        let Some(coefficient) = self.flow.iterations.checked_mul(2) else {
            return Some(None);
        };
        let required: Option<Vec<_>> = operands.iter().map(|source| {
            if !types::standard_string(self.tcx, self.expr_ty(source)) {
                return None;
            }
            self.key(source, &mut Vec::new()).map(|key| ExtentTerm {
                factors: vec![key],
                coefficient,
            })
        }).collect();
        let Some(required) = required else { return Some(None) };

        let mut remaining = self.flow.work.clone();
        if required.iter().all(|extent| remaining.iter_mut().any(|credit| {
            !credit.opaque
                && storage::consume_terms(&mut credit.extents, std::slice::from_ref(extent))
        })) {
            remaining.retain(|credit| credit.opaque || !credit.extents.is_empty());
            self.flow.work = remaining;
            self.findings.admitted_operations.insert(expression.hir_id);
            return Some(Some(true));
        }

        Some(if self.flow.work.iter().any(|credit| credit.opaque) {
            None
        } else {
            Some(false)
        })
    }
}
