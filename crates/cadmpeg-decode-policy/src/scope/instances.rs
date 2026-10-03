// SPDX-License-Identifier: Apache-2.0
//! Concrete generic edges, including checked dependency MIR.
use super::{indirect, key, objects, EdgeKind, Graph};
use crate::types;
use rustc_middle::mir::visit::Visitor;
use rustc_middle::mir::{self, Body, Location, Operand, Rvalue};
use rustc_middle::ty::{self, Instance, Ty, TyCtxt, TypeVisitableExt};
use rustc_span::def_id::DefId;
use std::collections::HashSet;

pub(super) struct Concrete<'tcx> {
    pub(super) caller: String,
    pub(super) instance: Instance<'tcx>,
    pub(super) environment: ty::TypingEnv<'tcx>,
    pub(super) depth: usize,
}

pub(super) fn enqueue<'tcx>(
    tcx: TyCtxt<'tcx>,
    graph: &mut Graph,
    pending: &mut Vec<Concrete<'tcx>>,
    caller: &str,
    instance: Instance<'tcx>,
    environment: ty::TypingEnv<'tcx>,
    route: (usize, EdgeKind),
) {
    let (depth, kind) = route;
    let id = instance.def_id();
    if !types::checked(tcx, id)
        || types::serde_body(tcx, id)
        || matches!(instance.def, ty::InstanceKind::Virtual(..))
    {
        return;
    }
    graph.edges.insert((caller.to_owned(), key(tcx, id), kind));
    if instance.args.has_non_region_param() {
        graph
            .symbolic_edges
            .insert((caller.to_owned(), key(tcx, id)));
        return;
    }
    if instance.args.has_escaping_bound_vars() {
        return;
    }
    if instance
        .args
        .iter()
        .any(|argument| !matches!(argument.kind(), ty::GenericArgKind::Lifetime(_)))
    {
        pending.push(Concrete {
            caller: caller.to_owned(),
            instance,
            environment,
            depth,
        });
    }
}

pub(super) fn expand<'tcx>(tcx: TyCtxt<'tcx>, graph: &mut Graph, mut pending: Vec<Concrete<'tcx>>) {
    let mut seen = HashSet::new();
    while let Some(concrete) = pending.pop() {
        if !seen.insert((
            concrete.caller.clone(),
            concrete.instance,
            concrete.environment,
        )) {
            continue;
        }
        if concrete.depth > tcx.recursion_limit().0
            || !tcx.is_mir_available(concrete.instance.def_id())
        {
            continue;
        }
        let body = tcx.instance_mir(ty::InstanceKind::Item(concrete.instance.def_id()));
        Edges {
            tcx,
            graph,
            pending: &mut pending,
            concrete: &concrete,
            body,
        }
        .visit_body(body);
    }
}

struct Edges<'a, 'tcx> {
    tcx: TyCtxt<'tcx>,
    graph: &'a mut Graph,
    pending: &'a mut Vec<Concrete<'tcx>>,
    concrete: &'a Concrete<'tcx>,
    body: &'tcx Body<'tcx>,
}

impl<'tcx> Edges<'_, 'tcx> {
    fn value(&mut self, value: Ty<'tcx>) -> Option<Ty<'tcx>> {
        self.concrete
            .instance
            .try_instantiate_mir_and_normalize_erasing_regions(
                self.tcx,
                self.concrete.environment,
                ty::EarlyBinder::bind(self.tcx, value),
            )
            .ok()
    }

    fn target(&mut self, id: DefId, args: ty::GenericArgsRef<'tcx>, address: bool) {
        let instance = match Instance::try_resolve(self.tcx, self.concrete.environment, id, args) {
            Ok(Some(instance)) if !matches!(instance.def, ty::InstanceKind::Virtual(..)) => {
                instance
            }
            _ => {
                if address {
                    let value = Ty::new_fn_def(self.tcx, id, ty::Binder::dummy(args));
                    indirect::address(
                        self.tcx,
                        self.graph,
                        self.concrete.environment,
                        &self.concrete.caller,
                        value,
                        value,
                    );
                }
                if matches!(self.tcx.def_kind(id), rustc_hir::def::DefKind::AssocFn)
                    && self.tcx.trait_of_assoc(id).is_some()
                {
                    let method = key(self.tcx, id);
                    let signature =
                        indirect::method_signature(self.tcx, self.concrete.environment, id, args);
                    self.graph.trait_calls.insert((
                        self.concrete.caller.clone(),
                        method.clone(),
                        signature.clone(),
                    ));
                    if args
                        .types()
                        .next()
                        .is_some_and(|value| matches!(value.peel_refs().kind(), ty::Dynamic(..)))
                    {
                        self.graph.object_calls.insert((
                            self.concrete.caller.clone(),
                            method,
                            signature,
                        ));
                    }
                }
                return;
            }
        };
        if address {
            let value = Ty::new_fn_def(
                self.tcx,
                instance.def_id(),
                ty::Binder::dummy(instance.args),
            );
            indirect::address(
                self.tcx,
                self.graph,
                self.concrete.environment,
                &self.concrete.caller,
                value,
                value,
            );
        }
        enqueue(
            self.tcx,
            self.graph,
            self.pending,
            &self.concrete.caller,
            instance,
            self.concrete.environment,
            (
                self.concrete.depth + 1,
                if address {
                    EdgeKind::FunctionAddress
                } else {
                    EdgeKind::GenericInstantiation
                },
            ),
        );
    }
}

