// SPDX-License-Identifier: Apache-2.0
use crate::{types, Analysis};
use rustc_hir::{def::Res, Expr, ExprKind, HirId, MatchSource, Node};
use rustc_middle::ty;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExtentTerm {
    pub(crate) factors: Vec<String>,
    pub(crate) coefficient: u64,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Credit {
    pub(crate) extents: Vec<ExtentTerm>,
    pub(crate) opaque: bool,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ScopedStorage {
    pub(crate) guard: String,
    pub(crate) scope: rustc_span::Span,
    pub(crate) terms: Vec<ExtentTerm>,
}

#[derive(Clone)]
pub(crate) struct Flow<'tcx> {
    pub(crate) work: Vec<Credit>,
    pub(crate) iterations: u64,
    pub(crate) storage: bool,
    pub(crate) storage_parameters: std::collections::HashSet<String>,
    pub(crate) storage_extents: Vec<ExtentTerm>,
    pub(crate) scoped_storage: Vec<ScopedStorage>,
    pub(crate) storage_slots: Vec<crate::storage::Slots>,
    pub(crate) parser_receipts: Vec<crate::parser::ParserReceipt<'tcx>>,
    pub(crate) loop_bounds: Vec<Vec<ExtentTerm>>,
    pub(crate) mutated: std::collections::HashSet<String>,
}

impl Default for Flow<'_> {
    fn default() -> Self {
        Self {
            work: Vec::new(),
            storage: false,
            storage_parameters: std::collections::HashSet::new(),
            storage_extents: Vec::new(),
            scoped_storage: Vec::new(),
            storage_slots: Vec::new(),
            parser_receipts: Vec::new(),
            loop_bounds: Vec::new(),
            iterations: 1,
            mutated: std::collections::HashSet::new(),
        }
    }
}

