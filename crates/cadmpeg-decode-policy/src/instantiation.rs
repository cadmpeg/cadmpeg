// SPDX-License-Identifier: Apache-2.0
use crate::{flow, Analysis, Findings};
use rustc_hir::intravisit::{walk_expr, Visitor};
use rustc_hir::{Expr, ExprKind};
use rustc_middle::ty::{Instance, TyCtxt, TypeVisitableExt};
use rustc_span::def_id::LocalDefId;
use rustc_span::Span;
use std::collections::HashSet;

pub(crate) struct Instantiation<'tcx> {
    pub(crate) instance: Instance<'tcx>,
    pub(crate) caller: LocalDefId,
    pub(crate) span: Span,
    pub(crate) enumerated: bool,
}

pub(crate) fn collect<'tcx>(tcx: TyCtxt<'tcx>, owners: &[LocalDefId]) -> Vec<Instantiation<'tcx>> {
    let mut result = Vec::new();
    for owner in owners {
        if !crate::production(tcx, *owner) {
            continue;
        }
        let mut findings = Findings::default();
        Collector {
            analysis: Analysis { tcx, typeck: tcx.typeck(*owner), owner: *owner,
                arguments: None, flow: flow::Flow::default(), findings: &mut findings },
            caller: *owner,
            origin: None,
            seen: HashSet::new(),
            result: &mut result,
        }.visit_body(tcx.hir_body_owned_by(*owner));
    }
    result
}

struct Collector<'a, 'b, 'tcx> {
    analysis: Analysis<'a, 'tcx>,
    caller: LocalDefId,
    origin: Option<Span>,
    seen: HashSet<Instance<'tcx>>,
    result: &'b mut Vec<Instantiation<'tcx>>,
}

impl<'tcx> Visitor<'tcx> for Collector<'_, '_, 'tcx> {
    fn visit_expr(&mut self, expression: &'tcx Expr<'tcx>) {
        if let Some((definition, _)) = self.analysis.call(expression) {
            if let Some(instance) = self.analysis.resolved_instance(expression, definition) {
                if self.analysis.checked_body(instance.def_id())
                    && instance.args.iter().any(|argument| !matches!(argument.kind(), rustc_middle::ty::GenericArgKind::Lifetime(_))) && !instance.args.has_non_region_param()
                    && self.seen.insert(instance) {
                    let span = self.origin.unwrap_or(expression.span);
                    self.result.push(Instantiation { instance, caller: self.caller, span,
                        enumerated: self.seen.len() <= self.analysis.tcx.recursion_limit().0 });
                    if let Some(local) = instance.def_id().as_local() {
                        let tcx = self.analysis.tcx;
                        let limit = tcx.recursion_limit().0;
                        if self.seen.len() <= limit {
                            Collector {
                                analysis: Analysis { tcx, typeck: tcx.typeck(local), owner: local,
                                    arguments: Some(instance.args), flow: flow::Flow::default(),
                                    findings: self.analysis.findings },
                                caller: self.caller, origin: Some(span), seen: self.seen.clone(),
                                result: self.result,
                            }.visit_body(tcx.hir_body_owned_by(local));
                        }
                    }
                }
            }
        }
        if !matches!(expression.kind, ExprKind::Closure(_)) {
            walk_expr(self, expression);
        }
    }
}

pub(crate) fn check_imported<'tcx>(tcx: TyCtxt<'tcx>, root: &Instantiation<'tcx>, findings: &mut Findings) {
    let mut pending = vec![root.instance];
    let mut seen = HashSet::new();
    while let Some(instance) = pending.pop() {
        if !seen.insert(instance) {
            continue;
        }
        let mut reporter = Analysis { tcx, typeck: tcx.typeck(root.caller), owner: root.caller,
            arguments: None, flow: flow::Flow::default(), findings };
        if seen.len() > tcx.recursion_limit().0 || !tcx.is_mir_available(instance.def_id()) {
            reporter.report(root.span, "unproven_decode_charge", "generic instantiations cannot be enumerated: checked dependency body unavailable or recursion limit reached");
            continue;
        }
        let body = tcx.instance_mir(instance.def);
        for block in body.basic_blocks.iter() {
            let rustc_middle::mir::TerminatorKind::Call { func, args, destination, .. } = &block.terminator().kind else {
                continue;
            };
            let raw = func.ty(body, tcx);
            if !raw.has_non_region_param() {
                continue;
            }
            let concrete = instance.instantiate_mir(tcx, rustc_middle::ty::EarlyBinder::bind(tcx, raw));
            let rustc_middle::ty::FnDef(definition, arguments) = concrete.kind() else {
                reporter.report(root.span, "unproven_decode_charge", "generic instantiation contains an indirect call");
                continue;
            };
            let Some(arguments) = arguments.no_bound_vars() else {
                reporter.report(root.span, "unproven_decode_charge", "generic instantiation has late-bound arguments");
                continue;
            };
            let resolved = Instance::try_resolve(tcx, rustc_middle::ty::TypingEnv::fully_monomorphized(),
                *definition, arguments).ok().flatten();
            let Some(resolved) = resolved else {
                reporter.report(root.span, "unproven_decode_charge", &format!("generic instantiation unresolved: {concrete}"));
                continue;
            };
            if matches!(resolved.def, rustc_middle::ty::InstanceKind::Virtual(_, _)) {
                reporter.report(root.span, "unproven_decode_charge", &format!("generic instantiation uses trait-object dispatch: {concrete}"));
                continue;
            }
            if reporter.checked_body(resolved.def_id()) {
                pending.push(resolved);
                continue;
            }
            if tcx.trait_of_assoc(*definition).is_none() {
                continue;
            }
            let receiver = args.first().map(|operand| instance.instantiate_mir(tcx,
                rustc_middle::ty::EarlyBinder::bind(tcx, operand.node.ty(body, tcx))));
            let Some(summary) = crate::external::summary(tcx, resolved.def_id(), receiver) else {
                reporter.report(root.span, "unproven_decode_charge", &format!("generic external operation missing summary: {concrete}"));
                continue;
            };
            let Some(receiver) = receiver else { continue; };
            let allocation = if summary.allocation == crate::external::Allocation::Clone {
                let output = instance.instantiate_mir(tcx, rustc_middle::ty::EarlyBinder::bind(tcx, destination.ty(body, tcx).ty));
                reporter.clone_shape(output)
            } else if summary.allocation == crate::external::Allocation::None {
                crate::types::Shape::Fixed
            } else {
                crate::types::heap(tcx, receiver.peel_refs(), &mut Vec::new())
            };
            let work = if summary.work == crate::external::Work::Fixed
                || summary.allocation == crate::external::Allocation::Clone && allocation == crate::types::Shape::Fixed {
                crate::types::Shape::Fixed
            } else {
                crate::types::work(tcx, receiver, &mut Vec::new())
            };
            for (shape, rule) in [(allocation, "uncharged_decode_allocation"), (work, "uncharged_decode_work")] {
                if shape != crate::types::Shape::Fixed {
                    reporter.report(root.span,
                        if shape == crate::types::Shape::Unknown { "unproven_decode_charge" } else { rule },
                        &format!("concrete instantiation <{receiver}> of {} reaches {}",
                            tcx.def_path_str(root.instance.def_id()), tcx.def_path_str(resolved.def_id())));
                }
            }
        }
    }
}
