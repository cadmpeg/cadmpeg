// SPDX-License-Identifier: Apache-2.0
use crate::{types, Analysis};
use rustc_hir::intravisit::{walk_expr, Visitor};
use rustc_hir::{
    def::Res, BinOpKind, Block, Expr, ExprKind, HirId, MatchSource, Node, PatKind, Stmt, StmtKind,
};
use rustc_middle::ty;

const MAX_INDEPENDENT_SLICE_VISITS: u64 = 256;

struct LocalUses<'borrow, 'analysis, 'tcx> {
    analysis: &'borrow Analysis<'analysis, 'tcx>,
    target: HirId,
    count: usize,
    overflowed: bool,
}

impl<'tcx> Visitor<'tcx> for LocalUses<'_, '_, 'tcx> {
    fn visit_expr(&mut self, expression: &'tcx Expr<'tcx>) {
        if let ExprKind::Path(ref path) = expression.kind {
            if matches!(
                self.analysis
                    .typeck
                    .qpath_res(path, expression.hir_id),
                Res::Local(id) if id == self.target
            ) {
                match self.count.checked_add(1) {
                    Some(count) => self.count = count,
                    None => self.overflowed = true,
                }
            }
        }
        walk_expr(self, expression);
    }
}

struct SliceBound {
    guard_statement: usize,
    vector_statement: usize,
}

