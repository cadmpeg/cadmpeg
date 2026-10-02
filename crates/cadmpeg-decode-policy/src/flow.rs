// SPDX-License-Identifier: Apache-2.0
use rustc_hir::{def::Res, Expr, ExprKind, HirId, MatchSource, Node};
use crate::{Analysis, types};

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Credit {
    pub(crate) extents: Vec<String>,
    pub(crate) opaque: bool,
}

#[derive(Clone, Default)]
pub(crate) struct Flow {
    pub(crate) work: Vec<Credit>,
    pub(crate) storage: bool,
    pub(crate) mutated: std::collections::HashSet<String>,
}

impl<'tcx> Analysis<'_, 'tcx> {
    pub(crate) fn key(&self, expression: &'tcx Expr<'tcx>, seen: &mut Vec<HirId>) -> Option<String> {
        if seen.contains(&expression.hir_id) { return None; }
        seen.push(expression.hir_id);
        match expression.kind {
            ExprKind::AddrOf(_, _, inner) | ExprKind::DropTemps(inner) | ExprKind::Unary(_, inner) => self.key(inner, seen),
            ExprKind::Path(ref path) => match self.typeck.qpath_res(path, expression.hir_id) {
                Res::Local(id) => Some(format!("local:{id:?}")),
                Res::Def(_, id) => Some(format!("definition:{id:?}")),
                _ => None,
            },
            ExprKind::Struct(_, fields, _) => {
                if let rustc_middle::ty::Adt(definition, _) = self.typeck.expr_ty(expression).kind() {
                    if types::standard(self.tcx, definition.did()) && self.tcx.item_name(definition.did()).as_str() == "Range" {
                        let start = fields.iter().find(|field| field.ident.name.as_str() == "start")?.expr;
                        let end = fields.iter().find(|field| field.ident.name.as_str() == "end")?.expr;
                        if matches!(start.kind, ExprKind::Lit(literal) if matches!(literal.node, rustc_ast::LitKind::Int(value, _) if value.get() == 0)) {
                            let terms = self.extent_terms(end, &mut Vec::new())?;
                            if let [term] = terms.as_slice() { return Some(term.clone()); }
                        }
                    }
                }
                None
            }
            ExprKind::Field(base, field) => self.key(base, seen).map(|key| format!("{key}.{}", field.name)),
            ExprKind::Index(base, _, _) => self.key(base, seen).map(|key| format!("{key}.window")),
            _ => {
                let (definition, operands) = self.call(expression)?;
                let name = self.tcx.item_name(definition);
                if types::standard(self.tcx, definition) && matches!(name.as_str(), "iter" | "iter_mut" | "into_iter" | "enumerate" | "rev" | "copied" | "cloned" | "map" | "filter" | "take" | "skip") || matches!(name.as_str(), "window" | "as_bytes" | "as_slice") {
                    operands.first().and_then(|operand| self.key(operand, seen))
                } else { None }
            }
        }
    }

    pub(crate) fn extent_terms(&self, expression: &'tcx Expr<'tcx>, seen: &mut Vec<HirId>) -> Option<Vec<String>> {
        if seen.contains(&expression.hir_id) { return None; }
        seen.push(expression.hir_id);
        match expression.kind {
            ExprKind::AddrOf(_, _, inner) | ExprKind::DropTemps(inner) => return self.extent_terms(inner, seen),
            ExprKind::Cast(inner, _) => {
                let source = self.typeck.expr_ty(inner);
                let destination = self.typeck.expr_ty(expression);
                if source == destination || matches!((source.kind(), destination.kind()), (rustc_middle::ty::Uint(left), rustc_middle::ty::Uint(right)) if left.bit_width().or(Some(self.tcx.data_layout.pointer_size().bits())).zip(right.bit_width().or(Some(self.tcx.data_layout.pointer_size().bits()))).is_some_and(|(left, right)| left <= right)) { return self.extent_terms(inner, seen); }
                return None;
            }
            ExprKind::Binary(operator, left, right) if operator.node == rustc_hir::BinOpKind::Add => {
                let mut terms = self.extent_terms(left, seen)?;
                terms.extend(self.extent_terms(right, seen)?);
                return Some(terms);
            }
            ExprKind::Match(scrutinee, _, MatchSource::TryDesugar(_)) => {
                let (_, args) = self.call(scrutinee)?;
                return args.first().and_then(|arg| self.extent_terms(arg, seen));
            }
            _ => (),
        }
        if let Some((definition, operands)) = self.call(expression) {
            let name = self.tcx.item_name(definition);
            if matches!(name.as_str(), "len" | "capacity") && types::standard(self.tcx, definition) || name.as_str() == "len" && self.tcx.def_path_str(definition).contains("View") {
                return operands.first().and_then(|operand| self.key(operand, &mut Vec::new())).map(|key| vec![key]);
            }
            if matches!(name.as_str(), "u64_from_index" | "from" | "ok_or" | "ok_or_else") {
                return operands.first().and_then(|operand| self.extent_terms(operand, seen));
            }
            if name.as_str() == "checked_add" {
                let mut terms = self.extent_terms(operands.first()?, seen)?;
                terms.extend(self.extent_terms(operands.get(1)?, seen)?);
                return Some(terms);
            }
        }
        if let Some(init) = self.initializer(expression) { return self.extent_terms(init, seen); }
        self.key(expression, &mut Vec::new()).map(|key| vec![key])
    }

    pub(crate) fn propagated(&self, expression: &'tcx Expr<'tcx>) -> bool {
        for (_, node) in self.tcx.hir_parent_iter(expression.hir_id) {
            match node {
                Node::Expr(parent) => match parent.kind {
                    ExprKind::Match(_, _, MatchSource::TryDesugar(_)) => return true,
                    ExprKind::Call(_, _) => {
                        if self.call(parent).is_none_or(|(id, _)| self.tcx.item_name(id).as_str() != "branch") { return false; }
                    }
                    ExprKind::MethodCall(_, _, _, _) => {
                        let Some((id, args)) = self.call(parent) else { return false; };
                        if self.tcx.item_name(id).as_str() != "map_err" { return false; }
                        let Some(mapper) = args.get(1) else { return false; };
                        let ExprKind::Path(ref path) = mapper.kind else { return false; };
                        if !matches!(self.typeck.qpath_res(path, mapper.hir_id), Res::Def(_, id) if self.tcx.item_name(id).as_str() == "ResourceLimit") { return false; }
                    }
                    ExprKind::AddrOf(_, _, _) | ExprKind::DropTemps(_) => (),
                    _ => return false,
                },
                _ => return false,
            }
        }
        false
    }

    fn context_operand(&self, expression: &'tcx Expr<'tcx>) -> bool {
        if types::has_context(self.tcx, self.typeck.expr_ty(expression), &mut Vec::new()) { return true; }
        match expression.kind {
            ExprKind::Field(base, _) | ExprKind::AddrOf(_, _, base) => self.context_operand(base),
            _ => false,
        }
    }

    pub(crate) fn context_operation(&self, expression: &'tcx Expr<'tcx>) -> bool {
        self.call(expression).is_some_and(|(_, operands)| operands.iter().any(|operand| self.context_operand(operand)))
    }

    pub(crate) fn record_charge(&mut self, expression: &'tcx Expr<'tcx>) {
        let Some((definition, operands)) = self.call(expression) else { return; };
        let name = self.tcx.item_name(definition);
        if !self.context_operation(expression) || !self.propagated(expression) { return; }
        if matches!(name.as_str(), "charge_retained" | "charge_retained_limit" | "reserve_scoped" | "reserve_scoped_limit" | "reserve_vec" | "reserve_vec_limit" | "collection_vec" | "vector_storage" | "reserve_capacity" | "try_reserve_retained_text" | "reserve_set" | "reserve_map" | "reserve_scoped_vec" | "reserve_scoped_vec_limit" | "reserve_temporary_vec" | "charge_hash_growth" | "admit_btree_entry" | "admit_btree_node_storage" | "admit_retained_btree_record" | "charge_input" | "linear_growth" | "admit_hash_map_entry" | "scoped_vector_storage" | "reserve_retained_vec_storage") { self.flow.storage = true; }
        if !matches!(name.as_str(), "charge_work" | "charge_work_limit") { return; }
        if let Some(amount) = operands.get(1) {
            if matches!(amount.kind, ExprKind::Lit(literal) if matches!(literal.node, rustc_ast::LitKind::Int(value, _) if value.get() == 0)) { return; }
            let terms = self.extent_terms(amount, &mut Vec::new());
            self.flow.work.push(Credit { opaque: terms.is_none(), extents: terms.unwrap_or_default() });
        }
    }

    pub(crate) fn take_credit(&mut self, operands: &[&'tcx Expr<'tcx>]) -> Option<bool> {
        let extents: Vec<_> = operands.iter().filter_map(|operand| self.key(operand, &mut Vec::new())).collect();
        if let Some(index) = self.flow.work.iter().position(|credit| !credit.opaque && !extents.is_empty() && credit.extents.iter().all(|term| extents.contains(term))) {
            self.flow.work.remove(index);
            return Some(true);
        }
        if self.flow.work.iter().any(|credit| credit.opaque) { return None; }
        Some(false)
    }

    pub(crate) fn invalidate_target(&mut self, expression: &'tcx Expr<'tcx>) {
        if let Some(key) = self.key(expression, &mut Vec::new()) {
            self.flow.work.retain(|credit| !credit.extents.iter().any(|term| term == &key || term.starts_with(&format!("{key}.")) || key.starts_with(&format!("{term}."))));
            self.flow.mutated.insert(key);
        } else {
            for credit in &mut self.flow.work { credit.opaque = true; }
        }
    }

    pub(crate) fn mutation(&mut self, expression: &'tcx Expr<'tcx>) {
        if let ExprKind::Assign(target, _, _) | ExprKind::AssignOp(_, target, _) = expression.kind { self.invalidate_target(target); }
        if let Some((_, operands)) = self.call(expression) {
            for operand in operands {
                if matches!(self.typeck.expr_ty_adjusted(operand).kind(), rustc_middle::ty::Ref(_, _, rustc_hir::Mutability::Mut)) { self.invalidate_target(operand); }
            }
        }
    }
}