impl<'tcx> Analysis<'_, 'tcx> {
    fn owns_field(&self, value: ty::Ty<'tcx>, name: rustc_span::Symbol) -> bool {
        match value.kind() {
            ty::Ref(_, pointee, _) => self.owns_field(*pointee, name),
            ty::Adt(owner, arguments)
                if types::standard(self.tcx, owner.did())
                    && self.tcx.item_name(owner.did()).as_str() == "Box" =>
            {
                arguments
                    .types()
                    .next()
                    .is_some_and(|pointee| self.owns_field(pointee, name))
            }
            ty::Adt(owner, _) => owner
                .variants()
                .iter()
                .any(|variant| variant.fields.iter().any(|field| field.name == name)),
            ty::Tuple(fields) => name
                .as_str()
                .parse::<usize>()
                .ok()
                .is_some_and(|index| index < fields.len()),
            _ => false,
        }
    }

    fn standard_sequence(&self, value: ty::Ty<'tcx>) -> bool {
        match value.peel_refs().kind() {
            ty::Array(..) | ty::Slice(_) => true,
            ty::Adt(owner, arguments) if types::standard(self.tcx, owner.did()) => {
                match self.tcx.item_name(owner.did()).as_str() {
                    "Vec" => true,
                    "Box" => arguments
                        .types()
                        .next()
                        .is_some_and(|pointee| self.standard_sequence(pointee)),
                    _ => false,
                }
            }
            _ => false,
        }
    }

    fn stable_index_key(&self, expression: &'tcx Expr<'tcx>) -> Option<String> {
        match expression.kind {
            ExprKind::AddrOf(_, _, inner) | ExprKind::DropTemps(inner) => {
                self.stable_index_key(inner)
            }
            ExprKind::Lit(literal) if matches!(literal.node, rustc_ast::LitKind::Int(_, _)) => self
                .constant_count(expression, &mut Vec::new())
                .map(|value| format!("constant:{value}")),
            ExprKind::Path(ref path) => match self.typeck.qpath_res(path, expression.hir_id) {
                Res::Local(id) => Some(format!("local:{id:?}")),
                Res::Def(_, id) => Some(format!("definition:{id:?}")),
                _ => None,
            },
            ExprKind::Field(base, field) => {
                if self.owns_field(self.expr_ty(base), field.name) {
                    self.stable_index_key(base)
                        .map(|key| format!("{key}.{}", field.name))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn standard_place_key(&self, expression: &'tcx Expr<'tcx>) -> Option<String> {
        match expression.kind {
            ExprKind::AddrOf(_, _, inner) | ExprKind::DropTemps(inner) => {
                self.standard_place_key(inner)
            }
            ExprKind::Unary(rustc_hir::UnOp::Deref, inner)
                if matches!(self.expr_ty(inner).kind(), ty::Ref(..))
                    || matches!(self.expr_ty(inner).peel_refs().kind(), ty::Adt(owner, _) if types::standard(self.tcx, owner.did()) && self.tcx.item_name(owner.did()).as_str() == "Box") =>
            {
                self.standard_place_key(inner)
            }
            ExprKind::Path(ref path) => match self.typeck.qpath_res(path, expression.hir_id) {
                Res::Local(id) => Some(format!("local:{id:?}")),
                Res::Def(_, id) => Some(format!("definition:{id:?}")),
                _ => None,
            },
            ExprKind::Field(base, field) => {
                if self.owns_field(self.expr_ty(base), field.name) {
                    self.standard_place_key(base)
                        .map(|key| format!("{key}.{}", field.name))
                } else {
                    None
                }
            }
            ExprKind::Index(base, index, _) => {
                if !self.standard_sequence(self.expr_ty(base)) {
                    return None;
                }
                let base = self.standard_place_key(base)?;
                let index = self.stable_index_key(index)?;
                Some(format!(
                    "indexed-element:{}:{base}:{}:{index}",
                    base.len(),
                    index.len()
                ))
            }
            _ => None,
        }
    }

    fn stable_mutation_place(&self, expression: &'tcx Expr<'tcx>) -> bool {
        match expression.kind {
            ExprKind::AddrOf(_, _, inner) | ExprKind::DropTemps(inner) => {
                self.stable_mutation_place(inner)
            }
            ExprKind::Unary(rustc_hir::UnOp::Deref, _)
            | ExprKind::Field(_, _)
            | ExprKind::Index(_, _, _) => self.standard_place_key(expression).is_some(),
            _ => true,
        }
    }

    fn char_result(&self, value: ty::Ty<'tcx>) -> bool {
        match value.peel_refs().kind() {
            ty::Char => true,
            ty::Adt(owner, arguments)
                if types::standard(self.tcx, owner.did())
                    && matches!(
                        self.tcx.item_name(owner.did()).as_str(),
                        "Option" | "Result"
                    ) =>
            {
                arguments
                    .types()
                    .next()
                    .is_some_and(|element| matches!(element.kind(), ty::Char))
            }
            _ => false,
        }
    }

    fn char_key(&self, expression: &'tcx Expr<'tcx>, seen: &mut Vec<HirId>) -> Option<String> {
        if seen.contains(&expression.hir_id) {
            return None;
        }
        seen.push(expression.hir_id);
        match expression.kind {
            ExprKind::AddrOf(_, _, inner) | ExprKind::DropTemps(inner) => {
                return self.char_key(inner, seen)
            }
            ExprKind::Unary(rustc_hir::UnOp::Deref, inner)
                if matches!(self.expr_ty(inner).kind(), ty::Ref(..)) =>
            {
                return self.char_key(inner, seen)
            }
            ExprKind::Lit(literal) => {
                if let rustc_ast::LitKind::Char(value) = literal.node {
                    return Some(format!("char:{:X}", u32::from(value)));
                }
            }
            ExprKind::Path(ref path) => {
                if let Res::Local(id) = self.typeck.qpath_res(path, expression.hir_id) {
                    return self
                        .initializer(expression)
                        .and_then(|initializer| self.char_key(initializer, seen))
                        .or_else(|| Some(format!("local:{id:?}")));
                }
            }
            ExprKind::Index(_, _, _) => {
                return self.standard_place_key(expression);
            }
            ExprKind::Field(base, field) => {
                if let Some(initializer) = self.initializer(base) {
                    if let ExprKind::Struct(_, fields, _) = initializer.kind {
                        if let Some(value) =
                            fields.iter().find(|value| value.ident.name == field.name)
                        {
                            return self.char_key(value.expr, seen);
                        }
                    }
                }
                return self
                    .standard_place_key(expression)
                    .or_else(|| Some(format!("char-expression:{:?}", expression.hir_id)));
            }
            ExprKind::Match(scrutinee, _, MatchSource::TryDesugar(_)) => {
                if let Some((_, operands)) = self.call(scrutinee) {
                    if let Some(value) = operands.first() {
                        return self.char_key(value, seen);
                    }
                }
                return Some(format!("char-expression:{:?}", expression.hir_id));
            }
            _ => (),
        }
        if let Some((definition, operands)) = self.call(expression) {
            let name = self.tcx.item_name(definition);
            if types::standard(self.tcx, definition)
                && matches!(
                    name.as_str(),
                    "unwrap" | "expect" | "map_err" | "ok_or" | "ok_or_else"
                )
                && operands
                    .first()
                    .is_some_and(|operand| self.char_result(self.expr_ty(operand)))
            {
                return operands
                    .first()
                    .and_then(|value| self.char_key(value, seen));
            }
            if self.char_result(self.expr_ty(expression))
                && matches!(
                    name.as_str(),
                    "from_u32" | "try_from" | "try_into" | "from" | "into"
                )
            {
                return Some(format!("char-conversion:{:?}", expression.hir_id));
            }
        }
        if self.char_result(self.expr_ty(expression)) {
            return self
                .standard_place_key(expression)
                .or_else(|| Some(format!("char-expression:{:?}", expression.hir_id)));
        }
        None
    }

    pub(crate) fn utf8_char_term(&self, expression: &'tcx Expr<'tcx>) -> Option<ExtentTerm> {
        let key = self.char_key(expression, &mut Vec::new())?;
        Some(ExtentTerm {
            factors: vec![format!("utf8_len:{key}")],
            coefficient: 1,
        })
    }

    fn range_key(&self, range: &'tcx Expr<'tcx>, seen: &mut Vec<HirId>) -> Option<String> {
        let range = self.initializer(range).unwrap_or(range);
        let ExprKind::Struct(_, fields, _) = range.kind else {
            return self.key(range, seen);
        };
        let ty::Adt(owner, _) = self.expr_ty(range).kind() else {
            return None;
        };
        if !types::standard(self.tcx, owner.did()) {
            return None;
        }
        let name = self.tcx.item_name(owner.did());
        if !matches!(
            name.as_str(),
            "Range" | "RangeTo" | "RangeFrom" | "RangeInclusive" | "RangeToInclusive"
        ) {
            return None;
        }
        let mut result = name.as_str().to_owned();
        for field in fields {
            let key = self
                .extent_terms(field.expr, &mut Vec::new())
                .map(|terms| format!("{terms:?}"))
                .or_else(|| self.key(field.expr, &mut seen.clone()))?;
            result.push_str(&format!(":{}={key}", field.ident.name));
        }
        Some(result)
    }

    pub(crate) fn key(
        &self,
        expression: &'tcx Expr<'tcx>,
        seen: &mut Vec<HirId>,
    ) -> Option<String> {
        if seen.contains(&expression.hir_id) {
            return None;
        }
        seen.push(expression.hir_id);
        match expression.kind {
            ExprKind::AddrOf(_, _, inner)
            | ExprKind::DropTemps(inner)
            | ExprKind::Unary(_, inner) => self.key(inner, seen),
            ExprKind::Path(ref path) => match self.typeck.qpath_res(path, expression.hir_id) {
                Res::Local(id) => self
                    .initializer(expression)
                    .and_then(|init| self.key(init, seen))
                    .or_else(|| Some(format!("local:{id:?}"))),
                Res::Def(_, id) => Some(format!("definition:{id:?}")),
                _ => None,
            },
            ExprKind::Struct(_, fields, _) => {
                if let rustc_middle::ty::Adt(definition, _) = self.expr_ty(expression).kind() {
                    if types::standard(self.tcx, definition.did())
                        && self.tcx.item_name(definition.did()).as_str() == "Range"
                    {
                        let start = fields
                            .iter()
                            .find(|field| field.ident.name.as_str() == "start")?
                            .expr;
                        let end = fields
                            .iter()
                            .find(|field| field.ident.name.as_str() == "end")?
                            .expr;
                        if matches!(start.kind, ExprKind::Lit(literal) if matches!(literal.node, rustc_ast::LitKind::Int(value, _) if value.get() == 0))
                        {
                            let terms = self.extent_terms(end, &mut Vec::new())?;
                            if let [term] = terms.as_slice() {
                                if term.coefficient == 1 {
                                    if let [factor] = term.factors.as_slice() {
                                        return Some(factor.clone());
                                    }
                                }
                            }
                        }
                    }
                }
                None
            }
            ExprKind::Field(base, field) => {
                let split = self.initializer(base).unwrap_or(base);
                if self.call(split).is_some_and(|(id, _)| {
                    types::standard(self.tcx, id) && self.tcx.item_name(id).as_str() == "split_at"
                }) {
                    self.key(split, seen)
                } else {
                    self.key(base, seen)
                        .map(|key| format!("{key}.{}", field.name))
                }
            }
            ExprKind::Index(base, index, _) => {
                let value = self.expr_ty(index).peel_refs();
                if matches!(value.kind(), rustc_middle::ty::Adt(owner, _) if types::standard(self.tcx, owner.did()) && self.tcx.item_name(owner.did()).as_str().starts_with("Range"))
                {
                    let base = self.key(base, &mut seen.clone())?;
                    if matches!(value.kind(), ty::Adt(owner, _) if self.tcx.item_name(owner.did()).as_str() == "RangeFull")
                    {
                        return Some(base);
                    }
                    self.range_key(index, seen)
                        .map(|range| format!("{base}.range[{range}]"))
                } else {
                    self.key(base, seen).map(|key| format!("{key}.window"))
                }
            }
            _ => {
                let (definition, operands) = self.call(expression)?;
                let name = self.tcx.item_name(definition);
                if operands.first().is_some_and(|source| {
                    types::standard_str_chars_call(
                        self.tcx,
                        definition,
                        self.expr_ty(source),
                        self.expr_ty(expression),
                    )
                }) {
                    return operands.first().and_then(|source| self.key(source, seen));
                }
                if types::standard(self.tcx, definition) && matches!(name.as_str(), "unwrap" | "expect")
                    && operands.first().is_some_and(|operand| matches!(self.expr_ty(operand).peel_refs().kind(), ty::Adt(owner, _) if types::standard(self.tcx, owner.did()) && self.tcx.item_name(owner.did()).as_str() == "Option"))
                    && matches!(self.expr_ty(expression).peel_refs().kind(), ty::Slice(_) | ty::Str)
                {
                    return operands.first().and_then(|operand| self.key(operand, seen));
                }
                if types::standard(self.tcx, definition) && name.as_str() == "get" {
                    let range = operands.get(1).is_some_and(|operand|
                        matches!(self.expr_ty(operand).kind(), rustc_middle::ty::Adt(owner, _)
                            if types::standard(self.tcx, owner.did()) && self.tcx.item_name(owner.did()).as_str().starts_with("Range")));
                    if range {
                        let base = operands
                            .first()
                            .and_then(|operand| self.key(operand, &mut seen.clone()))?;
                        let range = operands.get(1)?;
                        if matches!(self.expr_ty(range).kind(), ty::Adt(owner, _) if self.tcx.item_name(owner.did()).as_str() == "RangeFull")
                        {
                            return Some(base);
                        }
                        return self
                            .range_key(range, seen)
                            .map(|range| format!("{base}.range[{range}]"));
                    }
                    return None;
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
                            | "take"
                            | "skip"
                            | "step_by"
                            | "filter_map"
                            | "windows"
                            | "chunks"
                            | "chunks_exact"
                            | "split_at"
                    )
                    || (types::standard(self.tcx, definition)
                        || self.tcx.crate_name(definition.krate).as_str() == "cadmpeg_core")
                        && matches!(name.as_str(), "window" | "as_bytes" | "as_slice")
                {
                    operands.first().and_then(|operand| self.key(operand, seen))
                } else {
                    None
                }
            }
        }
    }

    pub(crate) fn extent_terms(
        &self,
        expression: &'tcx Expr<'tcx>,
        seen: &mut Vec<HirId>,
    ) -> Option<Vec<ExtentTerm>> {
        if seen.contains(&expression.hir_id) {
            return None;
        }
        seen.push(expression.hir_id);
        if let Some((definition, _)) = self.call(expression) {
            if types::standard(self.tcx, definition)
                && self.tcx.item_name(definition).as_str() == "size_of"
            {
                let arguments = self.call_arguments(expression)?;
                let element = arguments.types().next()?;
                return Some(vec![ExtentTerm {
                    factors: vec![format!("size:{element}")],
                    coefficient: 1,
                }]);
            }
        }
        if let Some((definition, operands)) = self.call(expression) {
            if types::standard(self.tcx, definition)
                && self.tcx.item_name(definition).as_str() == "len_utf8"
                && operands
                    .first()
                    .is_some_and(|value| matches!(self.expr_ty(value).peel_refs().kind(), ty::Char))
            {
                return self
                    .utf8_char_term(operands.first()?)
                    .map(|term| vec![term]);
            }
        }
        if let Some(count) = self.constant_count(expression, &mut Vec::new()) {
            return Some(vec![ExtentTerm {
                factors: Vec::new(),
                coefficient: count,
            }]);
        }
        match expression.kind {
            ExprKind::AddrOf(_, _, inner) | ExprKind::DropTemps(inner) => {
                return self.extent_terms(inner, seen)
            }
            ExprKind::Binary(operator, left, right)
                if operator.node == rustc_hir::BinOpKind::Sub =>
            {
                let left = self.extent_terms(left, &mut seen.clone())?;
                let right = self.extent_terms(right, &mut seen.clone())?;
                return Some(vec![ExtentTerm {
                    factors: vec![format!("difference:{left:?}:{right:?}")],
                    coefficient: 1,
                }]);
            }
            ExprKind::Cast(inner, _) => {
                let source = self.expr_ty(inner);
                let destination = self.expr_ty(expression);
                if source == destination
                    || matches!((source.kind(), destination.kind()), (rustc_middle::ty::Uint(left), rustc_middle::ty::Uint(right)) if left.bit_width().or(Some(self.tcx.data_layout.pointer_size().bits())).zip(right.bit_width().or(Some(self.tcx.data_layout.pointer_size().bits()))).is_some_and(|(left, right)| left <= right))
                {
                    return self.extent_terms(inner, seen);
                }
                return None;
            }
            ExprKind::Match(scrutinee, _, MatchSource::TryDesugar(_)) => {
                let (_, args) = self.call(scrutinee)?;
                return args.first().and_then(|arg| self.extent_terms(arg, seen));
            }
            _ => (),
        }
        if let Some((definition, operands)) = self.call(expression) {
            let name = self.tcx.item_name(definition);
            if matches!(name.as_str(), "len" | "capacity") && types::standard(self.tcx, definition)
                || name.as_str() == "len" && self.tcx.def_path_str(definition).contains("View")
            {
                return operands
                    .first()
                    .and_then(|operand| self.key(operand, &mut Vec::new()))
                    .map(|key| {
                        vec![ExtentTerm {
                            factors: vec![if name.as_str() == "capacity" {
                                format!("{key}.capacity")
                            } else {
                                key
                            }],
                            coefficient: 1,
                        }]
                    });
            }
            if types::physical_item_path(
                self.tcx,
                definition,
                "cadmpeg_core",
                &["decode", "view", "u64_from_index"],
            )
                || (types::standard(self.tcx, definition) && matches!(name.as_str(), "ok_or" | "ok_or_else" | "map_err" | "try_from" | "try_into"))
                || (types::standard(self.tcx, definition) && name.as_str() == "from" && operands.first().is_some_and(|operand| matches!((self.expr_ty(operand).kind(), self.expr_ty(expression).kind()), (rustc_middle::ty::Uint(left), rustc_middle::ty::Uint(right)) if left.bit_width().or(Some(self.tcx.data_layout.pointer_size().bits())).zip(right.bit_width().or(Some(self.tcx.data_layout.pointer_size().bits()))).is_some_and(|(left, right)| left <= right)))) {
                return operands
                    .first()
                    .and_then(|operand| self.extent_terms(operand, seen));
            }
            let arithmetic = if types::standard(self.tcx, definition) {
                match name.as_str() {
                    "checked_add" => Some((false, 0)),
                    "checked_mul" => Some((true, 0)),
                    _ => None,
                }
            } else if self.trusted_context_callee(expression) && self.context_operation(expression)
            {
                match name.as_str() {
                    "cost_sum" => Some((false, 1)),
                    "cost_product" => Some((true, 1)),
                    _ => None,
                }
            } else {
                None
            };
            if let Some((false, offset)) = arithmetic {
                let mut terms = self.extent_terms(operands.get(offset)?, &mut seen.clone())?;
                terms.extend(self.extent_terms(operands.get(offset + 1)?, &mut seen.clone())?);
                return Some(terms);
            }
            if let Some((true, offset)) = arithmetic {
                let left = self.extent_terms(operands.get(offset)?, &mut seen.clone())?;
                let right = self.extent_terms(operands.get(offset + 1)?, &mut seen.clone())?;
                let mut product = Vec::new();
                for left in left {
                    for right in &right {
                        let mut factors = left.factors.clone();
                        factors.extend(right.factors.iter().cloned());
                        factors.sort();
                        product.push(ExtentTerm {
                            factors,
                            coefficient: left.coefficient.checked_mul(right.coefficient)?,
                        });
                    }
                }
                return Some(product);
            }
        }
        if let Some(init) = self.initializer(expression) {
            if let Some(terms) = self.extent_terms(init, seen) {
                return Some(terms);
            }
            if !matches!(init.kind, ExprKind::If(..) | ExprKind::Match(..)) {
                return None;
            }
        }
        self.key(expression, &mut Vec::new()).map(|key| {
            vec![ExtentTerm {
                factors: vec![key],
                coefficient: 1,
            }]
        })
    }

    pub(crate) fn propagated(&self, expression: &'tcx Expr<'tcx>) -> bool {
        for (_, node) in self.tcx.hir_parent_iter(expression.hir_id) {
            match node {
                Node::Expr(parent) => match parent.kind {
                    ExprKind::Match(_, _, MatchSource::TryDesugar(_)) => return true,
                    ExprKind::Call(_, _) | ExprKind::MethodCall(_, _, _, _) => {
                        let Some((id, _)) = self.call(parent) else {
                            return false;
                        };
                        // Standard Result::map_err keeps Err on the refusal path;
                        // the surrounding try returns before admitted work.
                        if !types::standard(self.tcx, id)
                            || !matches!(self.tcx.item_name(id).as_str(), "branch" | "map_err")
                        {
                            return false;
                        }
                    }
                    ExprKind::AddrOf(_, _, _) | ExprKind::DropTemps(_) => (),
                    _ => return false,
                },
                _ => return false,
            }
        }
        false
    }

    pub(super) fn context_operand(&self, expression: &'tcx Expr<'tcx>) -> bool {
        if types::has_context(self.tcx, self.expr_ty(expression), &mut Vec::new()) {
            return true;
        }
        match expression.kind {
            ExprKind::Field(base, _) | ExprKind::AddrOf(_, _, base) => self.context_operand(base),
            _ => false,
        }
    }

    pub(crate) fn context_operation(&self, expression: &'tcx Expr<'tcx>) -> bool {
        self.call(expression).is_some_and(|(_, operands)| {
            operands.iter().any(|operand| self.context_operand(operand))
        })
    }

    pub(crate) fn trusted_context_callee(&self, expression: &'tcx Expr<'tcx>) -> bool {
        self.call(expression).is_some_and(|(definition, operands)| {
            self.tcx.crate_name(definition.krate).as_str() == "cadmpeg_core"
                || std::env::var_os("CADMPEG_POLICY_FIXTURE").is_some()
                    && matches!(self.tcx.def_kind(definition), rustc_hir::def::DefKind::AssocFn)
                    && operands.first().is_some_and(|operand| matches!(self.expr_ty(operand).peel_refs().kind(), rustc_middle::ty::Adt(owner, _) if self.tcx.item_name(owner.did()).as_str() == "DecodeContext"))
        })
    }

    pub(crate) fn record_charge(&mut self, expression: &'tcx Expr<'tcx>) {
        let Some((definition, operands)) = self.call(expression) else {
            return;
        };
        let name = self.tcx.item_name(definition);
        if !self.context_operation(expression) || !self.propagated(expression) {
            return;
        }
        if matches!(
            name.as_str(),
            "charge_retained"
                | "charge_retained_limit"
                | "reserve_scoped"
                | "reserve_scoped_limit"
                | "reserve_vec"
                | "reserve_vec_limit"
                | "collection_vec"
                | "vector_storage"
                | "reserve_capacity"
                | "try_reserve_retained_text"
                | "reserve_set"
                | "reserve_map"
                | "reserve_hash_map_storage"
                | "reserve_hash_set_storage"
                | "reserve_scoped_vec"
                | "reserve_scoped_vec_limit"
                | "reserve_temporary_vec"
                | "charge_hash_growth"
                | "admit_btree_entry"
                | "admit_btree_node_storage"
                | "admit_retained_btree_record"
                | "charge_input"
                | "linear_growth"
                | "admit_hash_map_entry"
                | "scoped_vector_storage"
                | "reserve_retained_vec_storage"
        ) {
            self.flow.storage = true;
        }
        if matches!(
            name.as_str(),
            "charge_retained" | "charge_retained_limit" | "reserve_scoped" | "reserve_scoped_limit"
        ) && self.trusted_context_callee(expression)
        {
            if let Some(terms) = operands
                .get(1)
                .and_then(|amount| self.extent_terms(amount, &mut Vec::new()))
            {
                if let Some(terms) = self.scaled_storage_terms(&terms) {
                    if matches!(name.as_str(), "reserve_scoped" | "reserve_scoped_limit") {
                        self.record_scoped_storage(expression, terms);
                    } else {
                        self.flow.storage_extents.extend(terms);
                    }
                }
            }
        }
        self.record_slots(expression);
        self.record_parser_admission(expression);
        self.record_key_work(expression);
        self.record_move_work(expression);
        self.record_order_work(expression);
        if !matches!(name.as_str(), "charge_work" | "charge_work_limit") {
            return;
        }
        let exact_charge = types::decode_context_method(self.tcx, definition, "charge_work")
            || types::decode_context_method(self.tcx, definition, "charge_work_limit")
            || std::env::var_os("CADMPEG_POLICY_FIXTURE").is_some()
                && self.trusted_context_callee(expression);
        if !exact_charge {
            self.flow.work.push(Credit {
                extents: Vec::new(),
                opaque: true,
            });
            return;
        }
        if let Some(amount) = operands.get(1) {
            if matches!(amount.kind, ExprKind::Lit(literal) if matches!(literal.node, rustc_ast::LitKind::Int(value, _) if value.get() == 0))
            {
                return;
            }
            let terms = self
                .extent_terms(amount, &mut Vec::new())
                .and_then(|mut terms| {
                    for term in &mut terms {
                        term.coefficient = term.coefficient.checked_mul(self.flow.iterations)?;
                    }
                    Some(terms)
                });
            self.flow.work.push(Credit {
                opaque: terms.is_none(),
                extents: terms.unwrap_or_default(),
            });
        }
    }

    fn standard_range_source(&self, expression: &'tcx Expr<'tcx>) -> bool {
        match self.expr_ty(expression).peel_refs().kind() {
            ty::Slice(_) | ty::Array(_, _) | ty::Str => true,
            ty::Adt(owner, _) => {
                types::standard(self.tcx, owner.did())
                    && matches!(self.tcx.item_name(owner.did()).as_str(), "Vec" | "String")
            }
            _ => false,
        }
    }

    fn dominated_keys(&self, expression: &'tcx Expr<'tcx>, seen: &mut Vec<HirId>) -> Vec<String> {
        if seen.contains(&expression.hir_id) {
            return Vec::new();
        }
        seen.push(expression.hir_id);
        if let Some(init) = self.initializer(expression) {
            let keys = self.dominated_keys(init, seen);
            if !keys.is_empty() {
                return keys;
            }
        }
        if let ExprKind::AddrOf(_, _, inner) | ExprKind::DropTemps(inner) = expression.kind {
            return self.dominated_keys(inner, seen);
        }
        if let ExprKind::Index(base, range, _) = expression.kind {
            if self.standard_range_source(base)
                && matches!(self.expr_ty(range).peel_refs().kind(), ty::Adt(owner, _) if types::standard(self.tcx, owner.did()) && self.tcx.item_name(owner.did()).as_str().starts_with("Range"))
            {
                let mut keys: Vec<_> = self.key(expression, &mut Vec::new()).into_iter().collect();
                keys.extend(self.dominated_keys(base, seen));
                return keys;
            }
        }
        if let Some((definition, operands)) = self.call(expression) {
            if types::standard(self.tcx, definition) && self.tcx.item_name(definition).as_str() == "get"
                && operands.first().is_some_and(|base| self.standard_range_source(base))
                && operands.get(1).is_some_and(|range| matches!(self.expr_ty(range).peel_refs().kind(), ty::Adt(owner, _) if types::standard(self.tcx, owner.did()) && self.tcx.item_name(owner.did()).as_str().starts_with("Range"))) {
                let mut keys: Vec<_> = self.key(expression, &mut Vec::new()).into_iter().collect();
                keys.extend(operands.first().map_or_else(Vec::new, |base| self.dominated_keys(base, seen)));
                return keys;
            }
            if types::standard(self.tcx, definition)
                && self.tcx.item_name(definition).as_str() == "zip"
            {
                return operands
                    .iter()
                    .flat_map(|operand| self.dominated_keys(operand, &mut seen.clone()))
                    .collect();
            }
        }
        self.key(expression, &mut Vec::new()).into_iter().collect()
    }

    pub(crate) fn take_credit(&mut self, operands: &[&'tcx Expr<'tcx>]) -> Option<bool> {
        let keys: Vec<_> = operands
            .iter()
            .flat_map(|operand| self.dominated_keys(operand, &mut Vec::new()))
            .collect();
        self.take_credit_for_keys(&keys)
    }

    pub(crate) fn take_credit_for_keys(&mut self, keys: &[String]) -> Option<bool> {
        for credit in &mut self.flow.work {
            if credit.opaque {
                continue;
            }
            if let Some(index) = credit.extents.iter().position(|term| {
                term.coefficient >= self.flow.iterations
                    && term.factors.len() == 1
                    && keys.contains(&term.factors[0])
            }) {
                credit.extents[index].coefficient -= self.flow.iterations;
                credit.extents.retain(|term| term.coefficient != 0);
                self.flow
                    .work
                    .retain(|credit| !credit.extents.is_empty() || credit.opaque);
                return Some(true);
            }
        }
        if self.flow.work.iter().any(|credit| credit.opaque) {
            None
        } else {
            Some(false)
        }
    }

    pub(crate) fn invalidate_target(&mut self, expression: &'tcx Expr<'tcx>) {
        let utf8_factor = matches!(self.expr_ty(expression).peel_refs().kind(), ty::Char)
            .then(|| {
                self.utf8_char_term(expression)
                    .and_then(|term| term.factors.into_iter().next())
            })
            .flatten();
        let stable_place = self.standard_place_key(expression);
        let key = if self.stable_mutation_place(expression) {
            self.key(expression, &mut Vec::new())
        } else {
            None
        };
        if let Some(key) = key {
            let invalidates_factor = |factor: &str| {
                factor_depends_on(factor, &key)
                    || stable_place
                        .as_deref()
                        .is_some_and(|place| factor_depends_on(factor, place))
                    || utf8_factor.as_deref() == Some(factor)
            };
            self.flow
                .parser_receipts
                .retain(|receipt| !factor_depends_on(&receipt.guard, &key));
            self.flow.scoped_storage.retain(|credit| {
                credit.guard != key
                    && !credit
                        .terms
                        .iter()
                        .flat_map(|term| &term.factors)
                        .any(|factor| invalidates_factor(factor))
            });
            self.flow.work.retain(|credit| {
                !credit
                    .extents
                    .iter()
                    .flat_map(|term| &term.factors)
                    .any(|factor| invalidates_factor(factor))
            });
            self.flow.storage_parameters.retain(|parameter| {
                parameter != &key
                    && !parameter.starts_with(&format!("{key}."))
                    && stable_place.as_ref().is_none_or(|place| {
                        parameter != place && !parameter.starts_with(&format!("{place}."))
                    })
            });
            self.flow.storage_extents.retain(|term| {
                !term.factors.iter().any(|factor| {
                    factor.contains(&key)
                        || factor.starts_with(&format!("{key}."))
                        || key.starts_with(&format!("{factor}."))
                        || utf8_factor
                            .as_deref()
                            .is_some_and(|utf8| factor.as_str() == utf8)
                        || stable_place
                            .as_deref()
                            .is_some_and(|place| factor_depends_on(factor, place))
                })
            });
            self.flow.storage_slots.retain(|credit| {
                credit.target != key
                    && stable_place
                        .as_ref()
                        .is_none_or(|place| credit.target.as_str() != place.as_str())
                    && !credit
                        .scope
                        .as_ref()
                        .is_some_and(|scope| scope.guard == key)
                    && !credit
                        .terms
                        .iter()
                        .flat_map(|term| &term.factors)
                        .any(|factor| invalidates_factor(factor))
                    && !credit.target.starts_with(&format!("{key}."))
                    && !credit
                        .terms
                        .iter()
                        .flat_map(|term| &term.factors)
                        .any(|factor| factor.contains(&key))
            });
            self.flow.mutated.insert(key);
            if let Some(stable_place) = stable_place.as_ref() {
                self.flow.mutated.insert(stable_place.clone());
            }
        } else {
            self.flow.scoped_storage.clear();
            self.flow.storage_parameters.clear();
            self.flow.storage_extents.clear();
            self.flow.storage_slots.clear();
            self.flow.parser_receipts.clear();
            for credit in &mut self.flow.work {
                credit.opaque = true;
            }
        }
        let mut binding = expression;
        while let ExprKind::AddrOf(_, _, inner)
        | ExprKind::DropTemps(inner)
        | ExprKind::Unary(_, inner)
        | ExprKind::Field(inner, _)
        | ExprKind::Index(inner, _, _) = binding.kind
        {
            binding = inner;
        }
        if binding.hir_id != expression.hir_id {
            self.invalidate_target(binding);
        }
        if let ExprKind::Path(ref path) = binding.kind {
            if let Res::Local(id) = self.typeck.qpath_res(path, binding.hir_id) {
                self.flow.mutated.insert(format!("local:{id:?}"));
            }
        }
    }

    pub(crate) fn mutation(&mut self, expression: &'tcx Expr<'tcx>) {
        if let ExprKind::Assign(target, _, _) | ExprKind::AssignOp(_, target, _) = expression.kind {
            self.invalidate_target(target);
        }
        if let Some((definition, operands)) = self.call(expression) {
            if types::standard(self.tcx, definition)
                && self.tcx.item_name(definition).as_str() == "drop"
            {
                if let Some(operand) = operands.first() {
                    self.invalidate_target(operand);
                }
            }
            let reserved = types::standard(self.tcx, definition)
                && matches!(
                    self.tcx.item_name(definition).as_str(),
                    "try_reserve_exact" | "reserve_exact"
                );
            let admitted_growth = self
                .findings
                .admitted_growth_operations
                .contains(&expression.hir_id);
            let slots: Vec<_> = self
                .flow
                .storage_slots
                .iter()
                .filter(|credit| {
                    (credit.usage == crate::storage::SlotUse::Insertion || admitted_growth)
                        && (credit.usage != crate::storage::SlotUse::Reserve
                            || reserved && admitted_growth)
                })
                .cloned()
                .collect();
            for operand in operands {
                if !matches!(
                    self.expr_ty_adjusted(operand).kind(),
                    rustc_middle::ty::Ref(..)
                ) && self.key(operand, &mut Vec::new()).is_some_and(|key| {
                    self.flow
                        .scoped_storage
                        .iter()
                        .any(|credit| credit.guard == key)
                        || self
                            .flow
                            .parser_receipts
                            .iter()
                            .any(|receipt| factor_depends_on(&key, &receipt.guard))
                        || self.flow.storage_slots.iter().any(|credit| {
                            credit
                                .scope
                                .as_ref()
                                .is_some_and(|scope| scope.guard == key)
                        })
                }) {
                    self.invalidate_target(operand);
                }
                if matches!(
                    self.expr_ty_adjusted(operand).kind(),
                    rustc_middle::ty::Ref(_, _, rustc_hir::Mutability::Mut)
                ) {
                    self.invalidate_target(operand);
                }
            }
            if reserved || admitted_growth {
                for credit in slots {
                    if !self.flow.storage_slots.contains(&credit) {
                        self.flow.storage_slots.push(credit);
                    }
                }
            }
        }
    }
}

impl Flow<'_> {
    pub(crate) fn with_parameters(parameters: &std::collections::HashSet<HirId>) -> Self {
        Self {
            storage_parameters: parameters
                .iter()
                .map(|id| format!("local:{id:?}"))
                .collect(),
            ..Self::default()
        }
    }
}

fn take_sized_component(input: &str, separator: bool) -> Option<(&str, &str)> {
    let (length, value) = input.split_once(':')?;
    let length = length.parse::<usize>().ok()?;
    let component = value.get(..length)?;
    let remainder = value.get(length..)?;
    let remainder = if separator {
        remainder.strip_prefix(':')?
    } else {
        remainder
    };
    Some((component, remainder))
}

fn indexed_element_parts(place: &str) -> Option<(&str, &str)> {
    let encoded = place.strip_prefix("indexed-element:")?;
    let (base, encoded) = take_sized_component(encoded, true)?;
    let (index, _) = take_sized_component(encoded, false)?;
    Some((base, index))
}

fn place_depends_on(place: &str, key: &str) -> bool {
    place == key
        || place.starts_with(&format!("{key}."))
        || key.starts_with(&format!("{place}."))
        || indexed_element_parts(place).is_some_and(|(base, index)| {
            place_depends_on(base, key) || place_depends_on(index, key)
        })
}

pub(crate) fn factor_depends_on(factor: &str, key: &str) -> bool {
    factor == key
        || factor
            .strip_prefix("utf8_len:")
            .is_some_and(|place| place_depends_on(place, key))
        || factor.contains(".range[") && factor.contains(key)
        || matches!(
            factor.split_once(':').map(|(kind, _)| kind),
            Some(
                "keybytes"
                    | "treekeybytes"
                    | "sortbytes"
                    | "movebytes"
                    | "heappushbytes"
                    | "heappopbytes"
            )
        ) && factor.contains(key)
        || factor.starts_with(&format!("{key}."))
        || key.starts_with(&format!("{factor}."))
}
