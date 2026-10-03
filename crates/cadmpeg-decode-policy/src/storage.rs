// SPDX-License-Identifier: Apache-2.0
//! Symbolic backing storage and typed collection admission.
use crate::{flow::ExtentTerm, types, Analysis};
use rustc_hir::{Expr, ExprKind, Node, PatKind};
use rustc_middle::ty;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SlotUse {
    Storage,
    Insertion,
    Reserve,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct SlotScope {
    pub(crate) guard: String,
    pub(crate) span: rustc_span::Span,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Slots {
    pub(crate) admission: rustc_hir::HirId,
    pub(crate) target: String,
    pub(crate) terms: Vec<ExtentTerm>,
    pub(crate) loop_depth: usize,
    pub(crate) usage: SlotUse,
    pub(crate) scope: Option<SlotScope>,
}

impl<'tcx> Analysis<'_, 'tcx> {
    pub(crate) fn record_scoped_storage(&mut self, expression: &'tcx Expr<'tcx>, terms: Vec<ExtentTerm>) {
        let Some(guard) = self.result_binding(expression) else { return; };
        let Some(scope) = self.tcx.hir_parent_iter(expression.hir_id).find_map(|(_, node)| match node {
            Node::Block(block) => Some(block.span),
            _ => None,
        }) else { return; };
        self.flow.scoped_storage.push(crate::flow::ScopedStorage { guard, scope, terms });
    }

    fn take_storage_terms(&mut self, expression: &'tcx Expr<'tcx>, required: &[ExtentTerm], element: ty::Ty<'tcx>) -> bool {
        let mut retained = self.flow.storage_extents.clone();
        let mut scoped = self.flow.scoped_storage.clone();
        if let Ok(layout) = self.tcx.layout_of(self.typing_env().as_query_input(element)) {
            let size_factor = format!("size:{element}");
            for term in retained.iter_mut().chain(scoped.iter_mut().flat_map(|credit| &mut credit.terms)) {
                if let Some(index) = term.factors.iter().position(|factor| factor == &size_factor) {
                    let Some(coefficient) = term.coefficient.checked_mul(layout.size.bytes()) else { return false; };
                    term.coefficient = coefficient;
                    term.factors.remove(index);
                }
            }
        }
        for term in required {
            if term.coefficient == 0 { continue; }
            if consume_terms(&mut retained, std::slice::from_ref(term)) { continue; }
            if !scoped.iter_mut().any(|credit| credit.scope.contains(expression.span)
                && !self.flow.mutated.contains(&credit.guard)
                && consume_terms(&mut credit.terms, std::slice::from_ref(term))) { return false; }
        }
        self.flow.storage_extents = retained;
        self.flow.scoped_storage = scoped;
        true
    }

    pub(crate) fn exact_box_capacity(&self, expression: &'tcx Expr<'tcx>) -> bool {
        let Some((definition, operands)) = self.call(expression) else { return false; };
        if !types::standard(self.tcx, definition) || self.tcx.item_name(definition).as_str() != "into_boxed_slice" { return false; }
        let Some(receiver) = operands.first() else { return false; };
        let ty::Adt(owner, arguments) = self.expr_ty(receiver).peel_refs().kind() else { return false; };
        if !types::standard(self.tcx, owner.did()) || self.tcx.item_name(owner.did()).as_str() != "Vec" { return false; }
        let Some(element) = arguments.types().next() else { return false; };
        let Some(target) = operands.first().and_then(|operand| self.key(operand, &mut Vec::new())) else { return false; };
        self.tcx.hir_parent_iter(expression.hir_id)
            .take_while(|(_, node)| !matches!(node, Node::Expr(parent) if matches!(parent.kind, ExprKind::Closure(_))))
            .any(|(_, node)| {
            let Node::Expr(parent) = node else { return false; };
            let ExprKind::If(mut condition, then, _) = parent.kind else { return false; };
            if !then.span.contains(expression.span) { return false; }
            while let ExprKind::DropTemps(inner) = condition.kind { condition = inner; }
            let ExprKind::Binary(operator, left, right) = condition.kind else { return false; };
            if operator.node != rustc_hir::BinOpKind::Eq { return false; }
            for (size, zero) in [(left, right), (right, left)] {
                if self.constant_count(zero, &mut Vec::new()) == Some(0)
                    && self.call(size).is_some_and(|(id, _)| types::standard(self.tcx, id)
                        && self.tcx.item_name(id).as_str() == "size_of")
                    && self.call_arguments(size).is_some_and(|arguments| arguments.types().next() == Some(element)) { return true; }
            }
            let Some((left_id, left_args)) = self.call(left) else { return false; };
            let Some((right_id, right_args)) = self.call(right) else { return false; };
            types::standard(self.tcx, left_id) && types::standard(self.tcx, right_id)
                && matches!((self.tcx.item_name(left_id).as_str(), self.tcx.item_name(right_id).as_str()), ("len", "capacity") | ("capacity", "len"))
                && left_args.first().and_then(|operand| self.key(operand, &mut Vec::new())).as_ref() == Some(&target)
                && right_args.first().and_then(|operand| self.key(operand, &mut Vec::new())).as_ref() == Some(&target)
                && !self.flow.mutated.contains(&target)
        })
    }

    pub(crate) fn box_storage_paid(&mut self, expression: &'tcx Expr<'tcx>, operand: &'tcx Expr<'tcx>) -> bool {
        let ty::Adt(owner, arguments) = self.expr_ty(operand).peel_refs().kind() else { return false; };
        if !types::standard(self.tcx, owner.did()) || self.tcx.item_name(owner.did()).as_str() != "Vec" { return false; }
        let Some(element) = arguments.types().next() else { return false; };
        let Some(target) = self.key(operand, &mut Vec::new()) else { return false; };
        let Some(mut required) = self.scaled_storage_terms(&[ExtentTerm { factors: vec![target], coefficient: 1 }]) else { return false; };
        let size_factor = format!("size:{element}");
        for term in &mut required { term.factors.push(size_factor.clone()); term.factors.sort(); }
        if let Ok(layout) = self.tcx.layout_of(self.typing_env().as_query_input(element)) {
            for term in &mut required {
                if let Some(index) = term.factors.iter().position(|factor| factor == &size_factor) {
                    let Some(coefficient) = term.coefficient.checked_mul(layout.size.bytes()) else { return false; };
                    term.coefficient = coefficient;
                    term.factors.remove(index);
                }
            }
        }
        self.take_storage_terms(expression, &required, element)
    }

    pub(crate) fn symbolic_storage(
        &mut self,
        expression: &'tcx Expr<'tcx>,
        operands: &[&'tcx Expr<'tcx>],
        name: &str,
    ) -> bool {
        if !matches!(name, "try_reserve_exact" | "reserve_exact") {
            return false;
        }
        let Some(receiver) = operands.first() else {
            return false;
        };
        let ty::Adt(owner, arguments) = self.expr_ty(receiver).peel_refs().kind() else {
            return false;
        };
        if !types::standard(self.tcx, owner.did()) {
            return false;
        }
        let element = match self.tcx.item_name(owner.did()).as_str() {
            "Vec" => match arguments.types().next() {
                Some(element) => element,
                None => return false,
            },
            "String" => self.tcx.types.u8,
            "VecDeque" => match arguments.types().next() { Some(element) => element, None => return false },
            _ => return false,
        };
        let Some(mut terms) = operands
            .get(1)
            .and_then(|count| self.extent_terms(count, &mut Vec::new()))
        else {
            return false;
        };
        let count = operands.get(1).copied();
        if let Some(count) = count {
            if let ExprKind::Binary(operator, capacity, length) = count.kind {
                if operator.node == rustc_hir::BinOpKind::Sub
                    && self.call(length).is_some_and(|(id, values)| {
                        types::standard(self.tcx, id)
                            && self.tcx.item_name(id).as_str() == "len"
                            && values
                                .first()
                                .and_then(|value| self.key(value, &mut Vec::new()))
                                == self.key(receiver, &mut Vec::new())
                    })
                {
                    if let (Some(left), Some(target)) = (
                        self.extent_terms(capacity, &mut Vec::new()),
                        self.key(receiver, &mut Vec::new()),
                    ) {
                        let right = vec![ExtentTerm {
                            factors: vec![format!("{target}.capacity")],
                            coefficient: 1,
                        }];
                        terms = vec![ExtentTerm {
                            factors: vec![format!("difference:{left:?}:{right:?}")],
                            coefficient: 1,
                        }];
                    }
                }
            }
        }
        let counts = terms.clone();
        let Some(scaled) = self.scaled_storage_terms(&terms) else {
            return false;
        };
        terms = scaled;
        let size_factor = format!("size:{element}");
        for term in &mut terms {
            term.factors.push(size_factor.clone());
            term.factors.sort();
        }
        if let Ok(layout) = self
            .tcx
            .layout_of(self.typing_env().as_query_input(element))
        {
            let bytes = layout.size.bytes();
            for term in &mut terms {
                if let Some(index) = term
                    .factors
                    .iter()
                    .position(|factor| factor == &size_factor)
                {
                    let Some(coefficient) = term.coefficient.checked_mul(bytes) else {
                        return false;
                    };
                    term.coefficient = coefficient;
                    term.factors.remove(index);
                }
            }
        }
        if !self.take_storage_terms(expression, &terms, element) {
            return false;
        }
        if name == "reserve_exact" || self.reserve_success(expression) {
            if let Some(target) = self.key(receiver, &mut Vec::new()) {
                self.flow.storage_slots.push(Slots {
                    admission: expression.hir_id,
                    target,
                    terms: counts,
                    loop_depth: self.flow.loop_bounds.len(),
                    usage: SlotUse::Insertion,
                    scope: None,
                });
            }
        }
        true
    }

    pub(crate) fn scaled_storage_terms(&self, terms: &[ExtentTerm]) -> Option<Vec<ExtentTerm>> {
        let mut required = terms.to_vec();
        for bounds in &self.flow.loop_bounds {
            let mut product = Vec::new();
            for term in &required {
                for bound in bounds {
                    let mut factors = term.factors.clone();
                    factors.extend(bound.factors.clone());
                    factors.sort();
                    product.push(ExtentTerm {
                        factors,
                        coefficient: term.coefficient.checked_mul(bound.coefficient)?,
                    });
                }
            }
            required = product;
        }
        Some(required)
    }

    fn reserve_success(&self, expression: &'tcx Expr<'tcx>) -> bool {
        for (_, node) in self.tcx.hir_parent_iter(expression.hir_id) {
            match node {
                Node::Expr(parent) => match parent.kind {
                    ExprKind::Match(_, _, rustc_hir::MatchSource::TryDesugar(_)) => return true,
                    ExprKind::DropTemps(_) | ExprKind::AddrOf(_, _, _) => (),
                    ExprKind::Call(_, _) | ExprKind::MethodCall(_, _, _, _)
                        if self.call(parent).is_some_and(|(id, _)| {
                            types::standard(self.tcx, id)
                                && matches!(self.tcx.item_name(id).as_str(), "map_err" | "branch")
                        }) => {}
                    _ => return false,
                },
                _ => return false,
            }
        }
        false
    }

    fn result_binding(&self, expression: &Expr<'tcx>) -> Option<String> {
        self.result_binding_at(expression, 0)
    }

    fn result_binding_at(&self, expression: &Expr<'tcx>, index: usize) -> Option<String> {
        for (_, node) in self.tcx.hir_parent_iter(expression.hir_id) {
            match node {
                Node::LetStmt(local) => {
                    let pattern = match local.pat.kind {
                        PatKind::Tuple(patterns, _) => patterns.get(index)?,
                        _ if index == 0 => local.pat,
                        _ => return None,
                    };
                    return match pattern.kind {
                        PatKind::Binding(_, id, _, _) => Some(format!("local:{id:?}")),
                        _ => None,
                    };
                }
                Node::Expr(value) => match value.kind {
                    ExprKind::Match(_, _, rustc_hir::MatchSource::TryDesugar(_))
                    | ExprKind::DropTemps(_) => (),
                    ExprKind::Call(_, _)
                        if self.call(value).is_some_and(|(id, _)| {
                            types::standard(self.tcx, id)
                                && self.tcx.item_name(id).as_str() == "branch"
                        }) => {}
                    ExprKind::MethodCall(_, _, _, _)
                        if self.call(value).is_some_and(|(id, _)| {
                            types::standard(self.tcx, id)
                                && self.tcx.item_name(id).as_str() == "map_err"
                        }) => {}
                    _ => return None,
                },
                Node::Param(_) | Node::Item(_) => return None,
                _ => (),
            }
        }
        None
    }

    fn growth_scope(&self, expression: &Expr<'tcx>, index: usize) -> Option<SlotScope> {
        let guard = self.result_binding_at(expression, index)?;
        let span = self.tcx.hir_parent_iter(expression.hir_id).find_map(|(_, node)| match node {
            Node::Block(block) => Some(block.span),
            _ => None,
        })?;
        Some(SlotScope { guard, span })
    }

    pub(crate) fn record_slots(&mut self, expression: &'tcx Expr<'tcx>) {
        if !self.trusted_context_callee(expression) {
            return;
        }
        let Some((definition, operands)) = self.call(expression) else {
            return;
        };
        let name = self.tcx.item_name(definition);
        let (target, terms) = match name.as_str() {
            "collection_vec" | "vector_storage" | "scoped_vector_storage" => (
                self.result_binding(expression),
                operands
                    .get(1)
                    .and_then(|count| self.extent_terms(count, &mut Vec::new())),
            ),
            "reserve_vec"
            | "reserve_vec_limit"
            | "reserve_capacity"
            | "reserve_capacity_limit"
            | "try_reserve_retained_text"
            | "reserve_retained_vec_storage"
            | "reserve_set"
            | "reserve_map"
                | "reserve_hash_map_storage"
                | "reserve_hash_set_storage"
            | "reserve_heap"
            | "reserve_temporary_vec" => (
                operands
                    .get(1)
                    .and_then(|target| self.key(target, &mut Vec::new())),
                operands
                    .get(2)
                    .and_then(|count| self.extent_terms(count, &mut Vec::new())),
            ),
            "reserve_scoped_vec" | "reserve_scoped_vec_limit" => (
                operands
                    .get(2)
                    .and_then(|target| self.key(target, &mut Vec::new())),
                operands
                    .get(3)
                    .and_then(|count| self.extent_terms(count, &mut Vec::new())),
            ),
            "admit_hash_map_entry" | "admit_btree_entry" => (
                operands
                    .get(1)
                    .and_then(|target| self.key(target, &mut Vec::new())),
                Some(vec![ExtentTerm {
                    factors: Vec::new(),
                    coefficient: 1,
                }]),
            ),
            "admit_btree_node_storage" => {
                let Some((length_id, operands)) = operands.get(1).and_then(|value| self.call(value)) else { return; };
                if !types::standard(self.tcx, length_id) || self.tcx.item_name(length_id).as_str() != "len" { return; }
                let Some(value) = operands.first() else { return; };
                let ty::Adt(owner, args) = self.expr_ty(value).peel_refs().kind() else { return; };
                if !types::standard(self.tcx, owner.did()) { return; }
                let Some(call_args) = self.call_arguments(expression) else { return; };
                let admitted = call_args.types().collect::<Vec<_>>();
                let actual = args.types().collect::<Vec<_>>();
                let matches = match self.tcx.item_name(owner.did()).as_str() {
                    "BTreeMap" => admitted.len() == 2 && admitted.iter().eq(actual.iter().take(2)),
                    "BTreeSet" => admitted.first() == actual.first() && admitted.get(1) == Some(&self.tcx.types.unit),
                    _ => false,
                };
                if !matches { return; }
                (self.key(value, &mut Vec::new()), Some(vec![ExtentTerm { factors: Vec::new(), coefficient: 1 }]))
            }
            "linear_growth" => {
                let Some((length_id, length)) = operands.get(1).and_then(|value| self.call(value)) else { return; };
                let Some((capacity_id, capacity)) = operands.get(2).and_then(|value| self.call(value)) else { return; };
                if !types::standard(self.tcx, length_id) || self.tcx.item_name(length_id).as_str() != "len"
                    || !types::standard(self.tcx, capacity_id) || self.tcx.item_name(capacity_id).as_str() != "capacity" { return; }
                let Some(value) = length.first() else { return; };
                let target = self.key(value, &mut Vec::new());
                if target != capacity.first().and_then(|value| self.key(value, &mut Vec::new())) { return; }
                let ty::Adt(owner, args) = self.expr_ty(value).peel_refs().kind() else { return; };
                if !types::standard(self.tcx, owner.did()) || !matches!(self.tcx.item_name(owner.did()).as_str(), "Vec" | "VecDeque" | "BinaryHeap" | "String") { return; }
                let Some(call_args) = self.call_arguments(expression) else { return; };
                let element = if self.tcx.item_name(owner.did()).as_str() == "String" { Some(self.tcx.types.u8) } else { args.types().next() };
                if element != call_args.types().next() { return; }
                let Some(target) = target else { return; };
                let Some(result) = self.result_binding(expression) else { return; };
                let Some(scope) = self.growth_scope(expression, 2) else { return; };
                // The helper admits its returned reserve count and the requested
                // insertion slots for this exact collection and element type.
                if let Some(original) = operands.get(3).and_then(|count| self.extent_terms(count, &mut Vec::new())) {
                    self.flow.storage_slots.push(Slots {
                        admission: expression.hir_id,
                        target: target.clone(),
                        terms: original,
                        loop_depth: self.flow.loop_bounds.len(),
                        usage: SlotUse::Insertion,
                        scope: None,
                    });
                }
                self.flow.storage_slots.push(Slots {
                    admission: expression.hir_id,
                    target,
                    terms: vec![ExtentTerm { factors: vec![result], coefficient: 1 }],
                    loop_depth: self.flow.loop_bounds.len(),
                    usage: SlotUse::Reserve,
                    scope: Some(scope),
                });
                return;
            }
            "charge_hash_growth" => {
                let Some((length_id, length)) = operands.get(1).and_then(|value| self.call(value)) else { return; };
                let Some((capacity_id, _)) = operands.get(2).and_then(|value| self.call(value)) else { return; };
                if !types::standard(self.tcx, length_id) || self.tcx.item_name(length_id).as_str() != "len"
                    || !types::standard(self.tcx, capacity_id) || self.tcx.item_name(capacity_id).as_str() != "capacity" { return; }
                let Some(value) = length.first() else { return; };
                let ty::Adt(owner, args) = self.expr_ty(value).peel_refs().kind() else { return; };
                if !types::standard(self.tcx, owner.did()) { return; }
                let actual = args.types().collect::<Vec<_>>();
                let element = match self.tcx.item_name(owner.did()).as_str() {
                    "HashSet" => actual.first().copied(),
                    "HashMap" if actual.len() >= 2 => Some(ty::Ty::new_tup(self.tcx, &actual[..2])),
                    _ => None,
                };
                if element.is_none() || element != self.call_arguments(expression).and_then(|args| args.types().next()) { return; }
                let target = operands
                    .get(1)
                    .and_then(|length| self.call(length))
                    .and_then(|(_, values)| {
                        values
                            .first()
                            .and_then(|target| self.key(target, &mut Vec::new()))
                    });
                let capacity_target = operands
                    .get(2)
                    .and_then(|capacity| self.call(capacity))
                    .and_then(|(_, values)| {
                        values
                            .first()
                            .and_then(|target| self.key(target, &mut Vec::new()))
                    });
                if target != capacity_target {
                    return;
                }
                (
                    target,
                    operands
                        .get(3)
                        .and_then(|count| self.extent_terms(count, &mut Vec::new())),
                )
            }
            _ => return,
        };
        if let (Some(target), Some(terms)) = (target, terms) {
            let scope = if name.as_str() == "charge_hash_growth" {
                let Some(scope) = self.growth_scope(expression, 1) else { return; };
                Some(scope)
            } else { None };
            self.flow.storage_slots.push(Slots {
                admission: expression.hir_id,
                target,
                terms,
                loop_depth: self.flow.loop_bounds.len(),
                usage: SlotUse::Storage,
                scope,
            });
        }
    }

    pub(crate) fn admitted_slots(&mut self, expression: &'tcx Expr<'tcx>, operands: &[&'tcx Expr<'tcx>], name: &str) -> bool {
        let Some(receiver) = operands.first() else {
            return false;
        };
        let Some(target) = self.key(receiver, &mut Vec::new()) else {
            return false;
        };
        let terms = match name {
            "push" | "push_back" | "push_front" | "insert" => Some(vec![ExtentTerm {
                factors: Vec::new(),
                coefficient: 1,
            }]),
            "reserve" | "reserve_exact" | "try_reserve" | "try_reserve_exact" | "resize"
            | "resize_with" => operands
                .get(1)
                .and_then(|count| self.extent_terms(count, &mut Vec::new())),
            "append" | "extend_from_slice" | "push_str" => operands
                .get(1)
                .and_then(|source| self.key(source, &mut Vec::new()))
                .map(|key| {
                    vec![ExtentTerm {
                        factors: vec![key],
                        coefficient: 1,
                    }]
                }),
            _ => None,
        };
        let Some(terms) = terms else {
            return false;
        };
        for (index, credit) in self.flow.storage_slots.iter().enumerate() {
            let reserve = matches!(name, "reserve" | "reserve_exact" | "try_reserve" | "try_reserve_exact");
            if credit.target != target
                || credit.scope.as_ref().is_some_and(|scope| !scope.span.contains(expression.span) || self.flow.mutated.contains(&scope.guard))
                || credit.usage == SlotUse::Insertion && reserve
                || credit.usage == SlotUse::Reserve && !reserve
            {
                continue;
            }
            let mut required = terms.clone();
            let mut valid = true;
            for bounds in self.flow.loop_bounds.iter().skip(credit.loop_depth) {
                let mut product = Vec::new();
                for term in &required {
                    for bound in bounds {
                        let Some(coefficient) = term.coefficient.checked_mul(bound.coefficient)
                        else {
                            valid = false;
                            continue;
                        };
                        let mut factors = term.factors.clone();
                        factors.extend(bound.factors.clone());
                        factors.sort();
                        product.push(ExtentTerm {
                            factors,
                            coefficient,
                        });
                    }
                }
                required = product;
            }
            let mut remaining = credit.terms.clone();
            if valid && consume_terms(&mut remaining, &required) {
                if remaining.is_empty() {
                    self.flow.storage_slots.remove(index);
                } else {
                    self.flow.storage_slots[index].terms = remaining;
                }
                return true;
            }
        }
        false
    }
}

// A receipt admits each equal or dominated extent once.
pub(crate) fn consume_terms(credits: &mut Vec<ExtentTerm>, required: &[ExtentTerm]) -> bool {
    let mut remaining = credits.clone();
    for term in required {
        if term.coefficient == 0 {
            continue;
        }
        let Some(index) = remaining.iter().position(|credit| {
            credit.factors == term.factors && credit.coefficient >= term.coefficient
        }) else {
            return false;
        };
        remaining[index].coefficient -= term.coefficient;
        remaining.retain(|credit| credit.coefficient != 0);
    }
    *credits = remaining;
    true
}
