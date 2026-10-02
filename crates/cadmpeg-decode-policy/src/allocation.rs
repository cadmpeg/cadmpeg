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
        if types::standard(self.tcx, definition) && matches!(name, "new" | "default" | "must_use" | "box_assume_init_into_vec_unsafe" | "branch" | "from_residual" | "from_output") { return; }
        let context_call = operands.first().is_some_and(|operand| types::has_context(self.tcx, self.typeck.expr_ty(operand), &mut Vec::new()));
        if context_call {
            if name == "alloc_filled" {
                if let Some(value) = operands.get(2) {
                    let shape = self.clone_shape(self.typeck.expr_ty(value));
                    let empty = self.call(value).is_some_and(|(id, args)| types::standard(self.tcx, id) && args.is_empty() && matches!(self.tcx.item_name(id).as_str(), "new" | "default"));
                    if !empty { self.shape_report(expression, shape, "alloc_filled child Clone"); }
                }
            }
            return;
        }
        if matches!(name, "push" | "push_back" | "push_front" | "push_str" | "insert" | "extend" | "extend_from_slice" | "append" | "resize" | "resize_with" | "reserve" | "reserve_exact" | "try_reserve" | "try_reserve_exact") && types::standard(self.tcx, definition) {
            if let Some(receiver) = operands.first() {
                let shape = types::heap(self.tcx, self.typeck.expr_ty(receiver).peel_refs(), &mut Vec::new());
                self.shape_report(expression, if self.flow.storage && shape == Shape::Dynamic { Shape::Unknown } else { shape }, "collection growth outside core operation");
            }
            return;
        }
        if name == "cloned" && types::standard(self.tcx, definition) {
            let value = operands.first().and_then(|operand| self.iterator_item(self.typeck.expr_ty(operand))).map(|value| value.peel_refs());
            let shape = value.map_or(Shape::Unknown, |value| self.clone_shape(value));
            if shape != Shape::Fixed {
                let consumed = self.tcx.hir_parent_iter(expression.hir_id).any(|(_, node)| match node {
                    rustc_hir::Node::Expr(parent) => self.call(parent).is_some_and(|(id, _)| matches!(self.tcx.item_name(id).as_str(), "collect" | "from_iter" | "collect_vec" | "try_collect_vec" | "any" | "find" | "for_each")),
                    _ => false,
                });
                self.shape_report(expression, if consumed { shape } else { Shape::Unknown }, "cloned iterator child copies");
            }
            return;
        }
        if !matches!(name, "to_string" | "to_owned" | "clone" | "format" | "to_vec" | "collect" | "from" | "into" | "from_elem" | "with_capacity" | "with_capacity_in" | "from_iter" | "repeat" | "concat" | "join" | "into_boxed_slice" | "into_owned") {
            if types::heap(self.tcx, self.typeck.expr_ty(expression), &mut Vec::new()) != Shape::Fixed {
                if self.tcx.def_kind(definition) == rustc_hir::def::DefKind::Ctor(rustc_hir::def::CtorOf::Struct, rustc_hir::def::CtorKind::Fn) || self.tcx.def_kind(definition) == rustc_hir::def::DefKind::Ctor(rustc_hir::def::CtorOf::Variant, rustc_hir::def::CtorKind::Fn) { return; }
                if operands.iter().any(|operand| types::has_context(self.tcx, self.typeck.expr_ty(operand), &mut Vec::new())) { return; }
                if self.local_has_effects(definition) != Some(false) {
                    self.shape_report(expression, Shape::Unknown, "constructor or opaque callee: allocator reachability unresolved");
                }
            }
            return;
        }
        let result = types::heap(self.tcx, self.typeck.expr_ty(expression), &mut Vec::new());
        if result == Shape::Fixed { return; }
        if name == "format" && types::standard(self.tcx, definition) {
            let mut format = FormatShape { analysis: self, shape: Shape::Fixed };
            for operand in operands { format.visit_expr(operand); }
            let shape = format.shape;
            self.shape_report(expression, shape, "format!");
            return;
        }
        if matches!(name, "to_string" | "to_owned" | "clone") && operands.first().is_some_and(|operand| self.fixed_value(operand)) { return; }
        if types::standard(self.tcx, definition) {
            if matches!(name, "collect" | "from_iter" | "to_vec") {
                let shape = operands.first().map_or(Shape::Unknown, |operand| self.iteration(operand, &mut Vec::new()));
                self.shape_report(expression, shape, name);
                return;
            }
            if matches!(name, "from" | "into" | "into_owned") {
                if let Some(operand) = operands.first() {
                    if self.constant(operand, &mut Vec::new()) { return; }
                    if self.typeck.expr_ty(operand) == self.typeck.expr_ty(expression) { return; }
                    let shape = match self.typeck.expr_ty(operand).peel_refs().kind() {
                        ty::Str | ty::Slice(_) => Shape::Dynamic,
                        ty::Array(_, _) => Shape::Fixed,
                        _ => Shape::Unknown,
                    };
                    self.shape_report(expression, shape, name);
                    return;
                }
            }
            if matches!(name, "with_capacity" | "with_capacity_in" | "from_elem") {
                let count = if name == "from_elem" { operands.get(1) } else { operands.first() };
                if count.is_some_and(|count| self.constant(count, &mut Vec::new())) {
                    if name != "from_elem" || operands.first().is_some_and(|value| types::heap(self.tcx, self.typeck.expr_ty(value), &mut Vec::new()) == Shape::Fixed) { return; }
                }
                self.shape_report(expression, Shape::Dynamic, name);
                return;
            }
            if name == "into_boxed_slice" { self.shape_report(expression, Shape::Unknown, "into_boxed_slice may shrink/reallocate: capacity equality unresolved"); return; }
        }
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
                            if self.stack.contains(&local) { self.shape_report(expression, Shape::Unknown, "recursive custom Clone"); return; }
                            let mut stack = self.stack.clone();
                            stack.push(local);
                            let mut child = Analysis { tcx: self.tcx, typeck: self.tcx.typeck(local), owner: local, summaries: self.summaries, flow: crate::flow::Flow::default(), stack, findings: self.findings };
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
