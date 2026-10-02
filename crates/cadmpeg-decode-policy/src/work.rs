// SPDX-License-Identifier: Apache-2.0
use rustc_hir::intravisit::{walk_expr, Visitor};
use rustc_hir::{BinOpKind, Expr, ExprKind, LoopSource, MatchSource, StmtKind};
use rustc_span::Span;
use crate::{types, Analysis};
use types::Shape;

impl<'tcx> Analysis<'_, 'tcx> {
    fn work_report(&mut self, span: Span, shape: Shape, paid: Option<bool>, name: &str) {
        if shape == Shape::Fixed || shape == Shape::Dynamic && paid == Some(true) { return; }
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
                if let Some(definition) = self.typeck.type_dependent_def_id(expression.hir_id) {
                    if let Some(custom) = self.custom_trait(expression, definition) {
                        if self.local_has_effects(custom) != Some(false) { self.work_report(expression.span, Shape::Unknown, Some(false), "custom comparison work"); }
                        return;
                    }
                }
                let bounded = self.constant(left, &mut Vec::new()) || self.constant(right, &mut Vec::new());
                if !bounded {
                    let mut paid = self.take_credit(&[left, right]);
                    if paid == Some(true) && (self.deep_work(self.typeck.expr_ty(left)) || self.deep_work(self.typeck.expr_ty(right))) { paid = None; }
                    self.work_report(expression.span, shape, paid, "comparison");
                }
            }
        }
        let Some((definition, operands)) = self.call(expression) else { return; };
        let name = self.tcx.item_name(definition);
        if self.context_operation(expression) { return; }
        if !types::standard(self.tcx, definition) {
            // An unavailable body can scale work with a scalar count too.
            let known_index_conversion = self.tcx.crate_name(definition.krate).as_str() == "cadmpeg_core" && name.as_str() == "u64_from_index";
            if !known_index_conversion && self.local_has_effects(definition) != Some(false) {
                let paid = self.take_credit(&operands);
                self.work_report(expression.span, Shape::Unknown, paid, &format!("opaque callee work {}", self.tcx.def_path_str(definition)));
            }
            return;
        }
        let name = name.as_str();
        if matches!(name, "clone" | "eq" | "cmp" | "partial_cmp" | "hash" | "from" | "into" | "into_owned" | "to_owned") {
            if let Some(custom) = self.custom_trait(expression, definition) {
                if self.local_has_effects(custom) != Some(false) {
                    self.work_report(expression.span, Shape::Unknown, Some(false), "custom trait work");
                }
                return;
            }
            if name == "clone" && types::heap(self.tcx, self.typeck.expr_ty(expression), &mut Vec::new()) == Shape::Fixed { return; }
        }
        if name == "format" {
            let shape = self.format_shape(&operands);
            self.work_report(expression.span, shape, Some(false), "format!");
            return;
        }
        if matches!(name, "from_elem" | "repeat") {
            let count = operands.get(1);
            let shape = if count.is_some_and(|count| self.constant(count, &mut Vec::new())) { Shape::Fixed } else { Shape::Dynamic };
            self.work_report(expression.span, shape, Some(false), name);
            return;
        }
        if matches!(name, "from" | "into" | "into_owned") {
            let result = self.typeck.expr_ty(expression);
            if match result.kind() { rustc_middle::ty::RawPtr(_, _) => true, rustc_middle::ty::Adt(definition, _) => types::standard(self.tcx, definition.did()) && self.tcx.item_name(definition.did()).as_str() == "NonNull", _ => false } { return; }
            if result == operands.first().map_or(self.typeck.expr_ty(expression), |operand| self.typeck.expr_ty(operand)) { return; }
            if let Some(receiver) = operands.first() {
                if matches!(self.typeck.expr_ty(receiver).peel_refs().kind(), rustc_middle::ty::Str | rustc_middle::ty::Slice(_)) {
                    let known_copy = matches!(result.kind(), rustc_middle::ty::Adt(definition, _) if types::standard(self.tcx, definition.did()) && matches!(self.tcx.item_name(definition.did()).as_str(), "String" | "Vec" | "Box" | "Rc" | "Arc" | "PathBuf" | "OsString"));
                    let paid = self.take_credit(&operands);
                    self.work_report(expression.span, if self.constant(receiver, &mut Vec::new()) { Shape::Fixed } else if known_copy { Shape::Dynamic } else { Shape::Unknown }, paid, name);
                    return;
                }
            }
        }
        let consumers = matches!(name, "any" | "all" | "position" | "rposition" | "find" | "rfind" | "find_map" | "min" | "max" | "min_by" | "max_by" | "min_by_key" | "max_by_key" | "fold" | "try_fold" | "reduce" | "sum" | "product" | "count" | "for_each" | "try_for_each" | "collect" | "from_iter");
        let scans = matches!(name, "contains" | "contains_key" | "get" | "get_mut" | "starts_with" | "ends_with" | "eq" | "cmp" | "partial_cmp" | "hash" | "copy_from_slice" | "copy_within" | "extend_from_slice" | "split_at" | "trim" | "trim_start" | "trim_end" | "replace" | "replacen" | "to_lowercase" | "to_uppercase" | "is_ascii" | "from_utf8" | "from_utf8_lossy" | "to_vec" | "clone" | "to_owned" | "to_string" | "sort" | "sort_by" | "sort_by_key" | "sort_unstable" | "sort_unstable_by" | "sort_unstable_by_key" | "binary_search" | "binary_search_by" | "binary_search_by_key" | "retain" | "drain" | "clear" | "truncate" | "resize" | "append" | "extend" | "insert" | "remove" | "join" | "concat");
        if !consumers && !scans {
            let path = self.tcx.def_path_str(definition);
            let fixed_call = matches!(name, "len" | "capacity" | "is_empty" | "as_str" | "as_bytes" | "as_slice" | "as_mut_slice" | "as_ref" | "as_mut" | "borrow" | "borrow_mut" | "deref" | "deref_mut" | "new" | "new_uninit" | "default" | "with_capacity" | "with_capacity_in" | "iter" | "iter_mut" | "into_iter" | "map" | "filter" | "filter_map" | "skip" | "take" | "enumerate" | "rev" | "zip" | "chain" | "peekable" | "fuse" | "copied" | "cloned" | "inspect" | "flat_map" | "flatten" | "step_by" | "skip_while" | "take_while" | "branch" | "from_output" | "from_residual" | "ok_or" | "ok_or_else" | "map_err" | "must_use" | "write_box_via_move" | "box_assume_init_into_vec_unsafe" | "size_of" | "align_of" | "push" | "push_back" | "push_front" | "reserve" | "reserve_exact" | "try_reserve" | "try_reserve_exact" | "unwrap_or" | "unwrap_or_else" | "is_some" | "is_none" | "is_ok" | "is_err") || matches!(self.tcx.def_kind(definition), rustc_hir::def::DefKind::Ctor(_, _)) || path.contains("fmt::") || path.contains("fmt::rt");
            if !fixed_call && operands.iter().any(|operand| types::work(self.tcx, self.typeck.expr_ty(operand), &mut Vec::new()) != Shape::Fixed) {
                self.work_report(expression.span, Shape::Unknown, Some(false), "standard callee work not summarized");
            }
            return;
        }
        let Some(receiver) = operands.first() else { return; };
        let value = self.typeck.expr_ty(receiver).peel_refs();
        if matches!(value.kind(), rustc_middle::ty::Bool | rustc_middle::ty::Char | rustc_middle::ty::Int(_) | rustc_middle::ty::Uint(_) | rustc_middle::ty::Float(_)) { return; }
        let vector = matches!(value.kind(), rustc_middle::ty::Adt(definition, _) if types::standard(self.tcx, definition.did()) && self.tcx.item_name(definition.did()).as_str() == "Vec");
        if matches!(name, "get" | "get_mut" | "split_at") && (vector || matches!(value.kind(), rustc_middle::ty::Slice(_) | rustc_middle::ty::Array(_, _))) { return; }
        if matches!(name, "clear" | "truncate") && vector {
            if let rustc_middle::ty::Adt(_, arguments) = value.kind() {
                if arguments.types().next().is_some_and(|element| self.tcx.type_is_copy_modulo_regions(rustc_middle::ty::TypingEnv::post_analysis(self.tcx, self.owner), element)) { return; }
            }
        }
        let shape = if consumers { self.iteration(receiver, &mut Vec::new()) } else { types::work(self.tcx, value, &mut Vec::new()) };
        if shape == Shape::Fixed { return; }
        if matches!(name, "starts_with" | "ends_with" | "eq" | "cmp" | "partial_cmp") && operands.get(1).is_some_and(|operand| self.constant(operand, &mut Vec::new())) { return; }
        let mut paid = self.take_credit(&operands);
        if paid == Some(true) && (matches!(name, "sort" | "sort_by" | "sort_by_key" | "sort_unstable" | "sort_unstable_by" | "sort_unstable_by_key" | "insert" | "remove" | "resize" | "append" | "extend") || self.deep_work(value) || self.capacity_iteration(receiver)) { paid = None; }
        self.work_report(expression.span, shape, paid, name);
    }

    fn capacity_iteration(&self, expression: &'tcx Expr<'tcx>) -> bool {
        if let rustc_middle::ty::Adt(definition, _) = self.typeck.expr_ty(expression).peel_refs().kind() {
            if types::standard(self.tcx, definition.did()) && matches!(self.tcx.item_name(definition.did()).as_str(), "HashMap" | "HashSet") { return true; }
        }
        self.call(expression).is_some_and(|(definition, operands)| types::standard(self.tcx, definition) && operands.first().is_some_and(|operand| self.capacity_iteration(operand)))
    }

    fn deep_work(&self, value: rustc_middle::ty::Ty<'tcx>) -> bool {
        match value.peel_refs().kind() {
            rustc_middle::ty::Slice(element) | rustc_middle::ty::Array(element, _) => types::work(self.tcx, *element, &mut Vec::new()) != Shape::Fixed,
            rustc_middle::ty::Adt(definition, arguments) if types::standard(self.tcx, definition.did()) => {
                matches!(self.tcx.item_name(definition.did()).as_str(), "HashMap" | "HashSet") || arguments.types().any(|element| types::work(self.tcx, element, &mut Vec::new()) != Shape::Fixed)
            }
            _ => false,
        }
    }

    fn skipped_source(&self, expression: &'tcx Expr<'tcx>) -> bool {
        if let Some((definition, operands)) = self.call(expression) {
            if types::standard(self.tcx, definition) {
                if matches!(self.tcx.item_name(definition).as_str(), "filter" | "filter_map" | "skip" | "skip_while" | "take_while" | "step_by" | "flatten" | "flat_map") { return true; }
                return operands.first().is_some_and(|operand| self.skipped_source(operand));
            }
        }
        false
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
                if matches!(name.as_str(), "charge_work" | "charge_work_limit") {
                    let Some(amount) = operands.get(1) else { return Some(false); };
                    if let ExprKind::Lit(literal) = amount.kind {
                        return Some(matches!(literal.node, rustc_ast::LitKind::Int(value, _) if value.get() > 0));
                    }
                    return None;
                }
                if matches!(name.as_str(), "charge_collection_items" | "charge_collection_items_limit" | "charge_retained" | "charge_retained_limit" | "reserve_scoped" | "reserve_scoped_limit") { return Some(false); }
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

    fn fixed_scalar_loop(&self, block: &'tcx rustc_hir::Block<'tcx>) -> bool {
        let Some(value) = block.expr else { return false; };
        let ExprKind::If(condition, body, _) = value.kind else { return false; };
        let condition = match condition.kind { ExprKind::DropTemps(inner) => inner, _ => condition };
        let ExprKind::Binary(operator, variable, bound) = condition.kind else { return false; };
        if !matches!(self.typeck.expr_ty(variable).kind(), rustc_middle::ty::Uint(_)) { return false; }
        let ExprKind::Lit(literal) = bound.kind else { return false; };
        let rustc_ast::LitKind::Int(bound, _) = literal.node else { return false; };
        if !matches!(operator.node, BinOpKind::Gt | BinOpKind::Ge) && !(operator.node == BinOpKind::Ne && bound.get() == 0) { return false; }
        if operator.node == BinOpKind::Ge && bound.get() == 0 { return false; }
        let ExprKind::Block(body, _) = body.kind else { return false; };
        let Some(variable_key) = self.key(variable, &mut Vec::new()) else { return false; };
        let mut shrinking = false;
        for statement in body.stmts {
            let expression = match statement.kind { StmtKind::Semi(value) | StmtKind::Expr(value) => value, StmtKind::Let(local) => match local.init { Some(value) => value, None => continue }, StmtKind::Item(_) => continue };
            if let ExprKind::AssignOp(operation, target, step) = expression.kind {
                if self.key(target, &mut Vec::new()).as_ref() == Some(&variable_key) {
                    let ExprKind::Lit(literal) = step.kind else { return false; };
                    let rustc_ast::LitKind::Int(step, _) = literal.node else { return false; };
                    if operation.node == rustc_ast::AssignOpKind::DivAssign && step.get() > 1 || operation.node == rustc_ast::AssignOpKind::ShrAssign && step.get() > 0 { shrinking = true; continue; }
                    return false;
                }
            }
            let mut guard = ShrinkGuard { analysis: self, variable: &variable_key, valid: true };
            guard.visit_expr(expression);
            if !guard.valid { return false; }
        }
        if let Some(tail) = body.expr {
            let mut guard = ShrinkGuard { analysis: self, variable: &variable_key, valid: true };
            guard.visit_expr(tail);
            if !guard.valid { return false; }
        }
        shrinking
    }

    fn restore_loop(&mut self, mut saved: crate::flow::Flow) {
        saved.mutated.extend(self.flow.mutated.iter().cloned());
        saved.work.retain(|credit| !credit.extents.iter().any(|term| saved.mutated.iter().any(|key| term == key || term.starts_with(&format!("{key}.")))));
        saved.storage |= self.flow.storage;
        self.flow = saved;
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
                            let effective = if paid == Some(true) {
                                if self.capacity_iteration(input) { None } else { paid }
                            } else if paid.is_none() {
                                if prefix == Some(true) && !self.skipped_source(input) { prefix } else { None }
                            } else if self.skipped_source(input) { Some(false) } else { prefix };
                            self.work_report(header, shape, effective, "for loop");
                            let saved = self.flow.clone();
                            self.flow.work.clear();
                            if let Some(body) = user_body { self.visit_expr(body); }
                            self.restore_loop(saved);
                            return;
                        }
                    }
                }
            }
        }
        match expression.kind {
            ExprKind::Closure(closure) => {
                let owner = closure.def_id;
                let mut stack = self.stack.clone();
                stack.push(owner);
                Analysis { tcx: self.tcx, typeck: self.tcx.typeck(owner), owner, summaries: self.summaries, flow: crate::flow::Flow::default(), stack, findings: self.findings }.visit_body(self.tcx.hir_body(closure.body));
                return;
            }
            ExprKind::Loop(block, _, source, header) => {
                let paid = if source == LoopSource::While {
                    match block.expr {
                        Some(value) => match value.kind {
                            ExprKind::If(_, body, _) => self.prefix_paid(body),
                            _ => self.prefix_block(block),
                        },
                        None => self.prefix_block(block),
                    }
                } else { self.prefix_block(block) };
                let shape = if source == LoopSource::While && self.fixed_scalar_loop(block) { Shape::Fixed } else if source == LoopSource::ForLoop { Shape::Unknown } else { Shape::Dynamic };
                self.work_report(header, shape, paid, "loop");
                let saved = self.flow.clone();
                self.flow.work.clear();
                self.visit_block(block);
                self.restore_loop(saved);
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
                self.flow.storage |= after_yes.storage;
                self.flow.mutated.extend(after_yes.mutated);
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
                    merged.storage |= self.flow.storage;
                    merged.mutated.extend(self.flow.mutated.iter().cloned());
                }
                self.flow = merged;
                return;
            }
            _ => (),
        }
        self.work(expression);
        walk_expr(self, expression);
        self.mutation(expression);
        self.record_charge(expression);
    }
}

struct ShrinkGuard<'a, 'b, 'tcx> {
    analysis: &'a Analysis<'b, 'tcx>,
    variable: &'a str,
    valid: bool,
}

impl<'tcx> Visitor<'tcx> for ShrinkGuard<'_, '_, 'tcx> {
    fn visit_expr(&mut self, expression: &'tcx Expr<'tcx>) {
        match expression.kind {
            ExprKind::Assign(target, _, _) | ExprKind::AssignOp(_, target, _) | ExprKind::AddrOf(_, rustc_hir::Mutability::Mut, target) => {
                if self.analysis.key(target, &mut Vec::new()).as_deref() == Some(self.variable) { self.valid = false; }
            }
            ExprKind::Continue(_) | ExprKind::Loop(_, _, _, _) => self.valid = false,
            _ => (),
        }
        if let Some((_, operands)) = self.analysis.call(expression) {
            if operands.iter().any(|operand| matches!(self.analysis.typeck.expr_ty_adjusted(operand).kind(), rustc_middle::ty::Ref(_, _, rustc_hir::Mutability::Mut)) && self.analysis.key(operand, &mut Vec::new()).as_deref() == Some(self.variable)) { self.valid = false; }
        }
        walk_expr(self, expression);
    }
}
