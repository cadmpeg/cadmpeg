// SPDX-License-Identifier: Apache-2.0
//! Symbolic backing storage and typed collection admission.
use crate::{flow::ExtentTerm, types, Analysis};
use rustc_hir::{Expr, ExprKind, Node, PatKind};
use rustc_middle::ty;

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Slots {
    pub(crate) admission: rustc_hir::HirId,
    pub(crate) target: String,
    pub(crate) terms: Vec<ExtentTerm>,
    pub(crate) loop_depth: usize,
    pub(crate) reserved: bool,
}

impl<'tcx> Analysis<'_, 'tcx> {
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
        let mut credits = self.flow.storage_extents.clone();
        if let Ok(layout) = self
            .tcx
            .layout_of(self.typing_env().as_query_input(element))
        {
            let bytes = layout.size.bytes();
            for term in terms.iter_mut().chain(credits.iter_mut()) {
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
        if !consume_terms(&mut credits, &terms) {
            return false;
        }
        self.flow.storage_extents = credits;
        if name == "reserve_exact" || self.reserve_success(expression) {
            if let Some(target) = self.key(receiver, &mut Vec::new()) {
                self.flow.storage_slots.push(Slots {
                    admission: expression.hir_id,
                    target,
                    terms: counts,
                    loop_depth: self.flow.loop_bounds.len(),
                    reserved: true,
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

    fn allocated_binding(&self, expression: &Expr<'tcx>) -> Option<String> {
        for (_, node) in self.tcx.hir_parent_iter(expression.hir_id) {
            match node {
                Node::LetStmt(local) => {
                    let pattern = match local.pat.kind {
                        PatKind::Tuple(patterns, _) => patterns.first()?,
                        _ => local.pat,
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
                self.allocated_binding(expression),
                operands
                    .get(1)
                    .and_then(|count| self.extent_terms(count, &mut Vec::new())),
            ),
            "reserve_vec"
            | "reserve_vec_limit"
            | "reserve_capacity"
            | "reserve_capacity_limit"
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
            "charge_hash_growth" => {
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
            self.flow.storage_slots.push(Slots {
                admission: expression.hir_id,
                target,
                terms,
                loop_depth: self.flow.loop_bounds.len(),
                reserved: false,
            });
        }
    }

    pub(crate) fn admitted_slots(&mut self, operands: &[&'tcx Expr<'tcx>], name: &str) -> bool {
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
            if credit.target != target
                || credit.reserved
                    && matches!(
                        name,
                        "reserve" | "reserve_exact" | "try_reserve" | "try_reserve_exact"
                    )
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
