// SPDX-License-Identifier: Apache-2.0
//! Parser receipts couple a sealed input with live, operation-specific admission.
use crate::{types, Analysis};
use rustc_hir::{Expr, ExprKind, Node};
use rustc_middle::ty;
use rustc_span::Span;

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ParserReceipt {
    pub(crate) guard: String,
    pub(crate) scope: Span,
}

impl<'tcx> Analysis<'_, 'tcx> {
    fn xml_admission_type(&self, value: ty::Ty<'tcx>) -> bool {
        matches!(value.peel_refs().kind(), ty::Adt(owner, _)
            if self.tcx.item_name(owner.did()).as_str() == "XmlParserAdmission"
                && (self.tcx.crate_name(owner.did().krate).as_str() == "cadmpeg_core"
                    || std::env::var_os("CADMPEG_POLICY_FIXTURE").is_some()))
    }

    pub(crate) fn record_parser_admission(&mut self, expression: &'tcx Expr<'tcx>) {
        if !self.trusted_context_callee(expression) || !self.context_operation(expression)
            || !self.propagated(expression) { return; }
        let Some((definition, _)) = self.call(expression) else { return; };
        if self.tcx.item_name(definition).as_str() != "xml_parser_admission" { return; }
        let ty::Adt(result, args) = self.expr_ty(expression).kind() else { return; };
        if !types::standard(self.tcx, result.did()) || self.tcx.item_name(result.did()).as_str() != "Result"
            || !args.types().next().is_some_and(|value| self.xml_admission_type(value)) { return; }
        let Some(guard) = self.result_binding(expression) else { return; };
        let Some(scope) = self.tcx.hir_parent_iter(expression.hir_id).find_map(|(_, node)| match node {
            Node::Block(block) => Some(block.span),
            _ => None,
        }) else { return; };
        self.flow.parser_receipts.push(ParserReceipt { guard, scope });
    }

    pub(crate) fn parser_call_paid(&mut self, expression: &'tcx Expr<'tcx>) -> bool {
        let Some((definition, operands)) = self.call(expression) else { return false; };
        if self.tcx.crate_name(definition.krate).as_str() != "roxmltree"
            || self.tcx.item_name(definition).as_str() != "parse_with_options" { return false; }
        let Some(input) = operands.first() else { return false; };
        let ExprKind::Field(admission, field) = input.kind else { return false; };
        if field.name.as_str() != "text" || !self.xml_admission_type(self.expr_ty(admission)) { return false; }
        let Some(guard) = self.key(admission, &mut Vec::new()) else { return false; };
        let Some(index) = self.flow.parser_receipts.iter().position(|receipt|
            receipt.guard == guard && receipt.scope.contains(expression.span)
                && !self.flow.mutated.iter().any(|key| crate::flow::factor_depends_on(&guard, key))) else { return false; };
        self.flow.parser_receipts.remove(index);
        self.findings.admitted_operations.insert(expression.hir_id);
        true
    }
}
