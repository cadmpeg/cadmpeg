// SPDX-License-Identifier: Apache-2.0
//! Single-use work receipts for incremental encoding_rs decoder loops.
use crate::{flow::Flow, types, Analysis, Findings};
use rustc_hir::intravisit::{walk_expr, Visitor};
use rustc_hir::{
    def::Res,
    Block, Expr, ExprKind, HirId, LoopSource, Node, Pat, PatKind,
};
use rustc_middle::ty::{self, Ty};
use rustc_span::def_id::DefId;
use std::collections::HashSet;

#[derive(Clone, Copy)]
struct DecoderStep {
    decoder: HirId,
    result: HirId,
    consumed: HirId,
    read: HirId,
    decoder_use: HirId,
    read_use: HirId,
}

struct SourceLineage<'tcx> {
    binding: HirId,
    root: &'tcx Expr<'tcx>,
    paired_encoding: Option<HirId>,
}

struct BindingValue<'tcx> {
    initializer: &'tcx Expr<'tcx>,
    tuple_field: Option<usize>,
    tuple_arity: Option<usize>,
}

struct DecoderCalls<'a, 'b, 'tcx> {
    analysis: &'a Analysis<'b, 'tcx>,
    calls: Vec<&'tcx Expr<'tcx>>,
    nested_loop: bool,
}

impl<'tcx> Visitor<'tcx> for DecoderCalls<'_, '_, 'tcx> {
    fn visit_expr(&mut self, expression: &'tcx Expr<'tcx>) {
        match expression.kind {
            ExprKind::Closure(_) | ExprKind::Loop(..) => {
                self.nested_loop = true;
                return;
            }
            _ => (),
        }
        if self.analysis.call(expression).is_some_and(|(definition, _)| {
            self.analysis.incremental_decode_method(definition)
        }) {
            self.calls.push(expression);
        }
        walk_expr(self, expression);
    }
}

struct ReadWrites<'a, 'b, 'tcx> {
    analysis: &'a Analysis<'b, 'tcx>,
    read: HirId,
    consumed: HirId,
    writes: Vec<(&'tcx Expr<'tcx>, bool)>,
    uses: Vec<HirId>,
}

struct BindingUses<'a, 'b, 'tcx> {
    analysis: &'a Analysis<'b, 'tcx>,
    binding: HirId,
    uses: Vec<HirId>,
}

impl<'tcx> Visitor<'tcx> for BindingUses<'_, '_, 'tcx> {
    fn visit_expr(&mut self, expression: &'tcx Expr<'tcx>) {
        if matches!(expression.kind, ExprKind::Path(_))
            && self.analysis.local_binding(expression) == Some(self.binding)
        {
            self.uses.push(expression.hir_id);
        }
        walk_expr(self, expression);
    }
}

impl<'tcx> Visitor<'tcx> for ReadWrites<'_, '_, 'tcx> {
    fn visit_expr(&mut self, expression: &'tcx Expr<'tcx>) {
        if matches!(expression.kind, ExprKind::Path(_))
            && self.analysis.local_binding(expression) == Some(self.read)
        {
            self.uses.push(expression.hir_id);
        }
        match expression.kind {
            ExprKind::Closure(_) | ExprKind::Loop(..) => return,
            ExprKind::Assign(target, _, _) | ExprKind::AssignOp(_, target, _)
                if self.analysis.local_binding(target) == Some(self.read) =>
            {
                let exact = matches!(expression.kind,
                    ExprKind::AssignOp(operator, target, value)
                        if operator.node == rustc_ast::AssignOpKind::AddAssign
                            && self.analysis.local_binding(target) == Some(self.read)
                            && self.analysis.local_binding(value) == Some(self.consumed));
                self.writes.push((expression, exact));
            }
            _ => (),
        }
        walk_expr(self, expression);
    }
}

struct ResultMatches<'a, 'b, 'tcx> {
    analysis: &'a Analysis<'b, 'tcx>,
    result: HirId,
    matches: Vec<&'tcx Expr<'tcx>>,
}

struct UnstructuredLoopControl {
    result_match: rustc_span::Span,
    found: bool,
}

