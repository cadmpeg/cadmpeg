// SPDX-License-Identifier: Apache-2.0
use rustc_hir::{def::Res, Expr, ExprKind, Node};
use rustc_middle::ty;
use crate::{types, Analysis};
use types::Shape;

impl<'tcx> Analysis<'_, 'tcx> {
    pub(crate) fn initializer(&self, expression: &Expr<'tcx>) -> Option<&'tcx Expr<'tcx>> {
        let ExprKind::Path(ref path) = expression.kind else { return None; };
        let Res::Local(id) = self.typeck.qpath_res(path, expression.hir_id) else { return None; };
        if self.flow.mutated.contains(&format!("local:{id:?}")) { return None; }
        for (_, node) in self.tcx.hir_parent_iter(id) {
            match node {
                Node::LetStmt(local) => return local.init,
                Node::Param(_) | Node::Item(_) | Node::Expr(_) => return None,
                _ => (),
            }
        }
        None
    }

    pub(crate) fn constant(&self, expression: &Expr<'tcx>, seen: &mut Vec<rustc_hir::HirId>) -> bool {
        if seen.contains(&expression.hir_id) { return false; }
        seen.push(expression.hir_id);
        match expression.kind {
            ExprKind::Lit(_) => true,
            ExprKind::AddrOf(_, _, inner) | ExprKind::Unary(_, inner) | ExprKind::Cast(inner, _) | ExprKind::DropTemps(inner) => self.constant(inner, seen),
            ExprKind::Binary(_, left, right) => self.constant(left, seen) && self.constant(right, seen),
            ExprKind::Path(ref path) => match self.typeck.qpath_res(path, expression.hir_id) {
                Res::Def(rustc_hir::def::DefKind::Const { .. } | rustc_hir::def::DefKind::AssocConst { .. }, _) => true,
                _ => self.initializer(expression).is_some_and(|init| self.constant(init, seen)),
            },
            _ => false,
        }
    }

    pub(crate) fn iteration(&self, expression: &'tcx Expr<'tcx>, seen: &mut Vec<rustc_hir::HirId>) -> Shape {
        if seen.contains(&expression.hir_id) { return Shape::Unknown; }
        seen.push(expression.hir_id);
        match expression.kind {
            ExprKind::AddrOf(_, _, inner) | ExprKind::DropTemps(inner) => return self.iteration(inner, seen),
            ExprKind::Array(_) | ExprKind::Repeat(_, _) => return Shape::Fixed,
            ExprKind::Struct(_, fields, _) => {
                if let ty::Adt(definition, _) = self.typeck.expr_ty(expression).peel_refs().kind() {
                    if types::standard(self.tcx, definition.did()) && self.tcx.item_name(definition.did()).as_str().starts_with("Range") {
                        return if fields.iter().all(|field| self.constant(field.expr, &mut Vec::new())) { Shape::Fixed } else { Shape::Dynamic };
                    }
                }
            }
            _ => (),
        }
        if let Some((definition, operands)) = self.call(expression) {
            let name = self.tcx.item_name(definition);
            if types::standard(self.tcx, definition) && matches!(name.as_str(), "iter" | "iter_mut" | "into_iter" | "enumerate" | "rev" | "copied" | "cloned" | "map" | "filter" | "filter_map" | "take" | "skip" | "step_by" | "take_while" | "skip_while" | "inspect" | "chain" | "zip" | "flatten" | "flat_map") {
                let shape = operands.first().map_or(Shape::Unknown, |operand| self.iteration(operand, seen));
                if matches!(name.as_str(), "flatten" | "flat_map") { return if shape == Shape::Fixed { Shape::Unknown } else { shape }; }
                if matches!(name.as_str(), "chain" | "zip") { return shape.join(operands.get(1).map_or(Shape::Unknown, |operand| self.iteration(operand, seen))); }
                return shape;
            }
        }
        let value = self.typeck.expr_ty(expression).peel_refs();
        if matches!(value.kind(), ty::Array(_, _)) { return Shape::Fixed; }
        if let Some(init) = self.initializer(expression) { return self.iteration(init, seen); }
        match value.kind() {
            ty::Str | ty::Slice(_) => Shape::Dynamic,
            ty::Adt(definition, _) => {
                let path = self.tcx.def_path_str(definition.did());
                let name = self.tcx.item_name(definition.did());
                if types::standard(self.tcx, definition.did()) && matches!(name.as_str(), "Option" | "Result" | "Once" | "Empty") { return Shape::Fixed; }
                if path.contains("slice::iter") || path.contains("str::iter") || path.contains("collections") || path.contains("vec::") || path.contains("range::") || name.as_str() == "View" { return Shape::Dynamic; }
                Shape::Unknown
            }
            _ => Shape::Unknown,
        }
    }
}
