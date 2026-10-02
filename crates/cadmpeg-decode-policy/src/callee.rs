// SPDX-License-Identifier: Apache-2.0
use rustc_hir::{Expr, ExprKind};
use rustc_hir::intravisit::Visitor;
use rustc_middle::ty::{self, Instance, TypingEnv};
use rustc_span::def_id::DefId;
use crate::{Analysis, flow, types};

impl<'tcx> Analysis<'_, 'tcx> {
    pub(crate) fn implementation(&self, expression: &'tcx Expr<'tcx>, definition: DefId) -> Option<DefId> {
        let args = match expression.kind {
            ExprKind::Call(callee, _) => self.typeck.node_args(callee.hir_id),
            _ => self.typeck.node_args(expression.hir_id),
        };
        Instance::try_resolve(self.tcx, TypingEnv::post_analysis(self.tcx, self.owner), definition, args).ok().flatten().map(|instance| instance.def_id())
    }

    pub(crate) fn local_has_effects(&self, definition: DefId) -> Option<bool> {
        if let Some(value) = self.summaries.borrow().get(&definition) { return Some(*value); }
        let local = definition.as_local()?;
        if self.stack.contains(&local) { return None; }
        let body = self.tcx.hir_maybe_body_owned_by(local)?;
        let mut findings = crate::Findings::default();
        let mut stack = self.stack.clone();
        stack.push(local);
        let mut analysis = Analysis { tcx: self.tcx, typeck: self.tcx.typeck(local), owner: local, summaries: self.summaries, flow: flow::Flow::default(), stack, findings: &mut findings };
        analysis.visit_body(body);
        let has_effects = !findings.entries.is_empty();
        self.summaries.borrow_mut().insert(definition, has_effects);
        Some(has_effects)
    }

    pub(crate) fn custom_trait(&self, expression: &'tcx Expr<'tcx>, definition: DefId) -> Option<DefId> {
        let mut implementation = self.implementation(expression, definition)?;
        if types::standard(self.tcx, implementation) && self.tcx.trait_of_assoc(definition).is_some() {
            let args = match expression.kind {
                ExprKind::Call(callee, _) => self.typeck.node_args(callee.hir_id),
                _ => self.typeck.node_args(expression.hir_id),
            };
            let peeled: Vec<_> = args.iter().map(|argument| match argument.kind() {
                ty::GenericArgKind::Type(value) => value.peel_refs().into(),
                _ => argument,
            }).collect();
            if let Ok(Some(instance)) = Instance::try_resolve(self.tcx, TypingEnv::post_analysis(self.tcx, self.owner), definition, self.tcx.mk_args(&peeled)) { implementation = instance.def_id(); }
        }
        if !types::standard(self.tcx, implementation) && !self.tcx.is_automatically_derived(self.tcx.parent(implementation)) { Some(implementation) } else { None }
    }

    pub(crate) fn indirect(&mut self, expression: &'tcx Expr<'tcx>) {
        let ExprKind::Call(callee, _) = expression.kind else { return; };
        if matches!(self.typeck.expr_ty(callee).kind(), ty::FnDef(_, _)) { return; }
        if let ty::Closure(definition, _) = self.typeck.expr_ty(callee).peel_refs().kind() {
            if self.local_has_effects(*definition) == Some(false) { return; }
        }
        self.report(expression.span, "unproven_decode_charge", "indirect call: callback allocation and work charges cannot be established; use a concrete context-taking callee or explicit admission inside the callback");
    }
}

impl<'tcx> Analysis<'_, 'tcx> {
    pub(crate) fn iterator_item(&self, value: ty::Ty<'tcx>) -> Option<ty::Ty<'tcx>> {
        let iterator = self.tcx.lang_items().iterator_trait()?;
        let item = self.tcx.associated_items(iterator).in_definition_order().find(|item| item.name() == rustc_span::sym::Item)?;
        let projection = ty::Ty::new_projection(self.tcx, ty::IsRigid::No, item.def_id, [value]);
        self.tcx.try_normalize_erasing_regions(TypingEnv::post_analysis(self.tcx, self.owner), ty::Unnormalized::new_wip(projection)).ok()
    }
}

impl<'tcx> Analysis<'_, 'tcx> {
    pub(crate) fn clone_shape(&self, value: ty::Ty<'tcx>) -> types::Shape {
        let shape = types::heap(self.tcx, value, &mut Vec::new());
        if shape == types::Shape::Fixed { return shape; }
        let Some(clone) = self.tcx.lang_items().clone_trait() else { return types::Shape::Unknown; };
        let Some(method) = self.tcx.associated_items(clone).in_definition_order().find(|item| item.name() == rustc_span::sym::clone) else { return types::Shape::Unknown; };
        let args = self.tcx.mk_args(&[value.into()]);
        match Instance::try_resolve(self.tcx, TypingEnv::post_analysis(self.tcx, self.owner), method.def_id, args) {
            Ok(Some(instance)) => {
                let id = instance.def_id();
                if types::standard(self.tcx, id) || self.tcx.is_automatically_derived(self.tcx.parent(id)) { shape }
                else if self.local_has_effects(id) == Some(false) { types::Shape::Fixed }
                else { types::Shape::Unknown }
            }
            _ => types::Shape::Unknown,
        }
    }
}
