// SPDX-License-Identifier: Apache-2.0
use rustc_hir::intravisit::{walk_expr, Visitor};
use rustc_hir::{BinOpKind, Expr, ExprKind, LoopSource, MatchSource, StmtKind};
use rustc_span::Span;
use crate::{types, Analysis};
use types::Shape;

impl<'tcx> Analysis<'_, 'tcx> {
    fn work_report(&mut self, span: Span, shape: Shape, paid: Option<bool>, name: &str) {
        if shape == Shape::Fixed || paid == Some(true) { return; }
        if shape == Shape::Unknown || paid.is_none() {
            self.report(span, "unproven_decode_charge", &format!("{name}: iteration extent, callee, or charge coverage is unresolved; use concrete operands, a core charged operation or an explicit extent charge"));
        } else {
            self.report(span, "uncharged_decode_work", &format!("{name} performs input-sized work without admission; charge its extent immediately before it, or charge every iteration path before work"));
        }
    }

    pub(crate) fn work(&mut self, expression: &'tcx Expr<'tcx>) {
        if let ExprKind::Binary(operator, left, right) = expression.kind {
            if matches!(operator.node, BinOpKind::Eq | BinOpKind::Ne | BinOpKind::Lt | BinOpKind::Le | BinOpKind::Gt | BinOpKind::Ge) {
                let shape = types::work(self.tcx, self.typeck.expr_ty(left), &mut Vec::new()).join(types::work(self.tcx, self.typeck.expr_ty(right), &mut Vec::new()));
                // Comparing a dynamic sequence with a fixed-size operand reads
                // at most that operand's fixed extent.
                let bounded = self.constant(left, &mut Vec::new()) || self.constant(right, &mut Vec::new());
                if !bounded {
                    let paid = self.take_credit(&[left, right]);
                    self.work_report(expression.span, shape, paid, "comparison");
                }
            }
        }
        let Some((definition, operands)) = self.call(expression) else { return; };
        let name = self.tcx.item_name(definition);
        if self.context_operation(expression) { return; }
        if !types::standard(self.tcx, definition) {
            if operands.iter().any(|operand| types::work(self.tcx, self.typeck.expr_ty(operand), &mut Vec::new()) != Shape::Fixed) {
                // Calls into opaque code can hide scans even when they return a scalar.
                if !definition.is_local() {
                    let paid = self.take_credit(&operands);
                    self.work_report(expression.span, Shape::Unknown, paid, "opaque callee work");
                }
            }
            return;
        }
        let name = name.as_str();
        let consumers = matches!(name, "any" | "all" | "position" | "rposition" | "find" | "rfind" | "find_map" | "min" | "max" | "min_by" | "max_by" | "min_by_key" | "max_by_key" | "fold" | "try_fold" | "reduce" | "sum" | "product" | "count" | "for_each" | "try_for_each" | "collect" | "from_iter");
        let scans = matches!(name, "contains" | "contains_key" | "get" | "get_mut" | "starts_with" | "ends_with" | "eq" | "cmp" | "partial_cmp" | "hash" | "copy_from_slice" | "copy_within" | "extend_from_slice" | "split_at" | "trim" | "trim_start" | "trim_end" | "replace" | "replacen" | "to_lowercase" | "to_uppercase" | "is_ascii" | "from_utf8" | "from_utf8_lossy" | "to_vec" | "clone" | "to_owned" | "to_string" | "sort" | "sort_by" | "sort_by_key" | "sort_unstable" | "sort_unstable_by" | "sort_unstable_by_key" | "binary_search" | "binary_search_by" | "binary_search_by_key" | "retain" | "drain" | "clear" | "truncate" | "resize" | "append" | "extend" | "insert" | "remove" | "join" | "concat");
        if !consumers && !scans { return; }
        let Some(receiver) = operands.first() else { return; };
        let value = self.typeck.expr_ty(receiver).peel_refs();
        if matches!(name, "get" | "get_mut" | "split_at") && matches!(value.kind(), rustc_middle::ty::Slice(_) | rustc_middle::ty::Array(_, _)) { return; }
        let shape = if consumers { self.iteration(receiver, &mut Vec::new()) } else { types::work(self.tcx, value, &mut Vec::new()) };
        if shape == Shape::Fixed { return; }
        if matches!(name, "starts_with" | "ends_with" | "eq" | "cmp" | "partial_cmp") && operands.get(1).is_some_and(|operand| self.constant(operand, &mut Vec::new())) { return; }
        let paid = self.take_credit(&operands);
        self.work_report(expression.span, shape, paid, name);
    }

    fn prefix_paid(&self, expression: &'tcx Expr<'tcx>) -> Option<bool> {
        match expression.kind {
            ExprKind::Block(block, _) => self.prefix_block(block),
            ExprKind::Match(scrutinee, _, MatchSource::TryDesugar(_)) => {
                let (_, args) = self.call(scrutinee)?;
                let call = args.first()?;
                let (definition, operands) = self.call(call)?;
                if !self.context_operation(call) { return Some(false); }
                let name = self.tcx.item_name(definition);
                if matches!(name.as_str(), "charge_work" | "charge_work_limit" | "charge_collection_items" | "charge_collection_items_limit" | "charge_retained" | "charge_retained_limit" | "reserve_scoped" | "reserve_scoped_limit") {
                    let Some(amount) = operands.get(1) else { return Some(false); };
                    if let ExprKind::Lit(literal) = amount.kind {
                        return Some(matches!(literal.node, rustc_ast::LitKind::Int(value, _) if value.get() > 0));
                    }
                    return None;
                }
                if matches!(name.as_str(), "push_vec" | "insert_btree_map" | "insert_btree_set" | "copy_retained_text" | "copy_retained_text_limit" | "push_back" | "push_front" | "push_hash_group") && (self.tcx.crate_name(definition.krate).as_str() == "cadmpeg_core" || std::env::var_os("CADMPEG_POLICY_FIXTURE").is_some()) { return Some(true); }
                None
            }
            ExprKind::If(condition, yes, Some(no)) if self.fixed_value(condition) => {
                let left = self.prefix_paid(yes);
                let right = self.prefix_paid(no);
                if left == Some(true) && right == Some(true) { Some(true) } else if left.is_none() || right.is_none() { None } else { Some(false) }
            }
            _ => Some(false),
        }
    }

    fn prefix_block(&self, block: &'tcx rustc_hir::Block<'tcx>) -> Option<bool> {
        for statement in block.stmts {
            let value = match statement.kind {
                StmtKind::Expr(value) | StmtKind::Semi(value) => value,
                StmtKind::Let(local) => match local.init { Some(value) => value, None => continue },
                StmtKind::Item(_) => continue,
            };
            match self.prefix_paid(value) {
                Some(true) => return Some(true),
                None => return None,
                Some(false) => if !self.constant(value, &mut Vec::new()) && !matches!(value.kind, ExprKind::Path(_)) { return Some(false); },
            }
        }
        match block.expr { Some(value) => self.prefix_paid(value), None => Some(false) }
    }

    pub(crate) fn visit_work_expression(&mut self, expression: &'tcx Expr<'tcx>) {
        if let ExprKind::Match(source, arms, MatchSource::ForLoopDesugar) = expression.kind {
            if let [arm] = arms {
                if let ExprKind::Loop(block, _, LoopSource::ForLoop, header) = arm.body.kind {
                    if let Some((_, args)) = self.call(source) {
                        if let Some(input) = args.first() {
                            self.visit_expr(input);
                            let shape = self.iteration(input, &mut Vec::new());
                            let paid = self.take_credit(&[input]);
                            let user_body = block.stmts.first().and_then(|statement| match statement.kind {
                                StmtKind::Expr(value) | StmtKind::Semi(value) => match value.kind {
                                    ExprKind::Match(_, next_arms, _) => next_arms.iter().find_map(|arm| match arm.body.kind { ExprKind::Block(_, _) => Some(arm.body), _ => None }),
                                    _ => None,
                                },
                                _ => None,
                            });
                            let prefix = user_body.and_then(|body| self.prefix_paid(body));
                            let effective = if paid == Some(true) { paid } else { prefix };
                            self.work_report(header, shape, effective, "for loop");
                            let saved = self.flow.clone();
                            self.flow.work.clear();
                            if let Some(body) = user_body { self.visit_expr(body); }
                            self.flow = saved;
                            return;
                        }
                    }
                }
            }
        }
        match expression.kind {
            ExprKind::Loop(block, _, source, header) => {
                let paid = self.prefix_block(block);
                let shape = if source == LoopSource::ForLoop { Shape::Unknown } else { Shape::Dynamic };
                self.work_report(header, shape, paid, "loop");
                let saved = self.flow.clone();
                self.flow.work.clear();
                self.visit_block(block);
                self.flow = saved;
                return;
            }
            ExprKind::If(condition, yes, no) => {
                self.visit_expr(condition);
                let before = self.flow.clone();
                self.visit_expr(yes);
                let after_yes = self.flow.clone();
                self.flow = before;
                if let Some(no) = no { self.visit_expr(no); }
                self.flow.work.retain(|credit| after_yes.work.contains(credit));
                self.flow.storage &= after_yes.storage;
                return;
            }
            ExprKind::Match(scrutinee, arms, source) if !matches!(source, MatchSource::TryDesugar(_)) => {
                self.visit_expr(scrutinee);
                let before = self.flow.clone();
                let mut merged = before.clone();
                for arm in arms {
                    self.flow = before.clone();
                    if let Some(guard) = arm.guard { self.visit_expr(guard); }
                    self.visit_expr(arm.body);
                    merged.work.retain(|credit| self.flow.work.contains(credit));
                    merged.storage &= self.flow.storage;
                }
                self.flow = merged;
                return;
            }
            ExprKind::Assign(_, _, _) | ExprKind::AssignOp(_, _, _) => self.invalidate(),
            _ => (),
        }
        self.work(expression);
        walk_expr(self, expression);
        self.record_charge(expression);
    }
}
