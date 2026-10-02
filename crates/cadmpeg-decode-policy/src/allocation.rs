// SPDX-License-Identifier: Apache-2.0
use rustc_hir::intravisit::{walk_expr, Visitor};
use rustc_hir::{Expr, ExprKind};
use rustc_middle::ty::{self, Instance, TypingEnv};
use crate::{types, Analysis};
use types::Shape;

impl<'tcx> Analysis<'_, 'tcx> {
    pub(crate) fn fixed_value(&self, expression: &Expr<'tcx>) -> bool {
        match expression.kind {
            ExprKind::Lit(_) => true,
            ExprKind::AddrOf(_, _, inner) | ExprKind::DropTemps(inner) => self.fixed_value(inner),
            _ => {
                let value = self.typeck.expr_ty(expression).peel_refs();
                types::heap(self.tcx, value, &mut Vec::new()) == Shape::Fixed && match value.kind() {
                    ty::Str | ty::Slice(_) => false,
                    _ => self.tcx.type_is_copy_modulo_regions(TypingEnv::post_analysis(self.tcx, self.owner), value),
                }
            }
        }
    }

    pub(crate) fn allocation(&mut self, expression: &'tcx Expr<'tcx>) {
        let Some((definition, operands)) = self.call(expression) else { return; };
        let name = self.tcx.item_name(definition);
        let name = name.as_str();
        if !matches!(name, "to_string" | "to_owned" | "clone" | "format") { return; }
        let result = types::heap(self.tcx, self.typeck.expr_ty(expression), &mut Vec::new());
        if result == Shape::Fixed { return; }
        if name == "format" && types::standard(self.tcx, definition) {
            let mut format = FormatShape { analysis: self, shape: Shape::Fixed };
            for operand in operands { format.visit_expr(operand); }
            let shape = format.shape;
            self.shape_report(expression, shape, "format!");
            return;
        }
        if operands.first().is_some_and(|operand| self.fixed_value(operand)) { return; }
        if name == "clone" {
            let args = match expression.kind {
                ExprKind::Call(callee, _) => self.typeck.node_args(callee.hir_id),
                _ => self.typeck.node_args(expression.hir_id),
            };
            match Instance::try_resolve(self.tcx, TypingEnv::post_analysis(self.tcx, self.owner), definition, args) {
                Ok(Some(instance)) if !types::standard(self.tcx, instance.def_id()) => {
                    let parent = self.tcx.parent(instance.def_id());
                    if !self.tcx.is_automatically_derived(parent) {
                        // A custom Clone is inspected at its implementation, not
                        // inferred to allocate from the result's ownership.
                        if let Some(local) = instance.def_id().as_local() {
                            let mut child = Analysis { tcx: self.tcx, typeck: self.tcx.typeck(local), owner: local, findings: self.findings };
                            child.visit_body(self.tcx.hir_body_owned_by(local));
                        } else { self.shape_report(expression, Shape::Unknown, "custom Clone body unavailable"); }
                        return;
                    }
                }
                Ok(None) | Err(_) => { self.shape_report(expression, Shape::Unknown, "Clone implementation unresolved"); return; }
                _ => (),
            }
        }
        self.shape_report(expression, result, name);
    }
}

struct FormatShape<'a, 'b, 'tcx> {
    analysis: &'a Analysis<'b, 'tcx>,
    shape: Shape,
}

impl<'tcx> Visitor<'tcx> for FormatShape<'_, '_, 'tcx> {
    fn visit_expr(&mut self, expression: &'tcx Expr<'tcx>) {
        if let Some((definition, operands)) = self.analysis.call(expression) {
            let name = self.analysis.tcx.item_name(definition);
            if types::standard(self.analysis.tcx, definition) && (name.as_str().starts_with("new_") || name.as_str() == "from_usize") && self.analysis.tcx.def_path_str(definition).contains("fmt") {
                for operand in operands {
                    if !self.analysis.fixed_value(operand) {
                        let value = self.analysis.typeck.expr_ty(operand).peel_refs();
                        let shape = match value.kind() {
                            ty::Str | ty::Slice(_) => Shape::Dynamic,
                            _ => types::heap(self.analysis.tcx, value, &mut Vec::new()),
                        };
                        self.shape = self.shape.join(if shape == Shape::Fixed { Shape::Unknown } else { shape });
                    } else if name.as_str() == "from_usize" && !matches!(operand.kind, ExprKind::Lit(_)) {
                        self.shape = self.shape.join(Shape::Unknown);
                    }
                }
            }
        }
        walk_expr(self, expression);
    }
}