impl<'tcx> Analysis<'_, 'tcx> {
    /// Returns the slice read by a direct standard `iter().copied()` extension.
    pub(crate) fn copied_slice_source(
        &self,
        expression: &'tcx Expr<'tcx>,
    ) -> Option<&'tcx Expr<'tcx>> {
        let (definition, operands) = self.call(expression)?;
        if !types::standard(self.tcx, definition)
            || self.tcx.item_name(definition).as_str() != "extend"
        {
            return None;
        }
        let receiver = operands.first()?;
        if !types::standard_vector(self.tcx, self.expr_ty(receiver)) {
            return None;
        }
        let ty::Adt(_, vector_arguments) = self.expr_ty(receiver).peel_refs().kind() else {
            return None;
        };
        let element = vector_arguments.types().next()?;
        if !self
            .tcx
            .type_is_copy_modulo_regions(self.typing_env(), element)
        {
            return None;
        }

        let copied_expression = Self::without_drop_temps(*operands.get(1)?);
        let (copied_definition, copied_operands) = self.call(copied_expression)?;
        if !types::standard(self.tcx, copied_definition)
            || self.tcx.item_name(copied_definition).as_str() != "copied"
        {
            return None;
        }
        let slice_iterator = Self::without_drop_temps(*copied_operands.first()?);
        let (iter_definition, iter_operands) = self.call(slice_iterator)?;
        if !types::standard(self.tcx, iter_definition)
            || self.tcx.item_name(iter_definition).as_str() != "iter"
        {
            return None;
        }
        let source = *iter_operands.first()?;
        let source_id = self.local_binding(source)?;
        let ty::Ref(_, slice, rustc_hir::Mutability::Not) = self.expr_ty(source).kind() else {
            return None;
        };
        let ty::Slice(slice_element) = slice.kind() else {
            return None;
        };
        if *slice_element != element
            || !self.immutable_binding(source_id)
            || self.mutated_local(source_id)
        {
            return None;
        }
        Some(source)
    }

    /// Certifies only the reserve and copy steps of an independently bounded slice copy.
    pub(crate) fn bounded_slice_copy(&self, expression: &'tcx Expr<'tcx>) -> bool {
        let Some((definition, operands)) = self.call(expression) else {
            return false;
        };
        if !types::standard(self.tcx, definition) {
            return false;
        }
        match self.tcx.item_name(definition).as_str() {
            "try_reserve_exact" => self.bounded_slice_reserve(expression, &operands),
            "extend" => self.bounded_slice_extend(expression, &operands),
            _ => false,
        }
    }

    fn bounded_slice_reserve(
        &self,
        expression: &'tcx Expr<'tcx>,
        operands: &[&'tcx Expr<'tcx>],
    ) -> bool {
        let Some(receiver) = operands.first() else {
            return false;
        };
        let Some(capacity) = operands.get(1).copied() else {
            return false;
        };
        if !self.propagated(expression) || !types::standard_vector(self.tcx, self.expr_ty(receiver))
        {
            return false;
        }
        let Some(target) = self.local_binding(receiver) else {
            return false;
        };
        let Some((block, reserve_statement)) = self.operation_block(expression) else {
            return false;
        };
        let Some(bound) = self.slice_bound(block, reserve_statement, None, capacity, receiver)
        else {
            return false;
        };
        self.only_fresh_vector_use(block, bound.vector_statement, reserve_statement, target)
            && self.local_uses_in_statement(&block.stmts[reserve_statement], target) == Some(1)
    }

    fn bounded_slice_extend(
        &self,
        expression: &'tcx Expr<'tcx>,
        operands: &[&'tcx Expr<'tcx>],
    ) -> bool {
        let Some(receiver) = operands.first() else {
            return false;
        };
        let Some(source) = self.copied_slice_source(expression) else {
            return false;
        };
        let Some(target) = self.local_binding(receiver) else {
            return false;
        };
        let Some((block, extend_statement)) = self.operation_block(expression) else {
            return false;
        };
        let Some((vector_statement, _)) = self.local_declaration(block, target) else {
            return false;
        };

        let mut reserve = None;
        for (index, statement) in block
            .stmts
            .iter()
            .enumerate()
            .take(extend_statement)
            .skip(vector_statement + 1)
        {
            let Some(uses) = self.local_uses_in_statement(statement, target) else {
                return false;
            };
            if uses == 0 {
                continue;
            }
            let Some((capacity, reserve_call)) = self.reserve_call_in_statement(statement, target)
            else {
                return false;
            };
            if uses != 1 || reserve.is_some() {
                return false;
            }
            reserve = Some((index, capacity, reserve_call));
        }
        let Some((reserve_statement, capacity, reserve_call)) = reserve else {
            return false;
        };
        let Some(bound) =
            self.slice_bound(block, extend_statement, Some(source), capacity, receiver)
        else {
            return false;
        };
        reserve_statement > bound.guard_statement
            && self.propagated(reserve_call)
            && self.local_uses_in_statement(&block.stmts[extend_statement], target) == Some(1)
    }

    fn reserve_call_in_statement(
        &self,
        statement: &'tcx Stmt<'tcx>,
        target: HirId,
    ) -> Option<(&'tcx Expr<'tcx>, &'tcx Expr<'tcx>)> {
        let mut reserve = self.propagated_result(Self::statement_expression(statement)?)?;
        while let Some((definition, operands)) = self.call(reserve) {
            if !types::standard(self.tcx, definition)
                || self.tcx.item_name(definition).as_str() != "map_err"
            {
                break;
            }
            reserve = Self::without_drop_temps(*operands.first()?);
        }
        let (definition, operands) = self.call(reserve)?;
        if !types::standard(self.tcx, definition)
            || self.tcx.item_name(definition).as_str() != "try_reserve_exact"
            || self.local_binding(*operands.first()?) != Some(target)
        {
            return None;
        }
        Some((*operands.get(1)?, reserve))
    }

    fn slice_bound(
        &self,
        block: &'tcx Block<'tcx>,
        operation_statement: usize,
        expected_source: Option<&'tcx Expr<'tcx>>,
        capacity_expression: &'tcx Expr<'tcx>,
        receiver: &'tcx Expr<'tcx>,
    ) -> Option<SliceBound> {
        if operation_statement >= block.stmts.len()
            || self.expr_ty(capacity_expression) != self.tcx.types.usize
            || !types::standard_vector(self.tcx, self.expr_ty(receiver))
        {
            return None;
        }
        let capacity = self.local_binding(capacity_expression)?;
        if !self.immutable_binding(capacity) || self.mutated_local(capacity) {
            return None;
        }
        let (capacity_statement, capacity_initializer) = self.local_declaration(block, capacity)?;
        let length = self.checked_capacity_source(capacity_initializer)?;
        let (length_statement, length_initializer) = self.local_declaration(block, length)?;
        if length_statement >= capacity_statement
            || self.expr_ty(length_initializer) != self.tcx.types.usize
            || !self.immutable_binding(length)
            || self.mutated_local(length)
        {
            return None;
        }
        let source = self.slice_length_source(length_initializer)?;
        if self.mutated_local(source)
            || expected_source
                .is_some_and(|expression| self.local_binding(expression) != Some(source))
        {
            return None;
        }
        let guard_statement = block
            .stmts
            .iter()
            .take(operation_statement)
            .enumerate()
            .find_map(|(index, statement)| {
                let expression = Self::statement_expression(statement)?;
                (self.slice_length_guard(expression) == Some(length)).then_some(index)
            })?;
        if length_statement >= guard_statement
            || capacity_statement >= guard_statement
            || guard_statement >= operation_statement
        {
            return None;
        }
        let target = self.local_binding(receiver)?;
        let (vector_statement, initializer) = self.local_declaration(block, target)?;
        if vector_statement <= guard_statement
            || vector_statement >= operation_statement
            || !self.fresh_vector_initializer(initializer, receiver)
            || !self.vector_is_copy(receiver)
        {
            return None;
        }
        Some(SliceBound {
            guard_statement,
            vector_statement,
        })
    }

    fn operation_block(&self, expression: &'tcx Expr<'tcx>) -> Option<(&'tcx Block<'tcx>, usize)> {
        let mut closure_ancestor = false;
        let mut nearest_block = None;
        for (_, node) in self.tcx.hir_parent_iter(expression.hir_id) {
            match node {
                Node::Expr(parent) if matches!(parent.kind, ExprKind::Closure(_)) => {
                    closure_ancestor = true;
                }
                Node::Block(block) if nearest_block.is_none() => {
                    nearest_block = Some(
                        block
                            .stmts
                            .iter()
                            .position(|statement| statement.span.contains(expression.span))
                            .map(|index| (block, index)),
                    );
                }
                Node::Item(_) => break,
                _ => (),
            }
        }
        if closure_ancestor {
            None
        } else {
            nearest_block.flatten()
        }
    }

    fn local_declaration(
        &self,
        block: &'tcx Block<'tcx>,
        target: HirId,
    ) -> Option<(usize, &'tcx Expr<'tcx>)> {
        block
            .stmts
            .iter()
            .enumerate()
            .find_map(|(index, statement)| {
                let StmtKind::Let(local) = statement.kind else {
                    return None;
                };
                if matches!(local.pat.kind, PatKind::Binding(_, id, _, None) if id == target) {
                    local.init.map(|initializer| (index, initializer))
                } else {
                    None
                }
            })
    }

    fn statement_expression(statement: &'tcx Stmt<'tcx>) -> Option<&'tcx Expr<'tcx>> {
        match statement.kind {
            StmtKind::Let(local) => local.init,
            StmtKind::Expr(expression) | StmtKind::Semi(expression) => Some(expression),
            StmtKind::Item(_) => None,
        }
    }

    fn local_binding(&self, expression: &'tcx Expr<'tcx>) -> Option<HirId> {
        let expression = Self::without_drop_temps(expression);
        match expression.kind {
            ExprKind::Path(ref path) => match self.typeck.qpath_res(path, expression.hir_id) {
                Res::Local(id) => Some(id),
                _ => None,
            },
            _ => None,
        }
    }

    fn without_drop_temps(mut expression: &'tcx Expr<'tcx>) -> &'tcx Expr<'tcx> {
        while let ExprKind::DropTemps(inner) = expression.kind {
            expression = inner;
        }
        expression
    }

    fn local_uses_in_statement(&self, statement: &'tcx Stmt<'tcx>, target: HirId) -> Option<usize> {
        let expression = Self::statement_expression(statement)?;
        let mut uses = LocalUses {
            analysis: self,
            target,
            count: 0,
            overflowed: false,
        };
        uses.visit_expr(expression);
        (!uses.overflowed).then_some(uses.count)
    }

    fn mutated_local(&self, id: HirId) -> bool {
        self.flow.mutated.contains(&format!("local:{id:?}"))
    }

    fn immutable_binding(&self, id: HirId) -> bool {
        matches!(self.tcx.hir_node(id), Node::Pat(pattern)
            if matches!(pattern.kind, PatKind::Binding(mode, _, _, None)
                if mode == rustc_hir::BindingMode::NONE))
    }

    fn propagated_result(&self, expression: &'tcx Expr<'tcx>) -> Option<&'tcx Expr<'tcx>> {
        let ExprKind::Match(scrutinee, _, MatchSource::TryDesugar(_)) =
            Self::without_drop_temps(expression).kind
        else {
            return None;
        };
        let (definition, operands) = self.call(Self::without_drop_temps(scrutinee))?;
        if !types::standard(self.tcx, definition)
            || self.tcx.item_name(definition).as_str() != "branch"
        {
            return None;
        }
        Some(Self::without_drop_temps(*operands.first()?))
    }

    fn checked_capacity_source(&self, initializer: &'tcx Expr<'tcx>) -> Option<HirId> {
        let (option_method, option_operands) = self.call(self.propagated_result(initializer)?)?;
        if !types::standard(self.tcx, option_method)
            || self.tcx.item_name(option_method).as_str() != "ok_or_else"
        {
            return None;
        }
        let (checked_add, checked_operands) =
            self.call(Self::without_drop_temps(*option_operands.first()?))?;
        if !types::standard(self.tcx, checked_add)
            || self.tcx.item_name(checked_add).as_str() != "checked_add"
            || self.constant_count(*checked_operands.get(1)?, &mut Vec::new()) != Some(1)
        {
            return None;
        }
        self.local_binding(*checked_operands.first()?)
    }

    fn slice_length_source(&self, initializer: &'tcx Expr<'tcx>) -> Option<HirId> {
        let (definition, operands) = self.call(Self::without_drop_temps(initializer))?;
        if !types::standard(self.tcx, definition)
            || self.tcx.item_name(definition).as_str() != "len"
            || self.expr_ty(initializer) != self.tcx.types.usize
        {
            return None;
        }
        let source_expression = *operands.first()?;
        let source = self.local_binding(source_expression)?;
        if !self.immutable_binding(source) {
            return None;
        }
        let ty::Ref(_, slice, rustc_hir::Mutability::Not) = self.expr_ty(source_expression).kind()
        else {
            return None;
        };
        matches!(slice.kind(), ty::Slice(_)).then_some(source)
    }

    fn slice_length_guard(&self, expression: &'tcx Expr<'tcx>) -> Option<HirId> {
        let ExprKind::If(condition, then, otherwise) = Self::without_drop_temps(expression).kind
        else {
            return None;
        };
        if otherwise.is_some() || !Self::guard_returns(then) {
            return None;
        }
        let ExprKind::Binary(operator, left, right) = condition.kind else {
            return None;
        };
        if operator.node != BinOpKind::Ge
            || !self.depth_limit_constant(right)
            || self.constant_count(right, &mut Vec::new()) != Some(MAX_INDEPENDENT_SLICE_VISITS)
        {
            return None;
        }
        self.local_binding(left)
    }

    fn depth_limit_constant(&self, expression: &'tcx Expr<'tcx>) -> bool {
        let expression = Self::without_drop_temps(expression);
        let ExprKind::Path(ref path) = expression.kind else {
            return false;
        };
        matches!(
            self.typeck.qpath_res(path, expression.hir_id),
            Res::Def(rustc_hir::def::DefKind::AssocConst { .. }, definition)
                if self.tcx.item_name(definition).as_str() == "INDEPENDENT_RECURSION_DEPTH"
                    && self.tcx.def_path_str(definition).contains("WorkBudget")
        )
    }

    fn guard_returns(expression: &'tcx Expr<'tcx>) -> bool {
        let ExprKind::Block(block, _) = expression.kind else {
            return false;
        };
        if block
            .expr
            .is_some_and(|tail| matches!(Self::without_drop_temps(tail).kind, ExprKind::Ret(_)))
        {
            return true;
        }
        block.stmts.last().is_some_and(|statement| {
            Self::statement_expression(statement).is_some_and(|value| {
                matches!(Self::without_drop_temps(value).kind, ExprKind::Ret(_))
            })
        })
    }

    fn fresh_vector_initializer(
        &self,
        initializer: &'tcx Expr<'tcx>,
        receiver: &'tcx Expr<'tcx>,
    ) -> bool {
        let Some((definition, operands)) = self.call(initializer) else {
            return false;
        };
        types::standard(self.tcx, definition)
            && self.tcx.item_name(definition).as_str() == "new"
            && operands.is_empty()
            && types::standard_vector(self.tcx, self.expr_ty(initializer))
            && self.expr_ty(initializer) == self.expr_ty(receiver).peel_refs()
    }

    fn only_fresh_vector_use(
        &self,
        block: &'tcx Block<'tcx>,
        declaration: usize,
        operation: usize,
        target: HirId,
    ) -> bool {
        declaration < operation
            && block.stmts[declaration + 1..operation]
                .iter()
                .all(|statement| self.local_uses_in_statement(statement, target) == Some(0))
    }

    fn vector_is_copy(&self, receiver: &'tcx Expr<'tcx>) -> bool {
        let ty::Adt(_, arguments) = self.expr_ty(receiver).peel_refs().kind() else {
            return false;
        };
        arguments.types().next().is_some_and(|element| {
            self.tcx
                .type_is_copy_modulo_regions(self.typing_env(), element)
        })
    }
}
