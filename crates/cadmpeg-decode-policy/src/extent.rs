// SPDX-License-Identifier: Apache-2.0
use crate::{types, Analysis};
use rustc_hir::{def::Res, Expr, ExprKind, Node};
use rustc_middle::ty;
use types::Shape;

impl<'tcx> Analysis<'_, 'tcx> {
    pub(crate) fn constant_count(
        &self,
        expression: &'tcx Expr<'tcx>,
        seen: &mut Vec<rustc_hir::HirId>,
    ) -> Option<u64> {
        if seen.contains(&expression.hir_id) {
            return None;
        }
        seen.push(expression.hir_id);
        if let ty::Array(_, length) = self.expr_ty(expression).peel_refs().kind() {
            return length.try_to_target_usize(self.tcx);
        }
        match expression.kind {
            ExprKind::Lit(literal) => match literal.node {
                rustc_ast::LitKind::Int(value, _) => u64::try_from(value.get()).ok(),
                _ => None,
            },
            ExprKind::AddrOf(_, _, inner) | ExprKind::DropTemps(inner) => {
                self.constant_count(inner, seen)
            }
            ExprKind::Cast(inner, _) => {
                let count = self.constant_count(inner, seen)?;
                let bits = match self.expr_ty(expression).kind() {
                    ty::Uint(width) => width
                        .bit_width()
                        .unwrap_or(self.tcx.data_layout.pointer_size().bits()),
                    _ => return None,
                };
                if bits >= 64 || count < 1u64.checked_shl(u32::try_from(bits).ok()?)? {
                    Some(count)
                } else {
                    None
                }
            }
            ExprKind::Struct(_, fields, _) if matches!(self.expr_ty(expression).kind(), ty::Adt(owner, _) if types::standard(self.tcx, owner.did()) && self.tcx.item_name(owner.did()).as_str() == "Range") =>
            {
                self.range_width(fields, seen)
            }
            ExprKind::Struct(_, fields, _) if matches!(self.expr_ty(expression).kind(), ty::Adt(owner, _) if types::standard(self.tcx, owner.did()) && self.tcx.item_name(owner.did()).as_str() == "RangeTo") => {
                self.constant_count(fields.first()?.expr, seen)
            }
            ExprKind::Index(_, range, _) if matches!(self.expr_ty(range).kind(), ty::Adt(owner, _) if types::standard(self.tcx, owner.did()) && matches!(self.tcx.item_name(owner.did()).as_str(), "Range" | "RangeTo")) => self.constant_count(range, seen),
            ExprKind::Match(scrutinee, _, rustc_hir::MatchSource::TryDesugar(_)) => {
                let (_, operands) = self.call(scrutinee)?;
                self.constant_count(operands.first()?, seen)
            }
            ExprKind::Path(ref path) => match self.typeck.qpath_res(path, expression.hir_id) {
                Res::Def(rustc_hir::def::DefKind::Const { .. }, definition) => self
                    .tcx
                    .const_eval_poly(definition)
                    .ok()?
                    .try_to_scalar()?
                    .to_u64()
                    .discard_err(),
                _ => self
                    .initializer(expression)
                    .and_then(|init| self.constant_count(init, seen)),
            },
            _ => {
                let (id, operands) = self.call(expression)?;
                if types::standard(self.tcx, id)
                    && matches!(
                        self.tcx.item_name(id).as_str(),
                        "len" | "iter" | "into_iter" | "unwrap" | "expect" | "as_ref" | "as_bytes" | "as_slice" | "Some"
                    )
                {
                    self.constant_count(operands.first()?, seen)
                } else if types::standard(self.tcx, id) && self.tcx.item_name(id).as_str() == "get"
                    && operands.get(1).is_some_and(|range| matches!(self.expr_ty(range).kind(), ty::Adt(owner, _) if types::standard(self.tcx, owner.did()) && matches!(self.tcx.item_name(owner.did()).as_str(), "Range" | "RangeTo"))) {
                    self.constant_count(operands.get(1)?, seen)
                } else {
                    None
                }
            }
        }
    }

    fn range_width(&self, fields: &'tcx [rustc_hir::ExprField<'tcx>], seen: &mut Vec<rustc_hir::HirId>) -> Option<u64> {
        let start = fields.iter().find(|field| field.ident.name.as_str() == "start")?.expr;
        let end = fields.iter().find(|field| field.ident.name.as_str() == "end")?.expr;
        if let (Some(start), Some(end)) = (self.constant_count(start, &mut seen.clone()), self.constant_count(end, &mut seen.clone())) {
            return end.checked_sub(start);
        }
        let ExprKind::Binary(operator, base, width) = end.kind else { return None; };
        if operator.node != rustc_hir::BinOpKind::Add { return None; }
        let key = self.key(start, &mut Vec::new())?;
        if self.key(base, &mut Vec::new()).as_ref() != Some(&key) { return None; }
        self.constant_count(width, seen)
    }

    pub(crate) fn bounded_work(&self, expression: &'tcx Expr<'tcx>) -> bool {
        fn fixed_children<'a>(tcx: rustc_middle::ty::TyCtxt<'a>, value: ty::Ty<'a>) -> bool {
            match value.peel_refs().kind() {
                ty::Str => true,
                ty::Slice(element) | ty::Array(element, _) => types::work(tcx, *element, &mut Vec::new()) == Shape::Fixed,
                ty::Adt(owner, args) if types::standard(tcx, owner.did()) && tcx.item_name(owner.did()).as_str() == "Option" => args.types().all(|value| fixed_children(tcx, value)),
                _ => false,
            }
        }
        self.constant_count(expression, &mut Vec::new()).is_some()
            && fixed_children(self.tcx, self.expr_ty(expression))
    }

    pub(crate) fn vector_collection_reuse(
        &self,
        expression: &'tcx Expr<'tcx>,
        source: &'tcx Expr<'tcx>,
    ) -> Option<Shape> {
        let output = self.expr_ty(expression);
        let ty::Adt(owner, _) = output.kind() else {
            return None;
        };
        if !types::standard(self.tcx, owner.did())
            || self.tcx.item_name(owner.did()).as_str() != "Vec"
        {
            return None;
        }
        if self.expr_ty(source) == output {
            return Some(Shape::Fixed);
        }
        if self.call(source).is_some_and(|(definition, args)| {
            types::standard(self.tcx, definition)
                && self.tcx.item_name(definition).as_str() == "into_iter"
                && args
                    .first()
                    .is_some_and(|value| self.expr_ty(value) == output)
        }) {
            return Some(Shape::Fixed);
        }
        if self.owns_vector_iterator(self.expr_ty(source)) {
            Some(Shape::Unknown)
        } else {
            None
        }
    }

    fn owns_vector_iterator(&self, value: ty::Ty<'tcx>) -> bool {
        let ty::Adt(owner, arguments) = value.kind() else {
            return false;
        };
        if !types::standard(self.tcx, owner.did()) {
            return false;
        }
        let name = self.tcx.item_name(owner.did());
        if name.as_str() == "IntoIter" && self.tcx.def_path_str(owner.did()).contains("vec::") {
            return true;
        }
        matches!(
            name.as_str(),
            "Map"
                | "Filter"
                | "FilterMap"
                | "Enumerate"
                | "Rev"
                | "Cloned"
                | "Copied"
                | "Inspect"
                | "Take"
                | "Skip"
                | "TakeWhile"
                | "SkipWhile"
                | "StepBy"
                | "Peekable"
                | "Fuse"
                | "Flatten"
                | "FlatMap"
                | "Chain"
                | "Zip"
        ) && arguments
            .types()
            .any(|inner| self.owns_vector_iterator(inner))
    }

    pub(crate) fn initializer(&self, expression: &Expr<'tcx>) -> Option<&'tcx Expr<'tcx>> {
        if let ExprKind::Field(base, field) = expression.kind {
            let index = field.name.as_str().parse::<usize>().ok()?;
            let tuple = self.initializer(base)?;
            if let ExprKind::Tup(values) = tuple.kind {
                return values.get(index);
            }
            return None;
        }
        let ExprKind::Path(ref path) = expression.kind else {
            return None;
        };
        let Res::Local(id) = self.typeck.qpath_res(path, expression.hir_id) else {
            return None;
        };
        if self.flow.mutated.contains(&format!("local:{id:?}")) {
            return None;
        }
        for (_, node) in self.tcx.hir_parent_iter(id) {
            match node {
                Node::LetStmt(local) => return local.init,
                Node::Expr(expression) if matches!(expression.kind, ExprKind::Let(_)) => {
                    if let ExprKind::Let(local) = expression.kind {
                        return Some(local.init);
                    }
                }
                Node::Param(_) | Node::Item(_) | Node::Expr(_) => return None,
                _ => (),
            }
        }
        None
    }

    pub(crate) fn constant(
        &self,
        expression: &'tcx Expr<'tcx>,
        seen: &mut Vec<rustc_hir::HirId>,
    ) -> bool {
        if seen.contains(&expression.hir_id) {
            return false;
        }
        seen.push(expression.hir_id);
        match expression.kind {
            ExprKind::Lit(_) => true,
            ExprKind::AddrOf(_, _, inner)
            | ExprKind::Unary(_, inner)
            | ExprKind::Cast(inner, _)
            | ExprKind::DropTemps(inner) => self.constant(inner, seen),
            ExprKind::Binary(_, left, right) => {
                self.constant(left, seen) && self.constant(right, seen)
            }
            ExprKind::Array(values) | ExprKind::Tup(values) => {
                values.iter().all(|value| self.constant(value, seen))
            }
            ExprKind::Repeat(value, _) => self.constant(value, seen),
            ExprKind::Block(block, _) => block.expr.is_some_and(|value| self.constant(value, seen)),
            ExprKind::If(_, yes, Some(no)) => {
                self.constant(yes, &mut seen.clone()) && self.constant(no, &mut seen.clone())
            }
            ExprKind::Call(_, _) | ExprKind::MethodCall(_, _, _, _) => {
                let Some((definition, operands)) = self.call(expression) else {
                    return false;
                };
                if matches!(
                    self.tcx.def_kind(definition),
                    rustc_hir::def::DefKind::Ctor(_, _)
                ) {
                    return operands.iter().all(|operand| self.constant(operand, seen));
                }
                if !types::standard(self.tcx, definition) {
                    return false;
                }
                match self.tcx.item_name(definition).as_str() {
                    "len" => operands.first().is_some_and(|operand| {
                        matches!(self.expr_ty(operand).peel_refs().kind(), ty::Array(_, _))
                            || self.constant(operand, seen)
                    }),
                    "size_of" | "align_of" => true,
                    "must_use" | "identity" => operands
                        .iter()
                        .all(|operand| self.constant(operand, &mut seen.clone())),
                    "format" => self.format_shape(&operands) == Shape::Fixed,
                    "from" | "into" | "to_owned" | "to_string" | "clone" | "as_str"
                    | "as_bytes" | "trim" | "trim_start" | "trim_end" | "to_ascii_lowercase"
                    | "to_ascii_uppercase" | "to_lowercase" | "to_uppercase" | "replace"
                    | "replacen" | "concat" | "join" | "repeat" => {
                        self.implementation(expression, definition)
                            .is_some_and(|id| types::standard(self.tcx, id))
                            && !operands.is_empty()
                            && operands
                                .iter()
                                .all(|operand| self.constant(operand, &mut seen.clone()))
                    }
                    "new" => operands.is_empty(),
                    "default" => {
                        operands.is_empty()
                            && self
                                .implementation(expression, definition)
                                .is_some_and(|id| types::standard(self.tcx, id))
                    }
                    _ => false,
                }
            }
            ExprKind::Field(_, _) => self
                .initializer(expression)
                .is_some_and(|init| self.constant(init, seen)),
            ExprKind::Path(ref path) => match self.typeck.qpath_res(path, expression.hir_id) {
                Res::Def(rustc_hir::def::DefKind::Ctor(_, _), definition)
                    if types::standard(self.tcx, definition)
                        && self.tcx.item_name(self.tcx.parent(definition)).as_str() == "None" =>
                {
                    true
                }
                Res::Def(
                    rustc_hir::def::DefKind::Const { .. }
                    | rustc_hir::def::DefKind::AssocConst { .. },
                    _,
                ) => true,
                Res::Local(id)
                    if self.fixed_parameters.contains(&id)
                        && !self.flow.mutated.contains(&format!("local:{id:?}")) =>
                {
                    true
                }
                _ => self
                    .initializer(expression)
                    .is_some_and(|init| self.constant(init, seen)),
            },
            _ => false,
        }
    }

    pub(crate) fn iteration(
        &self,
        expression: &'tcx Expr<'tcx>,
        seen: &mut Vec<rustc_hir::HirId>,
    ) -> Shape {
        if seen.contains(&expression.hir_id) {
            return Shape::Unknown;
        }
        seen.push(expression.hir_id);
        let value = self.expr_ty(expression).peel_refs();
        let constant_container = match value.kind() {
            ty::Str | ty::Slice(_) | ty::Array(_, _) => true,
            ty::Adt(owner, _) => {
                types::standard(self.tcx, owner.did())
                    && matches!(
                        self.tcx.item_name(owner.did()).as_str(),
                        "Vec"
                            | "VecDeque"
                            | "HashMap"
                            | "HashSet"
                            | "BTreeMap"
                            | "BTreeSet"
                            | "Option"
                            | "Result"
                            | "Range"
                            | "RangeInclusive"
                    )
            }
            _ => false,
        };
        if self.bounded_work(expression) || constant_container && self.constant(expression, &mut Vec::new()) {
            return Shape::Fixed;
        }

        match expression.kind {
            ExprKind::AddrOf(_, _, inner) | ExprKind::DropTemps(inner) => {
                return self.iteration(inner, seen)
            }
            ExprKind::Array(_) | ExprKind::Repeat(_, _) => return Shape::Fixed,
            ExprKind::Struct(_, fields, _) => {
                if let ty::Adt(definition, _) = self.expr_ty(expression).peel_refs().kind() {
                    if types::standard(self.tcx, definition.did())
                        && self
                            .tcx
                            .item_name(definition.did())
                            .as_str()
                            .starts_with("Range")
                    {
                        return if fields
                            .iter()
                            .all(|field| self.constant(field.expr, &mut Vec::new()))
                        {
                            Shape::Fixed
                        } else {
                            Shape::Dynamic
                        };
                    }
                }
            }
            _ => (),
        }
        if let Some((definition, operands)) = self.call(expression) {
            let name = self.tcx.item_name(definition);
            if name.as_str() == "new" && types::standard(self.tcx, definition) {
                if let ty::Adt(result, _) = self.expr_ty(expression).kind() {
                    if self.tcx.item_name(result.did()).as_str() == "RangeInclusive" {
                        return if operands
                            .iter()
                            .all(|operand| self.constant(operand, &mut Vec::new()))
                        {
                            Shape::Fixed
                        } else {
                            Shape::Dynamic
                        };
                    }
                }
            }
            if types::standard(self.tcx, definition)
                && matches!(
                    name.as_str(),
                    "iter"
                        | "iter_mut"
                        | "into_iter"
                        | "enumerate"
                        | "rev"
                        | "copied"
                        | "cloned"
                        | "map"
                        | "filter"
                        | "filter_map"
                        | "take"
                        | "skip"
                        | "step_by"
                        | "take_while"
                        | "skip_while"
                        | "inspect"
                        | "chain"
                        | "zip"
                        | "flatten"
                        | "flat_map"
                        | "windows"
                        | "chunks"
                        | "chunks_exact"
                        | "get"
                        | "split_at"
                )
            {
                let shape = operands
                    .first()
                    .map_or(Shape::Unknown, |operand| self.iteration(operand, seen));
                if matches!(name.as_str(), "flatten" | "flat_map") {
                    return if shape == Shape::Fixed {
                        Shape::Unknown
                    } else {
                        shape
                    };
                }
                if matches!(name.as_str(), "chain" | "zip") {
                    return shape.join(
                        operands
                            .get(1)
                            .map_or(Shape::Unknown, |operand| self.iteration(operand, seen)),
                    );
                }
                return shape;
            }
        }
        let value = self.expr_ty(expression).peel_refs();
        if matches!(value.kind(), ty::Array(_, _)) {
            return Shape::Fixed;
        }
        if let Some(init) = self.initializer(expression) {
            return self.iteration(init, seen);
        }
        types::iteration(self.tcx, value)
    }
}
