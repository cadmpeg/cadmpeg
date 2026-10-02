// SPDX-License-Identifier: Apache-2.0
use crate::{external, types, Analysis};
use rustc_hir::intravisit::{walk_expr, Visitor};
use rustc_hir::{BinOpKind, Expr, ExprKind, LoopSource, MatchSource, StmtKind};
use rustc_span::Span;
use types::Shape;

impl<'tcx> Analysis<'_, 'tcx> {
    fn work_report(&mut self, span: Span, shape: Shape, paid: Option<bool>, name: &str) {
        if shape == Shape::Fixed || shape == Shape::Dynamic && paid == Some(true) {
            return;
        }
        if shape == Shape::Unknown || paid.is_none() {
            self.report(span, "unproven_decode_charge", &format!("{name}: iteration extent, callee, or charge coverage is unresolved; use concrete operands, a core charged operation or an explicit extent charge"));
        } else {
            self.report(span, "uncharged_decode_work", &format!("{name} performs input-sized work without admission; charge its extent immediately before it, or charge every iteration path before work"));
        }
    }

    pub(crate) fn work(&mut self, expression: &'tcx Expr<'tcx>) {
        if let ExprKind::Binary(operator, left, right) = expression.kind {
            if matches!(
                operator.node,
                BinOpKind::Eq
                    | BinOpKind::Ne
                    | BinOpKind::Lt
                    | BinOpKind::Le
                    | BinOpKind::Gt
                    | BinOpKind::Ge
            ) {
                let shape = types::work(self.tcx, self.expr_ty(left), &mut Vec::new())
                    .join(types::work(self.tcx, self.expr_ty(right), &mut Vec::new()));
                // Comparing a dynamic sequence with a fixed-size operand reads
                // at most that operand's fixed extent.
                if let Some(definition) = self.typeck.type_dependent_def_id(expression.hir_id) {
                    if self
                        .implementation(expression, definition)
                        .and_then(|id| external::summary(self.tcx, id, Some(self.expr_ty(left))))
                        .is_some_and(|cost| cost.work == external::Work::Fixed)
                    {
                        return;
                    }
                    if self.checked_call(expression, definition) {
                        return;
                    }
                    if let Some(custom) = self.custom_trait(expression, definition) {
                        if !self.checked_body(custom) && !types::derived(self.tcx, custom) {
                            self.work_report(
                                expression.span,
                                Shape::Unknown,
                                Some(false),
                                "custom comparison work",
                            );
                            return;
                        }
                        if !types::derived(self.tcx, custom) {
                            return;
                        }
                    }
                }
                let bounded =
                    self.constant(left, &mut Vec::new()) || self.constant(right, &mut Vec::new())
                        || self.bounded_work(left) || self.bounded_work(right);
                if !bounded {
                    let mut paid = self.take_credit(&[left, right]);
                    if paid == Some(true)
                        && (self.deep_work(self.expr_ty(left))
                            || self.deep_work(self.expr_ty(right)))
                    {
                        paid = None;
                    }
                    self.work_report(expression.span, shape, paid, "comparison");
                }
            }
        }
        let Some((definition, operands)) = self.call(expression) else {
            return;
        };
        let name = self.tcx.item_name(definition);
        if matches!(
            self.tcx.def_kind(definition),
            rustc_hir::def::DefKind::Ctor(_, _)
        ) {
            return;
        }
        if self.checked_call(expression, definition) {
            return;
        }
        let name = name.as_str();
        if let Some(custom) = self.custom_trait(expression, definition) {
            if self.checked_body(custom) {
                return;
            }
        }
        let Some(summary) = external::summary(
            self.tcx,
            self.implementation(expression, definition)
                .unwrap_or(definition),
            operands.first().map(|operand| self.expr_ty(operand)),
        ) else {
            self.report(
                expression.span,
                "unproven_decode_charge",
                &format!(
                    "external operation missing summary: {}",
                    self.tcx.def_path_str(definition)
                ),
            );
            return;
        };
        if self.tcx.trait_of_assoc(definition).is_some()
            && self.implementation(expression, definition).is_none()
        {
            self.work_report(
                expression.span,
                Shape::Unknown,
                Some(false),
                "trait implementation unresolved",
            );
            return;
        }
        if self.constant(expression, &mut Vec::new()) {
            return;
        }
        if name == "clone" {
            match self.clone_shape(self.expr_ty(expression)) {
                Shape::Fixed => return,
                Shape::Unknown => {
                    self.work_report(
                        expression.span,
                        Shape::Unknown,
                        Some(false),
                        "Clone element layout unresolved",
                    );
                    return;
                }
                Shape::Dynamic => (),
            }
        }
        if name == "format" {
            let shape = self.format_shape(&operands);
            self.work_report(expression.span, shape, Some(false), "format!");
            return;
        }
        if name == "from_elem" && operands.first().is_some_and(|operand| matches!(self.expr_ty(operand).kind(), rustc_middle::ty::Tuple(fields) if fields.is_empty())) { return; }
        if name == "resize" {
            let Some(fill) = operands.get(2) else {
                return;
            };
            let child = if self.constant(fill, &mut Vec::new()) {
                Shape::Fixed
            } else {
                self.clone_shape(self.expr_ty(fill))
            };
            let shape = if operands
                .get(1)
                .is_some_and(|count| self.constant(count, &mut Vec::new()))
            {
                child
            } else {
                child.join(Shape::Dynamic)
            };
            if child == Shape::Unknown {
                self.work_report(expression.span, child, Some(false), "resize child Clone");
            } else {
                let paid = operands
                    .get(1)
                    .map_or(Some(false), |count| self.take_credit(&[*count]));
                self.work_report(
                    expression.span,
                    shape,
                    if child == Shape::Dynamic { None } else { paid },
                    "resize",
                );
            }
            return;
        }
        if summary.work == external::Work::Repeat {
            let count = operands.get(1);
            let fixed_count = count.is_some_and(|count| self.constant(count, &mut Vec::new()));
            let shape = if count.is_some_and(|count| self.zero_extent(count)) {
                Shape::Fixed
            } else {
                let child = operands.first().map_or(Shape::Unknown, |operand| {
                    types::work(self.tcx, self.expr_ty(operand), &mut Vec::new())
                });
                if child == Shape::Unknown {
                    Shape::Unknown
                } else {
                    child.join(if fixed_count {
                        Shape::Fixed
                    } else {
                        Shape::Dynamic
                    })
                }
            };
            self.work_report(expression.span, shape, Some(false), name);
            return;
        }
        if matches!(name, "from" | "into" | "into_owned") {
            let result = self.expr_ty(expression);
            if matches!(result.kind(), rustc_middle::ty::Adt(definition, _) if types::standard(self.tcx, definition.did()) && self.tcx.item_name(definition.did()).as_str() == "Cow")
            {
                return;
            }
            if match result.kind() {
                rustc_middle::ty::RawPtr(_, _) => true,
                rustc_middle::ty::Adt(definition, _) => {
                    types::standard(self.tcx, definition.did())
                        && self.tcx.item_name(definition.did()).as_str() == "NonNull"
                }
                _ => false,
            } {
                return;
            }
            if result
                == operands
                    .first()
                    .map_or(self.expr_ty(expression), |operand| self.expr_ty(operand))
            {
                return;
            }
            if let Some(receiver) = operands.first() {
                if matches!(
                    self.expr_ty(receiver).peel_refs().kind(),
                    rustc_middle::ty::Str | rustc_middle::ty::Slice(_)
                ) {
                    let known_copy = matches!(result.kind(), rustc_middle::ty::Adt(definition, _) if types::standard(self.tcx, definition.did()) && matches!(self.tcx.item_name(definition.did()).as_str(), "String" | "Vec" | "Box" | "Rc" | "Arc" | "PathBuf" | "OsString"));
                    let paid = self.take_credit(&operands);
                    self.work_report(
                        expression.span,
                        if self.constant(receiver, &mut Vec::new()) {
                            Shape::Fixed
                        } else if known_copy {
                            Shape::Dynamic
                        } else {
                            Shape::Unknown
                        },
                        paid,
                        name,
                    );
                    return;
                }
            }
        }
        if matches!(name, "collect" | "from_iter") {
            if let Some(source) = operands.first() {
                if self.vector_collection_reuse(expression, source) == Some(Shape::Fixed) {
                    return;
                }
                if matches!(self.expr_ty(source).kind(), rustc_middle::ty::Adt(owner, _) if types::standard(self.tcx, owner.did()) && self.tcx.item_name(owner.did()).as_str() == "IntoIter" && self.tcx.def_path_str(owner.did()).contains("vec::"))
                {
                    self.work_report(
                        expression.span,
                        Shape::Unknown,
                        Some(false),
                        "owning vector iterator collection may move or copy",
                    );
                    return;
                }
            }
        }
        let consumers = summary.work == external::Work::Iterator;
        if summary.work == external::Work::Fixed {
            return;
        }
        let Some(receiver) = operands.first() else {
            return;
        };
        let value = self.expr_ty(receiver).peel_refs();
        if matches!(
            value.kind(),
            rustc_middle::ty::Bool
                | rustc_middle::ty::Char
                | rustc_middle::ty::Int(_)
                | rustc_middle::ty::Uint(_)
                | rustc_middle::ty::Float(_)
        ) {
            return;
        }
        if matches!(
            name,
            "to_vec" | "to_owned" | "copy_from_slice" | "copy_within"
        ) {
            let element = match value.kind() {
                rustc_middle::ty::Slice(element) | rustc_middle::ty::Array(element, _) => {
                    Some(*element)
                }
                rustc_middle::ty::Adt(definition, arguments)
                    if types::standard(self.tcx, definition.did())
                        && self.tcx.item_name(definition.did()).as_str() == "Vec" =>
                {
                    arguments.types().next()
                }
                _ => None,
            };
            if let Some(element) = element {
                let shape = types::slot_storage(self.tcx, element);
                if shape == Shape::Unknown
                    && !self
                        .tcx
                        .type_is_copy_modulo_regions(self.typing_env(), element)
                {
                    self.work_report(
                        expression.span,
                        shape,
                        Some(false),
                        "copy element layout unresolved",
                    );
                    return;
                }
                if shape == Shape::Fixed
                    && self
                        .tcx
                        .type_is_copy_modulo_regions(self.typing_env(), element)
                {
                    return;
                }
            }
        }
        let text = matches!(value.kind(), rustc_middle::ty::Str)
            || matches!(value.kind(), rustc_middle::ty::Adt(definition, _) if types::standard(self.tcx, definition.did()) && self.tcx.item_name(definition.did()).as_str() == "String");
        let vector = matches!(value.kind(), rustc_middle::ty::Adt(definition, _) if types::standard(self.tcx, definition.did()) && self.tcx.item_name(definition.did()).as_str() == "Vec");
        if matches!(name, "get" | "get_mut" | "split_at")
            && (vector
                || text
                || matches!(
                    value.kind(),
                    rustc_middle::ty::Slice(_) | rustc_middle::ty::Array(_, _)
                ))
        {
            return;
        }
        if (matches!(name, "clear" | "truncate")
            || name == "resize" && operands.get(1).is_some_and(|count| self.zero_extent(count)))
            && vector
        {
            if let rustc_middle::ty::Adt(_, arguments) = value.kind() {
                if arguments.types().next().is_some_and(|element| {
                    self.tcx
                        .type_is_copy_modulo_regions(self.typing_env(), element)
                }) {
                    return;
                }
            }
        }
        if name == "count" {
            if let rustc_middle::ty::Adt(definition, arguments) = value.kind() {
                let path = self.tcx.def_path_str(definition.did());
                let slice_count = types::standard(self.tcx, definition.did())
                    && path.contains("slice::")
                    && matches!(
                        self.tcx.item_name(definition.did()).as_str(),
                        "Iter" | "IterMut"
                    );
                let scalar_range = types::standard(self.tcx, definition.did())
                    && matches!(
                        self.tcx.item_name(definition.did()).as_str(),
                        "Range" | "RangeInclusive"
                    )
                    && arguments.types().all(|element| {
                        matches!(
                            element.kind(),
                            rustc_middle::ty::Int(_)
                                | rustc_middle::ty::Uint(_)
                                | rustc_middle::ty::Char
                        )
                    });
                if slice_count || scalar_range {
                    return;
                }
            }
        }
        let extent = match summary.work {
            external::Work::Argument(index) => operands.get(index).copied().unwrap_or(receiver),
            _ => receiver,
        };
        let shape = if name == "count" {
            Shape::Unknown
        } else if consumers {
            self.iteration(receiver, &mut Vec::new())
        } else {
            if self.constant(extent, &mut Vec::new()) || self.bounded_work(extent) {
                Shape::Fixed
            } else {
                types::work(self.tcx, self.expr_ty(extent), &mut Vec::new())
            }
        };
        if shape == Shape::Fixed {
            return;
        }
        if matches!(
            name,
            "starts_with" | "ends_with" | "eq" | "cmp" | "partial_cmp"
        ) && operands
            .get(1)
            .is_some_and(|operand| self.constant(operand, &mut Vec::new()) || self.bounded_work(operand))
        {
            return;
        }
        let mut paid = match summary.work {
            external::Work::Argument(_) => self.take_credit(&[extent]),
            _ => self.take_credit(&operands),
        };
        let fixed_copy = matches!(
            name,
            "extend_from_slice" | "copy_from_slice" | "copy_within" | "to_vec" | "to_owned"
        ) && match self.expr_ty(extent).peel_refs().kind() {
            rustc_middle::ty::Slice(element) | rustc_middle::ty::Array(element, _) => self
                .tcx
                .type_is_copy_modulo_regions(self.typing_env(), *element),
            _ => false,
        };
        let checked_value = if matches!(summary.work, external::Work::Argument(_)) {
            self.expr_ty(extent).peel_refs()
        } else {
            value
        };
        if paid == Some(true)
            && (matches!(
                name,
                "sort"
                    | "sort_by"
                    | "sort_by_key"
                    | "sort_unstable"
                    | "sort_unstable_by"
                    | "sort_unstable_by_key"
                    | "insert"
                    | "remove"
                    | "resize"
                    | "append"
                    | "extend"
            ) || self.deep_work(checked_value) && !fixed_copy
                || consumers && self.capacity_iteration(receiver))
        {
            paid = None;
        }
        self.work_report(expression.span, shape, paid, name);
    }

    fn capacity_iteration(&self, expression: &'tcx Expr<'tcx>) -> bool {
        if let rustc_middle::ty::Adt(definition, _) = self.expr_ty(expression).peel_refs().kind() {
            if types::standard(self.tcx, definition.did())
                && matches!(
                    self.tcx.item_name(definition.did()).as_str(),
                    "HashMap" | "HashSet"
                )
            {
                return true;
            }
        }
        self.call(expression).is_some_and(|(definition, operands)| {
            types::standard(self.tcx, definition)
                && operands
                    .first()
                    .is_some_and(|operand| self.capacity_iteration(operand))
        })
    }

    fn deep_work(&self, value: rustc_middle::ty::Ty<'tcx>) -> bool {
        match value.peel_refs().kind() {
            rustc_middle::ty::Slice(element) | rustc_middle::ty::Array(element, _) => {
                types::work(self.tcx, *element, &mut Vec::new()) != Shape::Fixed
            }
            rustc_middle::ty::Adt(definition, arguments)
                if types::standard(self.tcx, definition.did()) =>
            {
                matches!(
                    self.tcx.item_name(definition.did()).as_str(),
                    "HashMap" | "HashSet"
                ) || arguments
                    .types()
                    .any(|element| types::work(self.tcx, element, &mut Vec::new()) != Shape::Fixed)
            }
            _ => false,
        }
    }

    fn skipped_source(&self, expression: &'tcx Expr<'tcx>) -> bool {
        if let Some((definition, operands)) = self.call(expression) {
            if types::standard(self.tcx, definition) {
                if matches!(
                    self.tcx.item_name(definition).as_str(),
                    "filter"
                        | "filter_map"
                        | "skip"
                        | "skip_while"
                        | "take_while"
                        | "step_by"
                        | "flatten"
                        | "flat_map"
                ) {
                    return true;
                }
                return operands
                    .first()
                    .is_some_and(|operand| self.skipped_source(operand));
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
                if !self.context_operation(call) {
                    return Some(false);
                }
                let name = self.tcx.item_name(definition);
                if matches!(name.as_str(), "charge_work" | "charge_work_limit") {
                    if !self.trusted_context_callee(call) {
                        return None;
                    }
                    let Some(amount) = operands.get(1) else {
                        return Some(false);
                    };
                    if let ExprKind::Lit(literal) = amount.kind {
                        return Some(
                            matches!(literal.node, rustc_ast::LitKind::Int(value, _) if value.get() > 0),
                        );
                    }
                    return None;
                }
                if matches!(
                    name.as_str(),
                    "charge_collection_items"
                        | "charge_collection_items_limit"
                        | "charge_retained"
                        | "charge_retained_limit"
                        | "reserve_scoped"
                        | "reserve_scoped_limit"
                ) {
                    return Some(false);
                }
                None
            }
            ExprKind::If(condition, yes, Some(no)) if self.fixed_value(condition) => {
                let left = self.prefix_paid(yes);
                let right = self.prefix_paid(no);
                if left == Some(true) && right == Some(true) {
                    Some(true)
                } else if left.is_none() || right.is_none() {
                    None
                } else {
                    Some(false)
                }
            }
            _ => Some(false),
        }
    }

    fn prefix_block(&self, block: &'tcx rustc_hir::Block<'tcx>) -> Option<bool> {
        for statement in block.stmts {
            let value = match statement.kind {
                StmtKind::Expr(value) | StmtKind::Semi(value) => value,
                StmtKind::Let(local) => match local.init {
                    Some(value) => value,
                    None => continue,
                },
                StmtKind::Item(_) => continue,
            };
            match self.prefix_paid(value) {
                Some(true) => return Some(true),
                None => return None,
                Some(false) => {
                    if !self.constant(value, &mut Vec::new())
                        && !matches!(value.kind, ExprKind::Path(_))
                    {
                        return Some(false);
                    }
                }
            }
        }
        match block.expr {
            Some(value) => self.prefix_paid(value),
            None => Some(false),
        }
    }

    fn fixed_scalar_loop(&self, block: &'tcx rustc_hir::Block<'tcx>) -> bool {
        let Some(value) = block.expr else {
            return false;
        };
        let ExprKind::If(condition, body, _) = value.kind else {
            return false;
        };
        let condition = match condition.kind {
            ExprKind::DropTemps(inner) => inner,
            _ => condition,
        };
        let ExprKind::Binary(operator, variable, bound) = condition.kind else {
            return false;
        };
        if !matches!(self.expr_ty(variable).kind(), rustc_middle::ty::Uint(_)) {
            return false;
        }
        let ExprKind::Lit(literal) = bound.kind else {
            return false;
        };
        let rustc_ast::LitKind::Int(bound, _) = literal.node else {
            return false;
        };
        if !matches!(operator.node, BinOpKind::Gt | BinOpKind::Ge)
            && !(operator.node == BinOpKind::Ne && bound.get() == 0)
        {
            return false;
        }
        if operator.node == BinOpKind::Ge && bound.get() == 0 {
            return false;
        }
        let ExprKind::Block(body, _) = body.kind else {
            return false;
        };
        let Some(variable_key) = self.key(variable, &mut Vec::new()) else {
            return false;
        };
        let mut shrinking = false;
        for statement in body.stmts {
            let expression = match statement.kind {
                StmtKind::Semi(value) | StmtKind::Expr(value) => value,
                StmtKind::Let(local) => match local.init {
                    Some(value) => value,
                    None => continue,
                },
                StmtKind::Item(_) => continue,
            };
            if let ExprKind::AssignOp(operation, target, step) = expression.kind {
                if self.key(target, &mut Vec::new()).as_ref() == Some(&variable_key) {
                    let ExprKind::Lit(literal) = step.kind else {
                        return false;
                    };
                    let rustc_ast::LitKind::Int(step, _) = literal.node else {
                        return false;
                    };
                    if operation.node == rustc_ast::AssignOpKind::DivAssign && step.get() > 1
                        || operation.node == rustc_ast::AssignOpKind::ShrAssign && step.get() > 0
                    {
                        shrinking = true;
                        continue;
                    }
                    return false;
                }
            }
            let mut guard = ShrinkGuard {
                analysis: self,
                variable: &variable_key,
                valid: true,
            };
            guard.visit_expr(expression);
            if !guard.valid {
                return false;
            }
        }
        if let Some(tail) = body.expr {
            let mut guard = ShrinkGuard {
                analysis: self,
                variable: &variable_key,
                valid: true,
            };
            guard.visit_expr(tail);
            if !guard.valid {
                return false;
            }
        }
        shrinking
    }

    fn restore_loop(&mut self, mut saved: crate::flow::Flow) {
        saved.mutated.extend(self.flow.mutated.iter().cloned());
        saved.work.retain(|credit| {
            !credit
                .extents
                .iter()
                .flat_map(|term| &term.factors)
                .any(|term| {
                    saved
                        .mutated
                        .iter()
                        .any(|key| term == key || term.starts_with(&format!("{key}.")))
                })
        });
        saved.storage |= self.flow.storage;
        saved
            .storage_extents
            .retain(|term| self.flow.storage_extents.contains(term));
        saved
            .storage_slots
            .retain(|credit| self.flow.storage_slots.contains(credit));
        self.flow = saved;
    }

    pub(crate) fn visit_work_expression(&mut self, expression: &'tcx Expr<'tcx>) {
        if let ExprKind::Match(source, [arm], MatchSource::ForLoopDesugar) = expression.kind {
            if let ExprKind::Loop(block, _, LoopSource::ForLoop, header) = arm.body.kind {
                if let Some((_, args)) = self.call(source) {
                    if let Some(input) = args.first() {
                        self.visit_expr(input);
                        let shape = self.iteration(input, &mut Vec::new());
                        let paid = self.take_credit(&[input]);
                        let user_body =
                            block
                                .stmts
                                .first()
                                .and_then(|statement| match statement.kind {
                                    StmtKind::Expr(value) | StmtKind::Semi(value) => {
                                        match value.kind {
                                            ExprKind::Match(_, next_arms, _) => next_arms
                                                .iter()
                                                .find_map(|arm| match arm.body.kind {
                                                    ExprKind::Block(_, _) => Some(arm.body),
                                                    _ => None,
                                                }),
                                            _ => None,
                                        }
                                    }
                                    _ => None,
                                });
                        let prefix = user_body.and_then(|body| self.prefix_paid(body));
                        let effective = if paid == Some(true) {
                            if self.capacity_iteration(input) {
                                None
                            } else {
                                paid
                            }
                        } else if paid.is_none() {
                            if prefix == Some(true) && !self.skipped_source(input) {
                                prefix
                            } else {
                                None
                            }
                        } else if self.skipped_source(input) {
                            Some(false)
                        } else {
                            prefix
                        };
                        self.work_report(header, shape, effective, "for loop");
                        let mut saved = self.flow.clone();
                        let repetitions = self.constant_count(input, &mut Vec::new());
                        let iterations = if shape == Shape::Fixed {
                            repetitions
                                .filter(|count| *count > 0)
                                .and_then(|count| self.flow.iterations.checked_mul(count))
                        } else {
                            None
                        };
                        if let Some(iterations) = iterations {
                            self.flow.iterations = iterations;
                        } else {
                            self.flow.work.clear();
                        }
                        if let Some(bounds) = self.extent_terms(input, &mut Vec::new()) {
                            self.flow.loop_bounds.push(bounds);
                        }
                        if let Some(body) = user_body {
                            self.visit_expr(body);
                        }
                        if iterations.is_some() {
                            saved.work = self.flow.work.clone();
                        }
                        self.restore_loop(saved);
                        return;
                    }
                }
            }
        }
        match expression.kind {
            ExprKind::Closure(_) => return,
            ExprKind::Loop(block, _, source, header) => {
                let paid = if source == LoopSource::While {
                    match block.expr {
                        Some(value) => match value.kind {
                            ExprKind::If(_, body, _) => self.prefix_paid(body),
                            _ => self.prefix_block(block),
                        },
                        None => self.prefix_block(block),
                    }
                } else {
                    self.prefix_block(block)
                };
                let shape = if source == LoopSource::While && self.fixed_scalar_loop(block) {
                    Shape::Fixed
                } else if source == LoopSource::ForLoop {
                    Shape::Unknown
                } else {
                    Shape::Dynamic
                };
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
                if let Some(no) = no {
                    self.visit_expr(no);
                }
                self.flow
                    .work
                    .retain(|credit| after_yes.work.contains(credit));
                self.flow.storage |= after_yes.storage;
                self.flow
                    .storage_extents
                    .retain(|term| after_yes.storage_extents.contains(term));
                self.flow
                    .storage_slots
                    .retain(|credit| after_yes.storage_slots.contains(credit));
                self.flow.mutated.extend(after_yes.mutated);
                return;
            }
            ExprKind::Match(scrutinee, arms, source)
                if !matches!(source, MatchSource::TryDesugar(_)) =>
            {
                self.visit_expr(scrutinee);
                let before = self.flow.clone();
                let mut merged = before.clone();
                for arm in arms {
                    self.flow = before.clone();
                    if let Some(guard) = arm.guard {
                        self.visit_expr(guard);
                    }
                    self.visit_expr(arm.body);
                    merged.work.retain(|credit| self.flow.work.contains(credit));
                    merged.storage |= self.flow.storage;
                    merged
                        .storage_extents
                        .retain(|term| self.flow.storage_extents.contains(term));
                    merged
                        .storage_slots
                        .retain(|credit| self.flow.storage_slots.contains(credit));
                    merged.mutated.extend(self.flow.mutated.iter().cloned());
                }
                self.flow = merged;
                return;
            }
            _ => (),
        }
        walk_expr(self, expression);
        self.work(expression);
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
            ExprKind::Assign(target, _, _)
            | ExprKind::AssignOp(_, target, _)
            | ExprKind::AddrOf(_, rustc_hir::Mutability::Mut, target) => {
                if self.analysis.key(target, &mut Vec::new()).as_deref() == Some(self.variable) {
                    self.valid = false;
                }
            }
            ExprKind::Continue(_) | ExprKind::Loop(_, _, _, _) => self.valid = false,
            _ => (),
        }
        if let Some((_, operands)) = self.analysis.call(expression) {
            if operands.iter().any(|operand| {
                matches!(
                    self.analysis.expr_ty_adjusted(operand).kind(),
                    rustc_middle::ty::Ref(_, _, rustc_hir::Mutability::Mut)
                ) && self.analysis.key(operand, &mut Vec::new()).as_deref() == Some(self.variable)
            }) {
                self.valid = false;
            }
        }
        walk_expr(self, expression);
    }
}