impl<'tcx> Visitor<'tcx> for Edges<'_, 'tcx> {
    fn visit_operand(&mut self, operand: &Operand<'tcx>, location: Location) {
        if let Some(value) = self.value(operand.ty(self.body, self.tcx)) {
            if let ty::FnDef(id, args) = value.kind() {
                let direct_call = matches!(&self.body.basic_blocks[location.block].terminator().kind,
                    mir::TerminatorKind::Call { func, .. } if std::ptr::eq(func, operand));
                if let Some(args) = args.no_bound_vars() {
                    self.target(*id, args, !direct_call);
                }
            }
        }
        if let Operand::Constant(constant) = operand {
            if let mir::Const::Unevaluated(value, _) = constant.const_ {
                let args = self
                    .concrete
                    .instance
                    .try_instantiate_mir_and_normalize_erasing_regions(
                        self.tcx,
                        self.concrete.environment,
                        ty::EarlyBinder::bind(self.tcx, value.args),
                    );
                if let Ok(args) = args {
                    self.target(value.def, args, false);
                }
            }
        }
        self.super_operand(operand, location);
    }

    fn visit_rvalue(&mut self, value: &Rvalue<'tcx>, location: Location) {
        match value {
            Rvalue::Cast(
                mir::CastKind::PointerCoercion(ty::adjustment::PointerCoercion::Unsize, _),
                operand,
                target,
            ) => {
                let source = self.value(operand.ty(self.body, self.tcx));
                let target = self.value(*target);
                if let (Some(source), Some(target)) = (source, target) {
                    let mut instances = Vec::new();
                    objects::targets(
                        self.tcx,
                        self.concrete.environment,
                        source,
                        target,
                        &mut instances,
                    );
                    for target in instances {
                        objects::register(
                            self.tcx,
                            self.graph,
                            self.pending,
                            &self.concrete.caller,
                            target,
                            self.concrete.environment,
                            self.concrete.depth + 1,
                        );
                    }
                }
            }
            Rvalue::Cast(_, operand, target) => {
                if let (Some(source), Some(target)) = (
                    self.value(operand.ty(self.body, self.tcx)),
                    self.value(*target),
                ) {
                    if matches!(target.kind(), ty::FnPtr(..)) {
                        indirect::address(
                            self.tcx,
                            self.graph,
                            self.concrete.environment,
                            &self.concrete.caller,
                            source,
                            target,
                        );
                    }
                }
            }
            Rvalue::Aggregate(kind, _) => {
                if let mir::AggregateKind::Closure(id, args) = **kind {
                    let value = Ty::new_closure(self.tcx, id, args);
                    if let Some(value) = self.value(value) {
                        if let ty::Closure(id, args) = value.kind() {
                            indirect::address(
                                self.tcx,
                                self.graph,
                                self.concrete.environment,
                                &self.concrete.caller,
                                value,
                                value,
                            );
                            enqueue(
                                self.tcx,
                                self.graph,
                                self.pending,
                                &self.concrete.caller,
                                Instance::new_raw(*id, args),
                                self.concrete.environment,
                                (self.concrete.depth + 1, EdgeKind::FunctionAddress),
                            );
                        }
                    }
                }
            }
            _ => (),
        }
        self.super_rvalue(value, location);
    }

    fn visit_terminator(&mut self, terminator: &mir::Terminator<'tcx>, location: Location) {
        if let mir::TerminatorKind::Call { func, .. } = &terminator.kind {
            if let Some(value) = self.value(func.ty(self.body, self.tcx)) {
                if matches!(value.kind(), ty::FnPtr(..)) {
                    if let Some(signature) =
                        indirect::signature(self.tcx, self.concrete.environment, value)
                    {
                        self.graph
                            .pointer_calls
                            .insert((self.concrete.caller.clone(), signature));
                    }
                }
            }
        }
        self.super_terminator(terminator, location);
    }
}
