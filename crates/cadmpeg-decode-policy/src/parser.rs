// SPDX-License-Identifier: Apache-2.0
//! External-operation receipts couple a sealed input with live, operation-specific admission.
use crate::{types, Analysis};
use rustc_hir::{Expr, ExprKind, Node};
use rustc_middle::ty;
use rustc_span::Span;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParserKind<'tcx> {
    UnicodeCase,
    Xml,
    Zip,
    ZstdStep,
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
        let ty::Adt(owner, _) = value.peel_refs().kind() else {
            return None;
        };
        if !matches!(
            self.tcx.crate_name(owner.did().krate).as_str(),
            "cadmpeg_core" | "cadmpeg_container"
        ) && std::env::var_os("CADMPEG_POLICY_FIXTURE").is_none()
        {
            return None;
        }
        match self.tcx.item_name(owner.did()).as_str() {
            "UnicodeCaseAdmission" => Some("UnicodeCaseAdmission"),
            "XmlParserAdmission" => Some("XmlParserAdmission"),
            "ZipParserAdmission" => Some("ZipParserAdmission"),
            "ZstdStepAdmission" => Some("ZstdStepAdmission"),
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
        let Some((definition, _)) = self.call(expression) else {
            return;
        };
        let container = self.tcx.crate_name(definition.krate).as_str() == "cadmpeg_container"
            && self.tcx.item_name(definition).as_str() == "zip_parser_admission";
        if !(self.trusted_context_callee(expression) || container)
            || !self.context_operation(expression)
            || !self.propagated(expression)
        {
            return;
        }
        let ty::Adt(result, args) = self.expr_ty(expression).kind() else {
            return;
        };
        if !types::standard(self.tcx, result.did())
            || self.tcx.item_name(result.did()).as_str() != "Result"
        {
            return;
        }
        let Some(value) = args.types().next() else {
            return;
        };
        let kind = match (
            self.tcx.item_name(definition).as_str(),
            self.parser_type_name(value),
        ) {
            ("unicode_case_admission", Some("UnicodeCaseAdmission")) => ParserKind::UnicodeCase,
            ("xml_parser_admission", Some("XmlParserAdmission")) => ParserKind::Xml,
            ("zip_parser_admission", Some("ZipParserAdmission")) => ParserKind::Zip,
            ("zstd_step_admission", Some("ZstdStepAdmission")) => ParserKind::ZstdStep,
            ("json_parser_admission", Some("JsonParserAdmission")) => ParserKind::JsonTree,
            (
                name @ ("json_validation_admission" | "json_conversion_admission"),
                Some("TypedJsonAdmission"),
            ) => {
                let ty::Adt(_, args) = value.kind() else {
                    return;
                };
                let mut types = args.types();
                let Some(target) = types.next() else {
                    return;
                };
                let Some(source) = types.next() else {
                    return;
                };
                if name == "json_validation_admission"
                    && matches!(source.peel_refs().kind(), ty::Str)
                {
                    ParserKind::JsonValidation(self.tcx.erase_and_anonymize_regions(target))
                } else if name == "json_conversion_admission" && self.json_value_type(source) {
                    ParserKind::JsonConversion(self.tcx.erase_and_anonymize_regions(target))
                } else {
                    return;
                }
            }
            _ => return,
        };
        let Some(guard) = self.result_binding(expression) else {
            return;
        };
        let Some(scope) = self
            .tcx
            .hir_parent_iter(expression.hir_id)
            .find_map(|(_, node)| match node {
                Node::Block(block) => Some(block.span),
                _ => None,
            })
        else {
            return;
        };
        self.flow
            .parser_receipts
            .push(ParserReceipt { guard, scope, kind });
    }

    fn zstd_step_paid(
        &mut self,
        expression: &'tcx Expr<'tcx>,
        operands: &[&'tcx Expr<'tcx>],
    ) -> bool {
        let Some((definition, _)) = self.call(expression) else {
            return false;
        };
        if self.tcx.crate_name(definition.krate).as_str() != "zstd_safe"
            || self.tcx.item_name(definition).as_str() != "decompress_stream"
            || operands.len() != 3
        {
            return false;
        }
        let mut guard = None;
        for (operand, field_name) in operands.iter().zip(["decoder", "output", "input"]) {
            let mut operand = *operand;
            while let ExprKind::AddrOf(_, _, inner) | ExprKind::DropTemps(inner) = operand.kind {
                operand = inner;
            }
            let ExprKind::Field(admission, field) = operand.kind else {
                return false;
            };
            if field.name.as_str() != field_name
                || self.parser_type_name(self.expr_ty(admission)) != Some("ZstdStepAdmission")
            {
                return false;
            }
            let Some(key) = self.key(admission, &mut Vec::new()) else {
                return false;
            };
            if guard.as_ref().is_some_and(|guard| guard != &key) {
                return false;
            }
            guard = Some(key);
            if field_name == "output" {
                let ty::Adt(owner, args) = self.expr_ty(operand).peel_refs().kind() else {
                    return false;
                };
                if self.tcx.crate_name(owner.did().krate).as_str() != "zstd_safe"
                    || self.tcx.item_name(owner.did()).as_str() != "OutBuffer"
                    || !args.types().next().is_some_and(|value| {
                        matches!(value.kind(), ty::Slice(element)
                        if matches!(element.kind(), ty::Uint(ty::UintTy::U8)))
                    })
                {
                    return false;
                }
            }
        }
        let Some(guard) = guard else {
            return false;
        };
        let Some(index) = self.flow.parser_receipts.iter().position(|receipt| {
            receipt.kind == ParserKind::ZstdStep
                && receipt.guard == guard
                && receipt.scope.contains(expression.span)
                && !self
                    .flow
                    .mutated
                    .iter()
                    .any(|key| crate::flow::factor_depends_on(&guard, key))
        }) else {
            return false;
        };
        self.flow.parser_receipts.remove(index);
        self.findings.admitted_operations.insert(expression.hir_id);
        true
    }

    pub(crate) fn parser_call_paid(&mut self, expression: &'tcx Expr<'tcx>) -> bool {
        let Some((definition, operands)) = self.call(expression) else {
            return false;
        };
        if self.zstd_step_paid(expression, &operands) {
            return true;
        }
        let Some(mut input) = operands.first().copied() else {
            return false;
        };
        let zip = self.tcx.crate_name(definition.krate).as_str() == "zip"
            && self.tcx.item_name(definition).as_str() == "new";
        if zip {
            let Some((cursor, args)) = self.call(input) else {
                return false;
            };
            if !types::standard(self.tcx, cursor)
                || self.tcx.item_name(cursor).as_str() != "new"
                || !matches!(self.expr_ty(input).kind(), ty::Adt(owner, _) if types::standard(self.tcx, owner.did())
                    && self.tcx.item_name(owner.did()).as_str() == "Cursor")
            {
                return false;
            }
            let Some(bytes) = args.first().copied() else {
                return false;
            };
            input = bytes;
        }
        let ExprKind::Field(admission, field) = input.kind else {
            return false;
        };
        let Some(type_name) = self.parser_type_name(self.expr_ty(admission)) else {
            return false;
        };
        let expected_field = match type_name {
            "TypedJsonAdmission" => "source",
            "ZipParserAdmission" => "bytes",
            _ => "text",
        };
        if field.name.as_str() != expected_field {
            return false;
        }
        let Some(guard) = self.key(admission, &mut Vec::new()) else {
            return false;
        };
        let owner = self.tcx.crate_name(definition.krate);
        let name = self.tcx.item_name(definition);
        let target = self
            .call_arguments(expression)
            .and_then(|args| args.types().next())
            .map(|value| self.tcx.erase_and_anonymize_regions(value));
        let Some(index) = self.flow.parser_receipts.iter().position(|receipt| {
            if receipt.guard != guard || !receipt.scope.contains(expression.span)
                || self.flow.mutated.iter().any(|key| crate::flow::factor_depends_on(&guard, key)) { return false; }
            match receipt.kind {
                ParserKind::UnicodeCase => types::standard(self.tcx, definition)
                    && matches!(name.as_str(), "to_lowercase" | "to_uppercase")
                    && matches!(self.expr_ty(input).peel_refs().kind(), ty::Str),
                ParserKind::ZstdStep => false,
                ParserKind::Xml => owner.as_str() == "roxmltree" && name.as_str() == "parse_with_options",
                ParserKind::Zip => zip && matches!(self.expr_ty(expression).kind(), ty::Adt(result, args)
                    if types::standard(self.tcx, result.did()) && self.tcx.item_name(result.did()).as_str() == "Result"
                        && args.types().next().is_some_and(|value| matches!(value.kind(), ty::Adt(owner, _)
                            if self.tcx.crate_name(owner.did().krate).as_str() == "zip"
                                && self.tcx.item_name(owner.did()).as_str() == "ZipArchive"))),
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
