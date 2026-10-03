// SPDX-License-Identifier: Apache-2.0
//! Parser receipts couple a sealed input with live, operation-specific admission.
use crate::{types, Analysis};
use rustc_hir::{Expr, ExprKind, Node};
use rustc_middle::ty;
use rustc_span::Span;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParserKind<'tcx> {
    Xml,
    JsonTree,
    JsonValidation(ty::Ty<'tcx>),
    JsonConversion(ty::Ty<'tcx>),
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ParserReceipt<'tcx> {
    pub(crate) guard: String,
    pub(crate) scope: Span,
    pub(crate) kind: ParserKind<'tcx>,
}

impl<'tcx> Analysis<'_, 'tcx> {
    fn parser_type_name(&self, value: ty::Ty<'tcx>) -> Option<&str> {
        let ty::Adt(owner, _) = value.peel_refs().kind() else { return None; };
        if self.tcx.crate_name(owner.did().krate).as_str() != "cadmpeg_core"
            && std::env::var_os("CADMPEG_POLICY_FIXTURE").is_none() { return None; }
        match self.tcx.item_name(owner.did()).as_str() {
            "XmlParserAdmission" => Some("XmlParserAdmission"),
            "JsonParserAdmission" => Some("JsonParserAdmission"),
            "TypedJsonAdmission" => Some("TypedJsonAdmission"),
            _ => None,
        }
    }

    fn json_value_type(&self, value: ty::Ty<'tcx>) -> bool {
        matches!(value.peel_refs().kind(), ty::Adt(owner, _)
            if self.tcx.crate_name(owner.did().krate).as_str() == "serde_json"
                && self.tcx.item_name(owner.did()).as_str() == "Value")
    }

    pub(crate) fn record_parser_admission(&mut self, expression: &'tcx Expr<'tcx>) {
        if !self.trusted_context_callee(expression) || !self.context_operation(expression)
            || !self.propagated(expression) { return; }
        let Some((definition, _)) = self.call(expression) else { return; };
        let ty::Adt(result, args) = self.expr_ty(expression).kind() else { return; };
        if !types::standard(self.tcx, result.did()) || self.tcx.item_name(result.did()).as_str() != "Result" { return; }
        let Some(value) = args.types().next() else { return; };
        let kind = match (self.tcx.item_name(definition).as_str(), self.parser_type_name(value)) {
            ("xml_parser_admission", Some("XmlParserAdmission")) => ParserKind::Xml,
            ("json_parser_admission", Some("JsonParserAdmission")) => ParserKind::JsonTree,
            (name @ ("json_validation_admission" | "json_conversion_admission"), Some("TypedJsonAdmission")) => {
                let ty::Adt(_, args) = value.kind() else { return; };
                let mut types = args.types();
                let Some(target) = types.next() else { return; };
                let Some(source) = types.next() else { return; };
                if name == "json_validation_admission" && matches!(source.peel_refs().kind(), ty::Str) {
                    ParserKind::JsonValidation(self.tcx.erase_and_anonymize_regions(target))
                } else if name == "json_conversion_admission" && self.json_value_type(source) {
                    ParserKind::JsonConversion(self.tcx.erase_and_anonymize_regions(target))
                } else { return; }
            }
            _ => return,
        };
        let Some(guard) = self.result_binding(expression) else { return; };
        let Some(scope) = self.tcx.hir_parent_iter(expression.hir_id).find_map(|(_, node)| match node {
            Node::Block(block) => Some(block.span),
            _ => None,
        }) else { return; };
        self.flow.parser_receipts.push(ParserReceipt { guard, scope, kind });
    }

    pub(crate) fn parser_call_paid(&mut self, expression: &'tcx Expr<'tcx>) -> bool {
        let Some((definition, operands)) = self.call(expression) else { return false; };
        let Some(input) = operands.first() else { return false; };
        let ExprKind::Field(admission, field) = input.kind else { return false; };
        let Some(type_name) = self.parser_type_name(self.expr_ty(admission)) else { return false; };
        let expected_field = if type_name == "TypedJsonAdmission" { "source" } else { "text" };
        if field.name.as_str() != expected_field { return false; }
        let Some(guard) = self.key(admission, &mut Vec::new()) else { return false; };
        let owner = self.tcx.crate_name(definition.krate);
        let name = self.tcx.item_name(definition);
        let target = self.call_arguments(expression).and_then(|args| args.types().next())
            .map(|value| self.tcx.erase_and_anonymize_regions(value));
        let Some(index) = self.flow.parser_receipts.iter().position(|receipt| {
            if receipt.guard != guard || !receipt.scope.contains(expression.span)
                || self.flow.mutated.iter().any(|key| crate::flow::factor_depends_on(&guard, key)) { return false; }
            match receipt.kind {
                ParserKind::Xml => owner.as_str() == "roxmltree" && name.as_str() == "parse_with_options",
                ParserKind::JsonTree => owner.as_str() == "serde_json" && name.as_str() == "from_str"
                    && target.is_some_and(|value| self.json_value_type(value)
                        || matches!(value.kind(), ty::Adt(owner, _)
                            if self.tcx.crate_name(owner.did().krate).as_str() == "cadmpeg_core"
                                && self.tcx.item_name(owner.did()).as_str() == "PlainJson")),
                ParserKind::JsonValidation(expected) => owner.as_str() == "serde_json"
                    && name.as_str() == "from_str" && target == Some(expected),
                ParserKind::JsonConversion(expected) => owner.as_str() == "serde_json"
                    && name.as_str() == "from_value" && target == Some(expected),
            }
        }) else { return false; };
        self.flow.parser_receipts.remove(index);
        self.findings.admitted_operations.insert(expression.hir_id);
        true
    }
}