impl<'tcx> Visitor<'tcx> for UnstructuredLoopControl {
    fn visit_expr(&mut self, expression: &'tcx Expr<'tcx>) {
        match expression.kind {
            ExprKind::Closure(_) | ExprKind::Loop(..) => return,
            ExprKind::Continue(_) => self.found = true,
            ExprKind::Break(..) if !self.result_match.contains(expression.span) => {
                self.found = true;
            }
            _ => (),
        }
        walk_expr(self, expression);
    }
}

impl<'tcx> Visitor<'tcx> for ResultMatches<'_, '_, 'tcx> {
    fn visit_expr(&mut self, expression: &'tcx Expr<'tcx>) {
        match expression.kind {
            ExprKind::Closure(_) | ExprKind::Loop(..) => return,
            ExprKind::Match(scrutinee, _, _)
                if self.analysis.local_binding(scrutinee) == Some(self.result) =>
            {
                self.matches.push(expression);
            }
            _ => (),
        }
        walk_expr(self, expression);
    }
}

struct PriorCharges<'a, 'b, 'tcx> {
    analysis: &'a Analysis<'b, 'tcx>,
    calls: Vec<&'tcx Expr<'tcx>>,
}

impl<'tcx> Visitor<'tcx> for PriorCharges<'_, '_, 'tcx> {
    fn visit_expr(&mut self, expression: &'tcx Expr<'tcx>) {
        match expression.kind {
            ExprKind::Closure(_) | ExprKind::Loop(..) => return,
            _ => (),
        }
        if self.analysis.call(expression).is_some_and(|(definition, _)| {
            self.analysis.core_charge_work_method(definition)
        }) {
            self.calls.push(expression);
        }
        walk_expr(self, expression);
    }
}

