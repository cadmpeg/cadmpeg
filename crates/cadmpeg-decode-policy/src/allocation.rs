// SPDX-License-Identifier: Apache-2.0
use crate::{external, types, Analysis};
use rustc_hir::intravisit::{walk_expr, Visitor};
use rustc_hir::{Expr, ExprKind};
use rustc_middle::ty;
use types::Shape;

impl<'tcx> Analysis<'_, 'tcx> {
    pub(crate) fn zero_extent(&self, expression: &Expr<'tcx>) -> bool {
        matches!(expression.kind, ExprKind::Lit(literal) if matches!(literal.node, rustc_ast::LitKind::Int(value, _) if value.get() == 0))
    }

    pub(crate) fn fixed_value(&self, expression: &'tcx Expr<'tcx>) -> bool {
        if self.constant(expression, &mut Vec::new()) {
            return true;
        }
        match expression.kind {
            ExprKind::Lit(_) => true,
            ExprKind::AddrOf(_, _, inner) | ExprKind::DropTemps(inner) => self.fixed_value(inner),
            _ => {
                let value = self.expr_ty(expression).peel_refs();
                types::heap(self.tcx, value, &mut Vec::new()) == Shape::Fixed
                    && types::work(self.tcx, value, &mut Vec::new()) == Shape::Fixed
                    && match value.kind() {
                        ty::Str | ty::Slice(_) => false,
                        _ => self
                            .tcx
                            .type_is_copy_modulo_regions(self.typing_env(), value),
                    }
            }
        }
    }

    pub(crate) fn format_shape(&self, operands: &[&'tcx Expr<'tcx>]) -> Shape {
        let mut format = FormatShape {
            analysis: self,
            shape: Shape::Fixed,
        };
        for operand in operands {
            format.visit_expr(operand);
        }
        format.shape
    }

    fn symbolic_storage(&mut self, operands: &[&'tcx Expr<'tcx>], name: &str) -> bool {
        if !matches!(name, "try_reserve_exact" | "reserve_exact") { return false; }
        let Some(receiver) = operands.first() else { return false; };
        let ty::Adt(owner, arguments) = self.expr_ty(receiver).peel_refs().kind() else { return false; };
        if !types::standard(self.tcx, owner.did()) || self.tcx.item_name(owner.did()).as_str() != "Vec" { return false; }
        let Some(element) = arguments.types().next() else { return false; };
        let Some(mut terms) = operands.get(1).and_then(|count| self.extent_terms(count, &mut Vec::new())) else { return false; };
        for term in &mut terms { term.factors.push(format!("size:{element}")); term.factors.sort(); }
        if terms.iter().all(|term| self.flow.storage_extents.contains(term)) {
            for term in terms { if let Some(index) = self.flow.storage_extents.iter().position(|credit| credit == &term) { self.flow.storage_extents.remove(index); } }
            return true;
        }
        false
    }

    pub(crate) fn allocation(&mut self, expression: &'tcx Expr<'tcx>) {
        let Some((definition, operands)) = self.call(expression) else {
            return;
        };
        let name = self.tcx.item_name(definition);
        let name = name.as_str();
        if self.checked_call(expression, definition) && name != "alloc_filled" {
            return;
        }
        let summary = external::summary(
            self.tcx,
            self.implementation(expression, definition)
                .unwrap_or(definition),
            operands.first().map(|operand| self.expr_ty(operand)),
        );
        let allocation = summary.map(|summary| summary.allocation);
        if allocation == Some(external::Allocation::None) {
            return;
        }
        if summary.is_some_and(|summary| {
            summary
                .zero_operand
                .and_then(|index| operands.get(index))
                .is_some_and(|operand| self.zero_extent(operand))
        }) {
            return;
        }
        if summary.is_some_and(|summary| summary.empty_operand.and_then(|index| operands.get(index))
            .is_some_and(|operand| matches!(operand.kind, ExprKind::Array(values) if values.is_empty())
                || matches!(operand.kind, ExprKind::Lit(literal) if matches!(literal.node, rustc_ast::LitKind::Str(value, _) if value.as_str().is_empty())))) {
            return;
        }
        if allocation == Some(external::Allocation::Conversion)
            && matches!(self.expr_ty(expression).kind(), ty::Adt(owner, _) if types::standard(self.tcx, owner.did()) && self.tcx.item_name(owner.did()).as_str() == "Cow")
        {
            return;
        }
        let context_call = operands.first().is_some_and(|operand| {
            types::has_context(self.tcx, self.expr_ty(operand), &mut Vec::new())
        });
        if context_call && self.checked_call(expression, definition) {
            if name == "alloc_filled"
                && !operands.get(1).is_some_and(|count| self.zero_extent(count))
            {
                if let Some(value) = operands.get(2) {
                    let shape = self.clone_shape(self.expr_ty(value));
                    let empty = self.constant(value, &mut Vec::new())
                        || self.call(value).is_some_and(|(id, args)| {
                            types::standard(self.tcx, id)
                                && args.is_empty()
                                && matches!(self.tcx.item_name(id).as_str(), "new" | "default")
                        });
                    if !empty {
                        self.shape_report(expression, shape, "alloc_filled child Clone");
                    }
                }
            }
            return;
        }
        if let Some(external::Allocation::Input(index)) = allocation {
            let shape = operands.get(index).map_or(Shape::Unknown, |operand| if self.fixed_value(operand) { Shape::Fixed } else { types::work(self.tcx, self.expr_ty(operand), &mut Vec::new()) });
            self.shape_report(expression, shape, "external operation temporary or result storage");
            return;
        }
        if allocation == Some(external::Allocation::Growth) {
            if self.symbolic_storage(&operands, name) { return; }
            if let Some(receiver) = operands.first() {
                let receiver_type = self.expr_ty_adjusted(receiver).peel_refs();
                let shape = if matches!(receiver_type.kind(), ty::Adt(owner, _) if types::standard(self.tcx, owner.did()) && matches!(self.tcx.item_name(owner.did()).as_str(), "Vec" | "VecDeque" | "BinaryHeap")) {
                    match types::heap(self.tcx, receiver_type, &mut Vec::new()) {
                        Shape::Unknown => Shape::Dynamic,
                        shape => shape,
                    }
                } else { types::heap(self.tcx, receiver_type, &mut Vec::new()) };
                self.shape_report(
                    expression,
                    if shape == Shape::Dynamic
                        && (self.flow.storage
                            || (self.constant(receiver, &mut Vec::new())
                                && (!matches!(name, "extend" | "extend_from_slice" | "append")
                                    || operands.get(1).is_none_or(|operand| {
                                        self.iteration(operand, &mut Vec::new()) != Shape::Dynamic
                                    })))
                            || self
                                .key(receiver, &mut Vec::new())
                                .is_some_and(|key| self.flow.mutated.contains(&key)))
                    {
                        Shape::Unknown
                    } else {
                        shape
                    },
                    "collection growth outside core operation",
                );
            }
            return;
        }
        if matches!(
            self.tcx.def_kind(definition),
            rustc_hir::def::DefKind::Ctor(_, _)
        ) {
            return;
        }
        if name == "cloned" && types::standard(self.tcx, definition) {
            let value = operands
                .first()
                .and_then(|operand| self.iterator_item(self.expr_ty(operand)))
                .map(|value| value.peel_refs());
            let shape = value.map_or(Shape::Unknown, |value| self.clone_shape(value));
            if shape != Shape::Fixed {
                let consumed = self
                    .tcx
                    .hir_parent_iter(expression.hir_id)
                    .any(|(_, node)| match node {
                        rustc_hir::Node::Expr(parent) => {
                            self.call(parent).is_some_and(|(id, _)| {
                                matches!(
                                    self.tcx.item_name(id).as_str(),
                                    "collect"
                                        | "from_iter"
                                        | "collect_vec"
                                        | "try_collect_vec"
                                        | "any"
                                        | "find"
                                        | "for_each"
                                )
                            })
                        }
                        _ => false,
                    });
                self.shape_report(
                    expression,
                    if consumed { shape } else { Shape::Unknown },
                    "cloned iterator child copies",
                );
            }
            return;
        }
        if allocation.is_none() {
            return;
        }
        if matches!(name, "from" | "into") && types::standard(self.tcx, definition) {
            if let ty::Adt(result, _) = self.expr_ty(expression).kind() {
                if types::standard(self.tcx, result.did())
                    && matches!(self.tcx.item_name(result.did()).as_str(), "Rc" | "Arc")
                    && operands.first().is_some_and(|operand| {
                        matches!(
                            self.expr_ty(operand).peel_refs().kind(),
                            ty::Slice(_) | ty::Str
                        )
                    })
                {
                    let shape = if operands
                        .first()
                        .is_some_and(|operand| self.constant(operand, &mut Vec::new()))
                    {
                        Shape::Fixed
                    } else {
                        Shape::Dynamic
                    };
                    self.shape_report(expression, shape, "shared owned copy");
                    return;
                }
            }
        }
        if name == "clone" {
            match self.implementation(expression, definition) {
                Some(id) if !types::standard(self.tcx, id) => {
                    if !types::derived(self.tcx, id) {
                        return;
                    }
                }
                None => {
                    self.shape_report(
                        expression,
                        Shape::Unknown,
                        "Clone implementation unresolved",
                    );
                    return;
                }
                _ => (),
            }
        }
        if name == "default" && self.implementation(expression, definition).is_some_and(|id| types::derived(self.tcx, id)) {
            let shape = self.default_shape(self.expr_ty(expression), &mut Vec::new());
            self.shape_report(expression, shape, "derived Default");
            return;
        }
        let result = types::heap(self.tcx, self.expr_ty(expression), &mut Vec::new());
        if result == Shape::Fixed {
            return;
        }
        if name == "format" && types::standard(self.tcx, definition) {
            let shape = self.format_shape(&operands);
            self.shape_report(expression, shape, "format!");
            return;
        }
        if matches!(name, "to_string" | "to_owned" | "clone")
            && operands
                .first()
                .is_some_and(|operand| self.fixed_value(operand))
        {
            return;
        }
        if matches!(name, "from" | "into" | "to_owned") {
            if let Some(custom) = self.custom_trait(expression, definition) {
                if !self.checked_body(custom) {
                    self.shape_report(
                        expression,
                        Shape::Unknown,
                        "custom conversion allocator reachability",
                    );
                }
                return;
            }
            if self.implementation(expression, definition).is_none() {
                self.shape_report(
                    expression,
                    Shape::Unknown,
                    "conversion implementation unresolved",
                );
                return;
            }
        }
        if name == "to_string"
            && operands.first().is_some_and(|operand| {
                match self.expr_ty(operand).peel_refs().kind() {
                    ty::Str => false,
                    ty::Adt(definition, _) => !types::standard(self.tcx, definition.did()),
                    _ => true,
                }
            })
        {
            self.shape_report(
                expression,
                Shape::Unknown,
                "Display output extent unresolved",
            );
            return;
        }
        if types::standard(self.tcx, definition) {
            if matches!(name, "collect" | "from_iter") {
                if let Some(shape) = operands
                    .first()
                    .and_then(|source| self.vector_collection_reuse(expression, source))
                {
                    self.shape_report(expression, shape, "vector collection storage reuse");
                    return;
                }
            }
            if matches!(name, "collect" | "from_iter" | "to_vec") {
                let shape = operands.first().map_or(Shape::Unknown, |operand| {
                    self.iteration(operand, &mut Vec::new())
                });
                self.shape_report(
                    expression,
                    if result == Shape::Unknown {
                        Shape::Unknown
                    } else {
                        shape
                    },
                    name,
                );
                return;
            }
            if matches!(name, "from" | "into" | "into_owned") {
                if let Some(operand) = operands.first() {
                    if self.constant(operand, &mut Vec::new()) {
                        return;
                    }
                    if self.expr_ty(operand) == self.expr_ty(expression) {
                        return;
                    }
                    let shape = match self.expr_ty(operand).peel_refs().kind() {
                        ty::Str | ty::Slice(_) => Shape::Dynamic,
                        ty::Array(_, _) => Shape::Fixed,
                        _ => Shape::Unknown,
                    };
                    self.shape_report(expression, shape, name);
                    return;
                }
            }
            if matches!(name, "with_capacity" | "with_capacity_in" | "from_elem") {
                let count = if name == "from_elem" {
                    operands.get(1)
                } else {
                    operands.first()
                };
                if count.is_some_and(|count| self.constant(count, &mut Vec::new()))
                    && (name != "from_elem"
                        || operands.first().is_some_and(|value| {
                            types::heap(self.tcx, self.expr_ty(value), &mut Vec::new())
                                == Shape::Fixed
                        }))
                {
                    return;
                }
                self.shape_report(
                    expression,
                    if result == Shape::Unknown {
                        Shape::Unknown
                    } else {
                        Shape::Dynamic
                    },
                    name,
                );
                return;
            }
            if name == "into_boxed_slice" {
                self.shape_report(
                    expression,
                    Shape::Unknown,
                    "into_boxed_slice may shrink/reallocate: capacity equality unresolved",
                );
                return;
            }
        }
        self.shape_report(
            expression,
            if name == "clone" {
                self.clone_shape(self.expr_ty(expression))
            } else {
                result
            },
            name,
        );
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
            if types::standard(self.analysis.tcx, definition)
                && (name.as_str().starts_with("new_") || name.as_str() == "from_usize")
                && self.analysis.tcx.def_path_str(definition).contains("fmt")
            {
                for operand in operands {
                    if !self.analysis.fixed_value(operand) {
                        let value = self.analysis.expr_ty(operand).peel_refs();
                        let shape = match value.kind() {
                            ty::Str | ty::Slice(_) => Shape::Dynamic,
                            ty::Adt(owner, _) if !types::standard(self.analysis.tcx, owner.did()) => {
                                if name.as_str() == "new_debug" && self.analysis.trait_method(value, "Debug", "fmt").is_some_and(|id| types::derived(self.analysis.tcx, id)) {
                                    types::work(self.analysis.tcx, value, &mut Vec::new())
                                } else { Shape::Unknown }
                            }
                            _ => types::heap(self.analysis.tcx, value, &mut Vec::new()),
                        };
                        self.shape = self.shape.join(if shape == Shape::Fixed && !self.analysis.trait_method(value, "Debug", "fmt").is_some_and(|id| types::derived(self.analysis.tcx, id)) {
                            Shape::Unknown
                        } else { shape });
                    } else if name.as_str() == "from_usize"
                        && !self.analysis.constant(operand, &mut Vec::new())
                    {
                        self.shape = self.shape.join(Shape::Unknown);
                    }
                }
            }
        }
        walk_expr(self, expression);
    }
}
