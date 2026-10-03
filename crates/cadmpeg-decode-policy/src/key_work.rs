// SPDX-License-Identifier: Apache-2.0
//! Single-use receipts for complete key bytes and tree lookup bounds.
use crate::{flow::{Credit, ExtentTerm}, types, Analysis};
use rustc_hir::Expr;

impl<'tcx> Analysis<'_, 'tcx> {
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
        let Some(key) = args.get(1).and_then(|value| self.key(value, &mut Vec::new())) else { return; };
        let Some(count) = args.get(2) else { return; };
        let (factor, coefficient) = if let Some(count) = self.constant_count(count, &mut Vec::new()) {
            let Some(coefficient) = count.checked_mul(self.flow.iterations) else { return; };
            (format!("keybytes:{key}"), coefficient)
        } else if let Some(target) = self.tree_target(count) {
            (format!("treekeybytes:{target}|{key}"), self.flow.iterations)
        } else { return; };
        self.flow.work.push(Credit { extents: vec![ExtentTerm { factors: vec![factor], coefficient }], opaque: false });
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
        let comparison = matches!(name, "comparison" | "eq" | "ne" | "lt" | "le" | "gt" | "ge" | "cmp" | "partial_cmp");
        if comparison {
            let keys: Option<Vec<_>> = operands.iter().map(|value| self.key(value, &mut Vec::new())).collect();
            let Some(keys) = keys else { return false; };
            let before = self.flow.work.clone();
            if keys.iter().all(|key| self.consume_key_factor(&format!("keybytes:{key}"))) { return true; }
            self.flow.work = before;
            return false;
        }
        if !matches!(name, "get" | "get_mut" | "contains" | "contains_key" | "insert" | "remove" | "entry" | "hash") { return false; }
        let Some(receiver) = operands.first() else { return false; };
        if name == "hash" {
            return self.key(receiver, &mut Vec::new()).is_some_and(|key| self.consume_key_factor(&format!("keybytes:{key}")));
        }
        let value = self.expr_ty(receiver).peel_refs();
        let rustc_middle::ty::Adt(owner, _) = value.kind() else { return false; };
        if !types::standard(self.tcx, owner.did()) { return false; }
        let kind = self.tcx.item_name(owner.did());
        if !matches!(kind.as_str(), "HashMap" | "HashSet" | "BTreeMap" | "BTreeSet") { return false; }
        let Some(key) = operands.get(1).and_then(|key| self.key(key, &mut Vec::new())) else { return false; };
        if matches!(kind.as_str(), "BTreeMap" | "BTreeSet") {
            self.key(receiver, &mut Vec::new()).is_some_and(|target| self.consume_key_factor(&format!("treekeybytes:{target}|{key}")))
        } else {
            self.consume_key_factor(&format!("keybytes:{key}"))
        }
    }
}