impl<'tcx> Analysis<'_, 'tcx> {
    fn encoding_rs_definition(&self, definition: DefId, owner: &str, item: &str) -> bool {
        types::inherent_method_owner(self.tcx, definition, "encoding_rs", owner, item)
    }

    fn encoding_rs_type(&self, value: Ty<'tcx>, name: &str) -> bool {
        matches!(value.peel_refs().kind(), ty::Adt(owner, _)
            if types::physical_item_path(self.tcx, owner.did(), "encoding_rs", &[name]))
    }

    fn incremental_decode_method(&self, definition: DefId) -> bool {
        self.encoding_rs_definition(
            definition,
            "Decoder",
            "decode_to_utf8_without_replacement",
        )
    }

    fn core_charge_work_method(&self, definition: DefId) -> bool {
        types::decode_context_method(self.tcx, definition, "charge_work")
    }

    fn core_u64_from_index(&self, definition: DefId) -> bool {
        types::physical_item_path(
            self.tcx, definition, "cadmpeg_core", &["decode", "view", "u64_from_index"],
        )
    }

    fn local_binding(&self, expression: &'tcx Expr<'tcx>) -> Option<HirId> {
        let expression = strip_temporary(expression);
        let ExprKind::Path(path) = expression.kind else {
            return None;
        };
        match self.typeck.qpath_res(&path, expression.hir_id) {
            Res::Local(binding) => Some(binding),
            _ => None,
        }
    }

    fn binding_value(&self, binding: HirId) -> Option<BindingValue<'tcx>> {
        for (_, node) in self.tcx.hir_parent_iter(binding) {
            match node {
                Node::LetStmt(local) => {
                    let (tuple_field, tuple_arity) = tuple_binding_position(local.pat, binding)?;
                    return Some(BindingValue {
                        initializer: local.init?,
                        tuple_field,
                        tuple_arity,
                    });
                }
                Node::Expr(expression) if matches!(expression.kind, ExprKind::Let(_)) => {
                    if let ExprKind::Let(local) = expression.kind {
                        let (tuple_field, tuple_arity) = tuple_binding_position(local.pat, binding)?;
                        return Some(BindingValue {
                            initializer: local.init,
                            tuple_field,
                            tuple_arity,
                        });
                    }
                }
                Node::Param(_) | Node::Item(_) | Node::Expr(_) => return None,
                _ => (),
            }
        }
        None
    }

    fn whole_source_root(
        &self,
        expression: &'tcx Expr<'tcx>,
        seen: &mut Vec<HirId>,
    ) -> Option<&'tcx Expr<'tcx>> {
        let expression = strip_reference(expression);
        let binding = self.local_binding(expression)?;
        if seen.contains(&binding) {
            return None;
        }
        seen.push(binding);
        let Some(value) = self.binding_value(binding) else {
            return Some(expression);
        };
        let initializer = strip_temporary(value.initializer);
        match value.tuple_field {
            None => self.whole_source_root(initializer, seen),
            Some(index) if value.tuple_arity == Some(2) => {
                let ExprKind::Tup(fields) = initializer.kind else {
                    return None;
                };
                self.whole_source_root(fields.get(index)?, seen)
            }
            _ => None,
        }
    }

    fn byte_slice(&self, expression: &'tcx Expr<'tcx>) -> bool {
        matches!(self.expr_ty(expression).peel_refs().kind(), ty::Slice(element)
            if *element == self.tcx.types.u8)
    }

    fn tuple_binding(&self, pattern: &'tcx Pat<'tcx>, index: usize) -> Option<HirId> {
        let PatKind::Tuple(fields, _) = pattern.kind else {
            return None;
        };
        let PatKind::Binding(_, binding, _, _) = fields.get(index)?.kind else {
            return None;
        };
        Some(binding)
    }

    fn range_from_start(&self, expression: &'tcx Expr<'tcx>) -> Option<&'tcx Expr<'tcx>> {
        let expression = strip_temporary(expression);
        if !matches!(self.expr_ty(expression).peel_refs().kind(), ty::Adt(owner, _)
            if types::physical_item_path(self.tcx, owner.did(), "core", &["ops", "range", "RangeFrom"]))
        {
            return None;
        }
        let ExprKind::Struct(_, fields, _) = expression.kind else {
            return None;
        };
        fields
            .iter()
            .find(|field| field.ident.name.as_str() == "start")
            .map(|field| field.expr)
    }

    fn exact_bom_slice_closure(
        &self,
        closure_expression: &'tcx Expr<'tcx>,
        input: HirId,
    ) -> bool {
        let ExprKind::Closure(closure) = closure_expression.kind else {
            return false;
        };
        let body = self.tcx.hir_body_owned_by(closure.def_id);
        let Some(parameter) = body.params.first() else {
            return false;
        };
        if !matches!(parameter.pat.kind, PatKind::Tuple(fields, _) if fields.len() == 2) {
            return false;
        }
        let (Some(selected), Some(offset)) = (
            self.tuple_binding(parameter.pat, 0),
            self.tuple_binding(parameter.pat, 1),
        ) else {
            return false;
        };
        let mut findings = Findings::default();
        let nested = Analysis {
            tcx: self.tcx,
            typeck: self.tcx.typeck(closure.def_id),
            typing_owner: closure.def_id,
            arguments: None,
            fixed_parameters: HashSet::new(),
            flow: Flow::default(),
            findings: &mut findings,
        };
        let returned = strip_temporary(body.value);
        let returned = match returned.kind {
            ExprKind::Block(block, _) if block.stmts.is_empty() => {
                let Some(tail) = block.expr else {
                    return false;
                };
                strip_temporary(tail)
            }
            _ => returned,
        };
        let ExprKind::Tup(values) = returned.kind else {
            return false;
        };
        let (Some(encoding), Some(source)) = (values.first(), values.get(1))
        else {
            return false;
        };
        if nested.local_binding(encoding) != Some(selected) || !nested.byte_slice(source) {
            return false;
        }
        let source = strip_reference(source);
        let ExprKind::Index(base, range, _) = source.kind else {
            return false;
        };
        nested.local_binding(base) == Some(input)
            && nested
                .range_from_start(range)
                .is_some_and(|start| nested.local_binding(start) == Some(offset))
    }

    fn bom_map_or_source(&self, expression: &'tcx Expr<'tcx>) -> Option<&'tcx Expr<'tcx>> {
        let expression = strip_temporary(expression);
        let (map_or, arguments) = self.call(expression)?;
        if !types::physical_inherent_method(
            self.tcx, map_or, "core", &["option", "Option"], "map_or",
        )
            || arguments.len() != 3
            || !matches!(self.expr_ty(arguments[0]).peel_refs().kind(), ty::Adt(owner, _)
                if types::physical_item_path(self.tcx, owner.did(), "core", &["option", "Option"]))
        {
            return None;
        }
        let (for_bom, bom_arguments) = self.call(arguments[0])?;
        if !self.encoding_rs_definition(for_bom, "Encoding", "for_bom")
            || bom_arguments.len() != 1
            || !self.byte_slice(bom_arguments[0])
        {
            return None;
        }
        let input = self.local_binding(bom_arguments[0])?;
        let default = strip_temporary(arguments[1]);
        let ExprKind::Tup(defaults) = default.kind else {
            return None;
        };
        if defaults.len() != 2 || self.local_binding(&defaults[1]) != Some(input) {
            return None;
        }
        if !self.encoding_rs_type(self.expr_ty(&defaults[0]), "Encoding") {
            return None;
        }
        self.exact_bom_slice_closure(arguments[2], input)
            .then_some(bom_arguments[0])
    }

    fn tuple_binding_from_initializer(
        &self,
        initializer: &'tcx Expr<'tcx>,
        index: usize,
    ) -> Option<HirId> {
        for (_, node) in self.tcx.hir_parent_iter(initializer.hir_id) {
            if let Node::LetStmt(local) = node {
                let PatKind::Tuple(fields, _) = local.pat.kind else {
                    return None;
                };
                if fields.len() != 2 {
                    return None;
                }
                let PatKind::Binding(_, selected, _, _) = fields.get(index)?.kind else {
                    return None;
                };
                return Some(selected);
            }
        }
        None
    }

    fn source_lineage(&self, expression: &'tcx Expr<'tcx>) -> Option<SourceLineage<'tcx>> {
        let source = strip_temporary(expression);
        let binding = self.local_binding(source)?;
        if !self.byte_slice(source) {
            return None;
        }
        let (root, paired_encoding) = match self.binding_value(binding) {
            None => (source, None),
            Some(value) => match value.tuple_field {
                None => (
                    self.whole_source_root(value.initializer, &mut Vec::new())?,
                    None,
                ),
                Some(1) if value.tuple_arity == Some(2) => {
                    let initializer = strip_temporary(value.initializer);
                    let selected = self.tuple_binding_from_initializer(initializer, 0)?;
                    let root = match initializer.kind {
                        ExprKind::Tup(fields) => {
                            if self.local_binding(fields.get(0)?) != Some(selected) {
                                return None;
                            }
                            self.whole_source_root(fields.get(1)?, &mut Vec::new())?
                        }
                        _ => self.bom_map_or_source(initializer)?,
                    };
                    (root, Some(selected))
                }
                _ => return None,
            },
        };
        Some(SourceLineage {
            binding,
            root,
            paired_encoding,
        })
    }

    fn slice_range_is_read_suffix(
        &self,
        input: &'tcx Expr<'tcx>,
        source_binding: HirId,
    ) -> Option<(HirId, HirId)> {
        let input = strip_reference(input);
        let ExprKind::Index(base, range, _) = input.kind else {
            return None;
        };
        if self.local_binding(base) != Some(source_binding) {
            return None;
        }
        let start = self.range_from_start(range)?;
        Some((
            self.local_binding(start)?,
            strip_temporary(start).hir_id,
        ))
    }

    fn decoder_result_pattern(&self, pattern: &'tcx Pat<'tcx>, name: &str) -> bool {
        let path = match pattern.kind {
            PatKind::Expr(expression) => match expression.kind {
                rustc_hir::PatExprKind::Path(path) => path,
                _ => return false,
            },
            PatKind::TupleStruct(path, _, _) => path,
            _ => return false,
        };
        let Res::Def(_, definition) = self.typeck.qpath_res(&path, pattern.hir_id) else {
            return false;
        };
        let variant = if matches!(self.tcx.def_kind(definition), rustc_hir::def::DefKind::Ctor(..)) {
            self.tcx.parent(definition)
        } else {
            definition
        };
        matches!(self.tcx.def_kind(variant), rustc_hir::def::DefKind::Variant)
            && types::physical_item_path(self.tcx, variant, "encoding_rs", &["DecoderResult", name])
    }

    fn direct_break(&self, expression: &'tcx Expr<'tcx>) -> bool {
        let expression = strip_temporary(expression);
        match expression.kind {
            ExprKind::Break(..) => true,
            ExprKind::Block(block, _) if block.stmts.is_empty() => {
                block.expr.is_some_and(|value| self.direct_break(value))
            }
            _ => false,
        }
    }

    fn direct_return(&self, expression: &'tcx Expr<'tcx>) -> bool {
        let expression = strip_temporary(expression);
        match expression.kind {
            ExprKind::Ret(Some(_)) => true,
            ExprKind::Block(block, _) => {
                if let Some(value) = block.expr {
                    block.stmts.is_empty() && self.direct_return(value)
                } else {
                    block.stmts.len() == 1
                        && matches!(block.stmts[0].kind,
                            rustc_hir::StmtKind::Semi(value) if self.direct_return(value))
                }
            }
            _ => false,
        }
    }

    fn empty_block(&self, expression: &'tcx Expr<'tcx>) -> bool {
        let expression = strip_temporary(expression);
        matches!(expression.kind, ExprKind::Block(block, _) if block.stmts.is_empty() && block.expr.is_none())
    }

    fn decoder_result_exits(&self, block: &'tcx Block<'tcx>, result: HirId) -> bool {
        let mut finder = ResultMatches {
            analysis: self,
            result,
            matches: Vec::new(),
        };
        finder.visit_block(block);
        if finder.matches.len() != 1 {
            return false;
        }
        let ExprKind::Match(_, arms, _) = finder.matches[0].kind else {
            return false;
        };
        if arms.len() != 3 {
            return false;
        }
        let mut input_empty = false;
        let mut output_full = false;
        let mut malformed = false;
        for arm in arms {
            if self.decoder_result_pattern(arm.pat, "InputEmpty") {
                if input_empty || !self.direct_break(arm.body) {
                    return false;
                }
                input_empty = true;
            } else if self.decoder_result_pattern(arm.pat, "OutputFull") {
                if output_full || !self.empty_block(arm.body) {
                    return false;
                }
                output_full = true;
            } else if self.decoder_result_pattern(arm.pat, "Malformed") {
                if malformed || !self.direct_return(arm.body) {
                    return false;
                }
                malformed = true;
            } else {
                return false;
            }
        }
        input_empty && output_full && malformed
    }

    fn tuple_call_bindings(
        &self,
        call: &'tcx Expr<'tcx>,
    ) -> Option<(HirId, HirId, HirId)> {
        for (_, node) in self.tcx.hir_parent_iter(call.hir_id) {
            match node {
                Node::LetStmt(local) => {
                    if strip_temporary(local.init?).hir_id != strip_temporary(call).hir_id {
                        return None;
                    }
                    let PatKind::Tuple(fields, _) = local.pat.kind else {
                        return None;
                    };
                    if fields.len() != 3 {
                        return None;
                    }
                    let ids: Option<Vec<_>> = fields
                        .iter()
                        .map(|field| match field.kind {
                            PatKind::Binding(_, binding, _, _) => Some(binding),
                            _ => None,
                        })
                        .collect();
                    let ids = ids?;
                    return Some((ids[0], ids[1], ids[2]));
                }
                Node::Expr(parent)
                    if matches!(parent.kind, ExprKind::DropTemps(_) | ExprKind::AddrOf(..)) => {}
                Node::Param(_) | Node::Item(_) | Node::Block(_) => return None,
                _ => (),
            }
        }
        None
    }

    fn decoder_tuple_type(&self, call: &'tcx Expr<'tcx>) -> bool {
        let ty::Tuple(fields) = self.expr_ty(call).kind() else {
            return false;
        };
        fields.len() == 3
            && self.encoding_rs_type(fields[0], "DecoderResult")
            && fields[1] == self.tcx.types.usize
            && fields[2] == self.tcx.types.usize
    }

    fn decoder_constructor_receiver(
        &self,
        decoder_binding: HirId,
    ) -> Option<&'tcx Expr<'tcx>> {
        let value = self.binding_value(decoder_binding)?;
        if value.tuple_field.is_some() || value.tuple_arity.is_some() {
            return None;
        }
        let (definition, operands) = self.call(strip_temporary(value.initializer))?;
        if !self.encoding_rs_definition(
            definition,
            "Encoding",
            "new_decoder_without_bom_handling",
        ) || operands.len() != 1
            || !self.encoding_rs_type(self.expr_ty(operands[0]), "Encoding")
        {
            return None;
        }
        operands.first().copied()
    }

    fn decoder_call_shape(
        &self,
        call: &'tcx Expr<'tcx>,
    ) -> Option<DecoderStep> {
        let (definition, operands) = self.call(call)?;
        if !self.incremental_decode_method(definition)
            || operands.len() != 4
            || !self.decoder_tuple_type(call)
            || !self.encoding_rs_type(self.expr_ty(operands[0]), "Decoder")
            || !self.byte_slice(operands[1])
            || !matches!(self.expr_ty(operands[2]).kind(), ty::Ref(_, inner, rustc_hir::Mutability::Mut)
                if matches!(inner.kind(), ty::Array(element, length)
                    if *element == self.tcx.types.u8
                        && length.try_to_target_usize(self.tcx).is_some_and(|size| size >= 4)))
            || !matches!(strip_temporary(operands[3]).kind,
                ExprKind::Lit(literal) if matches!(literal.node, rustc_ast::LitKind::Bool(true)))
        {
            return None;
        }
        let decoder = self.local_binding(operands[0])?;
        let (result, consumed, _written) = self.tuple_call_bindings(call)?;
        let input = strip_reference(operands[1]);
        let ExprKind::Index(source, _, _) = input.kind else {
            return None;
        };
        let source_binding = self.local_binding(source)?;
        let (read, read_use) = self.slice_range_is_read_suffix(operands[1], source_binding)?;
        Some(DecoderStep {
            decoder,
            result,
            consumed,
            read,
            decoder_use: strip_temporary(operands[0]).hir_id,
            read_use,
        })
    }

    fn result_match_ordered(
        &self,
        block: &'tcx Block<'tcx>,
        result: HirId,
        after_statement: usize,
    ) -> Option<&'tcx Expr<'tcx>> {
        let mut finder = ResultMatches {
            analysis: self,
            result,
            matches: Vec::new(),
        };
        finder.visit_block(block);
        if finder.matches.len() != 1 {
            return None;
        }
        let expression = finder.matches[0];
        if block
            .expr
            .is_some_and(|tail| strip_temporary(tail).hir_id == strip_temporary(expression).hir_id)
        {
            return (after_statement < block.stmts.len()).then_some(expression);
        }
        let index = block
            .stmts
            .iter()
            .position(|statement| statement.span.contains(expression.span))?;
        let direct_statement = match block.stmts[index].kind {
            rustc_hir::StmtKind::Expr(value) | rustc_hir::StmtKind::Semi(value) => {
                strip_temporary(value).hir_id == strip_temporary(expression).hir_id
            }
            _ => false,
        };
        (index > after_statement && direct_statement).then_some(expression)
    }

    fn top_level_call_let(
        &self,
        block: &'tcx Block<'tcx>,
        call: &'tcx Expr<'tcx>,
    ) -> Option<usize> {
        let index = block
            .stmts
            .iter()
            .position(|statement| statement.span.contains(call.span))?;
        let rustc_hir::StmtKind::Let(local) = block.stmts[index].kind else {
            return None;
        };
        let initializer = strip_temporary(local.init?);
        (initializer.hir_id == strip_temporary(call).hir_id).then_some(index)
    }

    fn top_level_read_update(
        &self,
        block: &'tcx Block<'tcx>,
        write: &'tcx Expr<'tcx>,
    ) -> Option<usize> {
        let index = block
            .stmts
            .iter()
            .position(|statement| statement.span.contains(write.span))?;
        let rustc_hir::StmtKind::Semi(statement_expression) = block.stmts[index].kind else {
            return None;
        };
        (strip_temporary(statement_expression).hir_id == strip_temporary(write).hir_id)
            .then_some(index)
    }

    fn loop_structure(
        &self,
        block: &'tcx Block<'tcx>,
        call: &'tcx Expr<'tcx>,
        step: DecoderStep,
    ) -> bool {
        let DecoderStep { read, consumed, result, read_use, decoder, decoder_use } = step;
        let Some(call_statement) = self.top_level_call_let(block, call) else {
            return false;
        };
        let Some(read_value) = self.binding_value(read) else {
            return false;
        };
        if read_value.tuple_field.is_some()
            || read_value.tuple_arity.is_some()
            || !matches!(strip_temporary(read_value.initializer).kind,
                ExprKind::Lit(literal) if matches!(literal.node, rustc_ast::LitKind::Int(value, _) if value.get() == 0))
        {
            return false;
        }
        if self.flow.mutated.contains(&format!("local:{read:?}")) {
            return false;
        }
        let mut writes = ReadWrites {
            analysis: self,
            read,
            consumed,
            writes: Vec::new(),
            uses: Vec::new(),
        };
        writes.visit_block(block);
        if writes.writes.len() != 1 || !writes.writes[0].1 {
            return false;
        }
        let write = writes.writes[0].0;
        let ExprKind::AssignOp(_, target, _) = write.kind else {
            return false;
        };
        let allowed_read_uses = [read_use, strip_temporary(target).hir_id];
        if writes.uses.len() != allowed_read_uses.len()
            || !allowed_read_uses
                .iter()
                .all(|allowed| writes.uses.contains(allowed))
        {
            return false;
        }
        let mut decoder_uses = BindingUses {
            analysis: self,
            binding: decoder,
            uses: Vec::new(),
        };
        decoder_uses.visit_block(block);
        if decoder_uses.uses.len() != 1
            || decoder_uses.uses.first().copied() != Some(decoder_use)
        {
            return false;
        }
        let Some(write_statement) = self.top_level_read_update(block, write) else {
            return false;
        };
        if call_statement != 0 || write_statement != call_statement + 1 {
            return false;
        }
        let Some(result_match) = self.result_match_ordered(block, result, write_statement) else {
            return false;
        };
        let mut control = UnstructuredLoopControl {
            result_match: result_match.span,
            found: false,
        };
        control.visit_block(block);
        if control.found {
            return false;
        }
        self.decoder_result_exits(block, result)
    }

    fn prior_charge_source(
        &self,
        loop_expression: &'tcx Expr<'tcx>,
        root_binding: HirId,
    ) -> Option<&'tcx Expr<'tcx>> {
        let (block, loop_index, block_id) = self.enclosing_loop_statement(loop_expression)?;
        for statement in &block.stmts[..loop_index] {
            let mut charges = PriorCharges {
                analysis: self,
                calls: Vec::new(),
            };
            charges.visit_stmt(statement);
            for charge in charges.calls {
                let nearest_block = self
                    .tcx
                    .hir_parent_iter(charge.hir_id)
                    .find_map(|(id, node)| matches!(node, Node::Block(_)).then_some(id));
                if nearest_block != Some(block_id) || !self.propagated(charge) {
                    continue;
                }
                let Some((_, operands)) = self.call(charge) else {
                    continue;
                };
                if operands.len() != 3 || !self.context_operand(operands[0]) {
                    continue;
                }
                let Some((conversion, converted)) = self.call(operands[1]) else {
                    continue;
                };
                if !self.core_u64_from_index(conversion) || converted.len() != 1 {
                    continue;
                }
                let Some((length, length_operands)) = self.call(converted[0]) else {
                    continue;
                };
                if self.tcx.crate_name(length.krate).as_str() != "core"
                    || self.tcx.item_name(length).as_str() != "len"
                    || length_operands.len() != 1
                    || !self.byte_slice(length_operands[0])
                {
                    continue;
                }
                if self.local_binding(length_operands[0]) == Some(root_binding) {
                    return Some(length_operands[0]);
                }
            }
        }
        None
    }

    fn enclosing_loop_statement(
        &self,
        expression: &'tcx Expr<'tcx>,
    ) -> Option<(&'tcx Block<'tcx>, usize, HirId)> {
        for (parent_id, node) in self.tcx.hir_parent_iter(expression.hir_id) {
            if let Node::Block(block) = node {
                if let Some(index) = block
                    .stmts
                    .iter()
                    .position(|statement| statement.span.contains(expression.span))
                {
                    return Some((block, index, parent_id));
                }
            }
        }
        None
    }

    fn decoder_is_fresh_at_loop(
        &self,
        loop_expression: &'tcx Expr<'tcx>,
        decoder: HirId,
    ) -> bool {
        let Some(value) = self.binding_value(decoder) else {
            return false;
        };
        if value.tuple_field.is_some() || value.tuple_arity.is_some() {
            return false;
        }
        let Some((block, loop_index, _)) = self.enclosing_loop_statement(loop_expression) else {
            return false;
        };
        let Some(initializer_index) = block
            .stmts
            .iter()
            .position(|statement| statement.span.contains(value.initializer.span))
        else {
            return false;
        };
        if initializer_index >= loop_index {
            return false;
        }
        let mut uses = BindingUses {
            analysis: self,
            binding: decoder,
            uses: Vec::new(),
        };
        for (index, statement) in block.stmts.iter().take(loop_index).enumerate() {
            if index != initializer_index {
                uses.visit_stmt(statement);
            }
        }
        uses.uses.is_empty()
    }

    /// Establish a same-source cumulative decoder receipt and consume one
    /// prior propagated charge. The owning loop keeps a finding when the
    /// exact source charge is absent.
    pub(crate) fn measured_decoder_loop_paid(
        &mut self,
        expression: &'tcx Expr<'tcx>,
    ) -> Option<Option<bool>> {
        let ExprKind::Loop(block, _, LoopSource::Loop, _) = expression.kind else {
            return None;
        };
        let mut calls = DecoderCalls {
            analysis: self,
            calls: Vec::new(),
            nested_loop: false,
        };
        calls.visit_block(block);
        if calls.nested_loop || calls.calls.len() != 1 {
            return None;
        }
        let call = calls.calls[0];
        let step = self.decoder_call_shape(call)?;
        let decoder = step.decoder;
        let read = step.read;
        let (_, operands) = self.call(call)?;
        let source_expression = {
            let input = strip_reference(operands[1]);
            let ExprKind::Index(base, _, _) = input.kind else {
                return None;
            };
            base
        };
        let source = self.source_lineage(source_expression)?;
        if self.local_binding(source_expression) != Some(source.binding)
            || !self.loop_structure(block, call, step)
        {
            return None;
        }
        if let Some(paired_encoding) = source.paired_encoding {
            let receiver = self.decoder_constructor_receiver(decoder)?;
            if self.local_binding(receiver) != Some(paired_encoding) {
                return None;
            }
        } else {
            self.decoder_constructor_receiver(decoder)?;
        }
        if !self.decoder_is_fresh_at_loop(expression, decoder) {
            return None;
        }

        let Some(root_binding) = self.local_binding(source.root) else {
            return Some(None);
        };
        if [
            source.binding,
            root_binding,
            read,
            decoder,
        ]
        .iter()
        .any(|binding| self.flow.mutated.contains(&format!("local:{binding:?}")))
        {
            return None;
        }
        self.findings.handled_work_operations.insert(call.hir_id);
        let Some(charge_source) = self.prior_charge_source(expression, root_binding) else {
            return Some(Some(false));
        };
        Some(self.take_credit(&[charge_source]))
    }
}

fn strip_temporary<'tcx>(mut expression: &'tcx Expr<'tcx>) -> &'tcx Expr<'tcx> {
    while let ExprKind::DropTemps(inner) = expression.kind {
        expression = inner;
    }
    expression
}

fn strip_reference<'tcx>(expression: &'tcx Expr<'tcx>) -> &'tcx Expr<'tcx> {
    let mut expression = strip_temporary(expression);
    loop {
        match expression.kind {
            ExprKind::AddrOf(_, _, inner) | ExprKind::DropTemps(inner) => {
                expression = strip_temporary(inner)
            }
            _ => return expression,
        }
    }
}

fn tuple_binding_position(
    pattern: &Pat<'_>,
    binding: HirId,
) -> Option<(Option<usize>, Option<usize>)> {
    match pattern.kind {
        PatKind::Binding(_, found, _, _) if found == binding => Some((None, None)),
        PatKind::Tuple(fields, _) => fields.iter().enumerate().find_map(|(index, field)| {
            matches!(field.kind, PatKind::Binding(_, found, _, _) if found == binding)
                .then_some((Some(index), Some(fields.len())))
        }),
        _ => None,
    }
}
