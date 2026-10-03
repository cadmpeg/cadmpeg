// SPDX-License-Identifier: Apache-2.0
//! Single-use receipts for complete key bytes and tree lookup bounds.
use crate::{flow::{Credit, ExtentTerm}, types, Analysis};
use rustc_hir::{Expr, ExprKind, Node, PatKind};
use rustc_middle::ty::TyCtxt;
use rustc_span::{Span, def_id::DefId};

impl<'tcx> Analysis<'_, 'tcx> {
    fn key_index_operand(&self, expression: &'tcx Expr<'tcx>) -> String {
        if let Some(initializer) = self.initializer(expression) {
            if self.expr_ty(expression) == self.expr_ty(initializer) {
                return self.key_index_operand(initializer);
            }
        }
        if let ExprKind::Struct(_, fields, _) = expression.kind {
            if let rustc_middle::ty::Adt(owner, _) = self.expr_ty(expression).kind() {
                let name = self.tcx.item_name(owner.did());
                if types::standard(self.tcx, owner.did())
                    && matches!(name.as_str(), "Range" | "RangeFrom" | "RangeTo" | "RangeFull" | "RangeToInclusive") {
                    let fields: Vec<_> = fields.iter().map(|field| {
                        format!("{}:{}", field.ident.name, self.key_index_operand(field.expr))
                    }).collect();
                    return format!("{name}({})", fields.join(","));
                }
            }
        }
        self.constant_count(expression, &mut Vec::new()).map(|value| value.to_string())
            .or_else(|| self.key(expression, &mut Vec::new()))
            .unwrap_or_else(|| format!("expression:{:?}", expression.hir_id))
    }

    fn option_payload_binding(&self, expression: &'tcx Expr<'tcx>, initializer: &'tcx Expr<'tcx>) -> bool {
        let rustc_middle::ty::Adt(owner, arguments) = self.expr_ty(initializer).kind() else { return false; };
        if !types::standard(self.tcx, owner.did()) || self.tcx.item_name(owner.did()).as_str() != "Option"
            || arguments.types().next() != Some(self.expr_ty(expression)) { return false; }
        let ExprKind::Path(ref path) = expression.kind else { return false; };
        let rustc_hir::def::Res::Local(binding) = self.typeck.qpath_res(path, expression.hir_id) else { return false; };
        for (_, node) in self.tcx.hir_parent_iter(binding) {
            if let Node::Pat(pattern) = node {
                if let PatKind::TupleStruct(ref path, [payload], _) = pattern.kind {
                    return matches!(payload.kind, PatKind::Binding(_, id, _, None) if id == binding)
                        && matches!(self.typeck.qpath_res(path, pattern.hir_id), rustc_hir::def::Res::Def(_, definition)
                            if types::standard(self.tcx, definition) && self.tcx.item_name(definition).as_str() == "Some");
                }
                continue;
            }
            return false;
        }
        false
    }

    // Complete-value receipts distinguish subwindows; length admission may dominate them.
    fn key_work_operand(&self, expression: &'tcx Expr<'tcx>) -> Option<String> {
        if let Some(initializer) = self.initializer(expression) {
            if self.expr_ty(expression) == self.expr_ty(initializer)
                || self.option_payload_binding(expression, initializer) {
                if let Some(key) = self.key_work_operand(initializer) { return Some(key); }
            }
        }
        match expression.kind {
            ExprKind::Path(ref path) if matches!(self.typeck.qpath_res(path, expression.hir_id), rustc_hir::def::Res::Local(_)) => {
                let rustc_hir::def::Res::Local(id) = self.typeck.qpath_res(path, expression.hir_id) else { return None; };
                Some(format!("local:{id:?}"))
            }
            ExprKind::AddrOf(_, _, value) | ExprKind::DropTemps(value) | ExprKind::Unary(_, value) => self.key_work_operand(value),
            ExprKind::Index(base, index, _) => {
                let index = self.key_index_operand(index);
                self.key_work_operand(base).map(|base| format!("{base}[{index}]"))
            }
            ExprKind::Field(base, field) => self.key_work_operand(base).map(|base| format!("{base}.{}", field.name)),
            _ => {
                if let Some((definition, args)) = self.call(expression) {
                    if types::standard(self.tcx, definition) {
                        match self.tcx.item_name(definition).as_str() {
                            "get" | "get_mut" => {
                                let index = args.get(1)?;
                                let index = self.key_index_operand(index);
                                return self.key_work_operand(args.first()?).map(|base| format!("{base}[{index}]"));
                            }
                            "as_bytes" | "as_slice" => return self.key_work_operand(args.first()?),
                            _ => (),
                        }
                    }
                }
                self.key(expression, &mut Vec::new())
            },
        }
    }
    pub(crate) fn record_move_work(&mut self, expression: &'tcx Expr<'tcx>) {
        let Some((definition, args)) = self.call(expression) else { return; };
        if self.tcx.crate_name(definition.krate).as_str() != "cadmpeg_core"
            || self.tcx.item_name(definition).as_str() != "admit_moves"
            || !self.propagated(expression) { return; }
        let Some(key) = args.get(1).and_then(|value| self.key_work_operand(value)) else { return; };
        let Some(count) = args.get(2).and_then(|count| self.constant_count(count, &mut Vec::new())) else { return; };
        let Some(coefficient) = count.checked_mul(self.flow.iterations) else { return; };
        self.flow.work.push(Credit { extents: vec![ExtentTerm { factors: vec![format!("movebytes:{key}")], coefficient }], opaque: false });
    }
    pub(crate) fn move_work_paid(&mut self, operands: &[&'tcx Expr<'tcx>], name: &str) -> bool {
        let (index, moves): (usize, u64) = match name {
            "reverse" => (0, 3),
            "copy_from_slice" | "extend_from_slice" | "append" => (1, 1),
            _ => return false,
        };
        let Some(factor) = operands.get(index).and_then(|value| self.key_work_operand(value)).map(|key| format!("movebytes:{key}")) else { return false; };
        let Some(required) = moves.checked_mul(self.flow.iterations) else { return false; };
        let Some(term) = self.flow.work.iter_mut().filter(|credit| !credit.opaque).flat_map(|credit| &mut credit.extents)
            .find(|term| term.coefficient >= required && term.factors == [factor.as_str()]) else { return false; };
        term.coefficient -= required;
        true
    }

    fn tree_target(&self, count: &'tcx Expr<'tcx>) -> Option<String> {
        let (definition, args) = self.call(count)?;
        if self.tcx.crate_name(definition.krate).as_str() != "cadmpeg_core"
            || self.tcx.item_name(definition).as_str() != "tree_comparisons" { return None; }
        let (len, values) = self.call(args.get(1)?)?;
        if !types::standard(self.tcx, len) || self.tcx.item_name(len).as_str() != "len" { return None; }
        self.key(values.first()?, &mut Vec::new())
    }
    pub(crate) fn record_key_work(&mut self, expression: &'tcx Expr<'tcx>) {
        let Some((definition, args)) = self.call(expression) else { return; };
        if self.tcx.crate_name(definition.krate).as_str() != "cadmpeg_core"
            || self.tcx.item_name(definition).as_str() != "charge_key"
            || !self.propagated(expression) { return; }
        let Some(key) = args.get(1).and_then(|value| self.key_work_operand(value)) else { return; };
        let Some(count) = args.get(2) else { return; };
        let (factor, coefficient) = if let Some(count) = self.constant_count(count, &mut Vec::new()) {
            let Some(coefficient) = count.checked_mul(self.flow.iterations) else { return; };
            (format!("keybytes:{key}"), coefficient)
        } else if let Some(target) = self.tree_target(count) {
            (format!("treekeybytes:{target}|{key}"), self.flow.iterations)
        } else { return; };
        self.flow.work.push(Credit { extents: vec![ExtentTerm { factors: vec![factor], coefficient }], opaque: false });
    }
    pub(crate) fn record_sort_work(&mut self, expression: &'tcx Expr<'tcx>) {
        let Some((definition, args)) = self.call(expression) else { return; };
        if self.tcx.crate_name(definition.krate).as_str() != "cadmpeg_core"
            || self.tcx.item_name(definition).as_str() != "admit_sort"
            || !self.propagated(expression) { return; }
        let Some(key) = args.get(1).and_then(|value| self.key_work_operand(value)) else { return; };
        self.flow.work.push(Credit { extents: vec![ExtentTerm { factors: vec![format!("sortbytes:{key}")], coefficient: self.flow.iterations }], opaque: false });
    }
    fn consume_key_factor(&mut self, factor: &str) -> bool {
        let Some(term) = self.flow.work.iter_mut().filter(|credit| !credit.opaque)
            .flat_map(|credit| &mut credit.extents)
            .find(|term| term.coefficient >= self.flow.iterations && term.factors == [factor])
        else { return false; };
        term.coefficient -= self.flow.iterations;
        true
    }
    pub(crate) fn key_work_paid(&mut self, operands: &[&'tcx Expr<'tcx>], name: &str) -> bool {
        if name == "truncate" {
            let Some(receiver) = operands.first() else { return false; };
            let rustc_middle::ty::Adt(owner, _) = self.expr_ty(receiver).peel_refs().kind() else { return false; };
            if !types::standard(self.tcx, owner.did()) || self.tcx.item_name(owner.did()).as_str() != "Vec" { return false; }
            let Some(target) = self.key_work_operand(receiver) else { return false; };
            let Some(start) = operands.get(1).map(|value| self.key_index_operand(value)) else { return false; };
            return self.consume_key_factor(&format!("keybytes:{target}[RangeFrom(start:{start})]"));
        }
        if matches!(name, "sort_unstable_by" | "sort_unstable_by_key" | "sort_by" | "sort_by_key" | "sort" | "sort_unstable") {
            return operands.first().and_then(|value| self.key_work_operand(value))
                .is_some_and(|key| self.consume_key_factor(&format!("sortbytes:{key}")));
        }
        let comparison = matches!(name, "comparison" | "eq" | "ne" | "lt" | "le" | "gt" | "ge" | "cmp" | "partial_cmp");
        if comparison {
            let keys: Option<Vec<_>> = operands.iter().map(|value| self.key_work_operand(value)).collect();
            let Some(keys) = keys else { return false; };
            let before = self.flow.work.clone();
            if keys.iter().all(|key| self.consume_key_factor(&format!("keybytes:{key}"))) { return true; }
            self.flow.work = before;
            return false;
        }
        if !matches!(name, "get" | "get_mut" | "contains" | "contains_key" | "insert" | "remove" | "entry" | "get_key_value" | "remove_entry" | "hash") { return false; }
        let Some(receiver) = operands.first() else { return false; };
        if name == "hash" {
            return self.key_work_operand(receiver).is_some_and(|key| self.consume_key_factor(&format!("keybytes:{key}")));
        }
        let value = self.expr_ty(receiver).peel_refs();
        let rustc_middle::ty::Adt(owner, _) = value.kind() else { return false; };
        if !types::standard(self.tcx, owner.did()) { return false; }
        let kind = self.tcx.item_name(owner.did());
        if !matches!(kind.as_str(), "HashMap" | "HashSet" | "BTreeMap" | "BTreeSet") { return false; }
        let Some(key) = operands.get(1).and_then(|key| self.key_work_operand(key)) else { return false; };
        if matches!(kind.as_str(), "BTreeMap" | "BTreeSet") {
            self.key_work_operand(receiver).is_some_and(|target| self.consume_key_factor(&format!("treekeybytes:{target}|{key}")))
        } else {
            self.consume_key_factor(&format!("keybytes:{key}"))
        }
    }
}

/// A source site is stable across owning and importing compiler sessions.
/// The definition hash includes the crate configuration; offsets are file local.
pub(crate) fn proof_key(tcx: TyCtxt<'_>, owner: DefId, span: Span) -> String {
    let span = span.source_callsite();
    let file = tcx.sess.source_map().lookup_source_file(span.lo());
    format!("{}|{}|{}", crate::scope::key(tcx, owner), span.lo().0 - file.start_pos.0, span.hi().0 - span.lo().0)
}

impl<'tcx> Analysis<'_, 'tcx> {
    pub(crate) fn record_key_work_proof(&mut self, expression: &'tcx Expr<'tcx>) {
        self.findings.key_work_proofs.insert(proof_key(self.tcx, self.typeck.hir_owner.def_id.to_def_id(), expression.span));
    }
}
