// SPDX-License-Identifier: Apache-2.0
use crate::{external, flow, types, Analysis, Findings};
use rustc_hir::intravisit::{walk_expr, Visitor};
use rustc_hir::{Expr, ExprKind};
use rustc_middle::ty::{Instance, TyCtxt, TypeVisitableExt};
use rustc_span::def_id::LocalDefId;
use rustc_span::Span;
use std::collections::{HashSet, VecDeque};

pub(crate) struct Instantiation<'tcx> {
    pub(crate) instance: Instance<'tcx>,
    pub(crate) caller: LocalDefId,
    pub(crate) span: Span,
    pub(crate) enumerated: bool,
    pub(crate) fixed_operands: Vec<bool>,
    pub(crate) admitted_operands: Vec<bool>,
    pub(crate) admitted_parameters: HashSet<rustc_hir::HirId>,
    pub(crate) fixed_parameters: HashSet<rustc_hir::HirId>,
}

fn shared_identity_grammar(tcx: TyCtxt<'_>, definition: rustc_span::def_id::DefId) -> bool {
    [
        (&["ids", "IdentityComponent"][..], "try_new"),
        (&["ids", "IdentityKey"][..], "try_new"),
        (&["ids", "IdentityKeyTail"][..], "try_new"),
        (&["ids", "IdentityNamespace"][..], "new"),
    ]
    .iter()
    .any(|(owner, method)| {
        types::physical_inherent_method(tcx, definition, "cadmpeg_ir", owner, method)
    })
}

fn immutable_parameter_origin(
    body: &rustc_middle::mir::Body<'_>,
    operand: &rustc_middle::mir::Operand<'_>,
    active: &mut HashSet<rustc_middle::mir::Local>,
) -> Option<usize> {
    use rustc_middle::mir::{BorrowKind, Operand, ProjectionElem, Rvalue, StatementKind};
    let place = match operand {
        Operand::Copy(place) | Operand::Move(place) => *place,
        Operand::Constant(_) | Operand::RuntimeChecks(_) => return None,
    };
    if !place
        .projection
        .iter()
        .all(|projection| matches!(projection, ProjectionElem::Deref))
        || !active.insert(place.local)
    {
        return None;
    }
    let assignments: Vec<_> = body
        .basic_blocks
        .iter()
        .flat_map(|block| &block.statements)
        .filter_map(|statement| match &statement.kind {
            StatementKind::Assign(value) if value.0.local == place.local => Some(value),
            _ => None,
        })
        .collect();
    if body.basic_blocks.iter().any(|block| {
        matches!(&block.terminator().kind,
            rustc_middle::mir::TerminatorKind::Call { destination, .. }
                if destination.local == place.local)
    }) {
        return None;
    }
    let parameter = place.local.as_usize();
    if parameter > 0 && parameter <= body.arg_count {
        return assignments.is_empty().then_some(parameter);
    }
    let [assignment] = assignments.as_slice() else {
        return None;
    };
    if !assignment.0.projection.is_empty() {
        return None;
    }
    match &assignment.1 {
        Rvalue::Use(source, _) => immutable_parameter_origin(body, source, active),
        Rvalue::Ref(_, BorrowKind::Shared, source) => {
            immutable_parameter_origin(body, &Operand::Copy(*source), active)
        }
        _ => None,
    }
}

// The charged comparison owns the standard reference forwarding edge alone.
// Concrete child comparison and cost callbacks keep their own body proofs.
fn charged_reference_equality<'tcx>(
    tcx: TyCtxt<'tcx>,
    environment: rustc_middle::ty::TypingEnv<'tcx>,
    instance: Instance<'tcx>,
    body: &rustc_middle::mir::Body<'tcx>,
    comparison: rustc_middle::mir::BasicBlock,
    definition: rustc_span::def_id::DefId,
    receiver: rustc_middle::ty::Ty<'tcx>,
) -> Option<bool> {
    use rustc_middle::mir::{Operand, TerminatorKind};
    if !types::decode_context_method(tcx, instance.def_id(), "equal")
        || body.arg_count != 4
        || tcx
            .opt_item_name(definition)
            .is_none_or(|name| name.as_str() != "eq")
        || tcx.trait_of_assoc(definition).is_none_or(|owner| {
            !types::physical_item_path(tcx, owner, "core", &["cmp", "PartialEq"])
        })
        || !matches!(receiver.kind(), rustc_middle::ty::Ref(..))
        || crate::conversion::cyclic(body)
    {
        return None;
    }
    let TerminatorKind::Call { args: compared, .. } =
        &body.basic_blocks[comparison].terminator().kind
    else {
        return None;
    };
    if compared.len() != 2
        || immutable_parameter_origin(body, &compared[0].node, &mut HashSet::new()) != Some(2)
        || immutable_parameter_origin(body, &compared[1].node, &mut HashSet::new()) != Some(3)
    {
        return None;
    }
    let dominators = body.basic_blocks.dominators();
    if !dominators.is_reachable(comparison) {
        return None;
    }
    let mut charged = HashSet::new();
    for (block, data) in body.basic_blocks.iter_enumerated() {
        let TerminatorKind::Call {
            func,
            args,
            target: Some(target),
            ..
        } = &data.terminator().kind
        else {
            continue;
        };
        let rustc_middle::ty::FnDef(method, _) = func.ty(body, tcx).kind() else {
            continue;
        };
        if !types::decode_context_method(tcx, *method, "charge_key") {
            continue;
        }
        if args.len() != 4
            || !dominators.dominates(block, comparison)
            || !dominators.dominates(*target, comparison)
            || immutable_parameter_origin(body, &args[0].node, &mut HashSet::new()) != Some(1)
            || immutable_parameter_origin(body, &args[3].node, &mut HashSet::new()) != Some(4)
        {
            return None;
        }
        let Some(parameter) = immutable_parameter_origin(body, &args[1].node, &mut HashSet::new())
        else {
            return None;
        };
        if !matches!(parameter, 2 | 3) || !charged.insert(parameter) {
            return None;
        }
        let Operand::Constant(factor) = &args[2].node else {
            return None;
        };
        if instance
            .instantiate_mir(tcx, rustc_middle::ty::EarlyBinder::bind(tcx, factor.const_))
            .try_eval_bits(tcx, environment)
            != Some(1)
        {
            return None;
        }
    }
    (charged.len() == 2).then(|| crate::hash_set_callbacks::bounded_equality(tcx, receiver))
}

fn standard_vec(tcx: TyCtxt<'_>, value: rustc_middle::ty::Ty<'_>) -> bool {
    matches!(value.peel_refs().kind(), rustc_middle::ty::TyKind::Adt(owner, _)
        if types::physical_item_path(tcx, owner.did(), "alloc", &["vec", "Vec"]))
}

pub(crate) fn collect<'tcx>(tcx: TyCtxt<'tcx>, owners: &[LocalDefId]) -> Vec<Instantiation<'tcx>> {
    let mut result = Vec::new();
    for owner in owners {
        if !crate::production(tcx, owner.to_def_id()) {
            continue;
        }
        let mut findings = Findings::default();
        Analysis {
            tcx,
            typeck: tcx.typeck(*owner),
            typing_owner: *owner,
            arguments: None,
            fixed_parameters: HashSet::new(),
            flow: flow::Flow::default(),
            findings: &mut findings,
        }
        .visit_body(tcx.hir_body_owned_by(*owner));
        Collector {
            analysis: Analysis {
                tcx,
                typeck: tcx.typeck(*owner),
                typing_owner: *owner,
                arguments: None,
                fixed_parameters: HashSet::new(),
                flow: flow::Flow::default(),
                findings: &mut findings,
            },
            caller: *owner,
            origin: None,
            seen: HashSet::new(),
            result: &mut result,
        }
        .visit_body(tcx.hir_body_owned_by(*owner));
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
        if self
            .analysis
            .findings
            .admitted_operations
            .contains(&expression.hir_id)
            && self
                .analysis
                .call(expression)
                .is_some_and(|(definition, _)| {
                    shared_identity_grammar(self.analysis.tcx, definition)
                })
        {
            walk_expr(self, expression);
            self.analysis.mutation(expression);
            return;
        }
        let closure_type = match expression.kind {
            ExprKind::Call(callee, _) => Some(self.analysis.expr_ty(callee)),
            ExprKind::Closure(_) => Some(self.analysis.expr_ty(expression)),
            _ => None,
        };
        let instance = closure_type
            .and_then(|value| match value.peel_refs().kind() {
                rustc_middle::ty::Closure(definition, args) => Some(Instance {
                    def: rustc_middle::ty::InstanceKind::Item(*definition),
                    args,
                }),
                _ => None,
            })
            .or_else(|| {
                self.analysis
                    .call(expression)
                    .map(|(definition, _)| definition)
                    .or_else(|| {
                        matches!(
                            expression.kind,
                            ExprKind::Binary(..)
                                | ExprKind::Unary(..)
                                | ExprKind::Index(..)
                                | ExprKind::AssignOp(..)
                        )
                        .then(|| {
                            self.analysis
                                .typeck
                                .type_dependent_def_id(expression.hir_id)
                        })
                        .flatten()
                    })
                    .and_then(|definition| self.analysis.resolved_instance(expression, definition))
            });
        if let Some(instance) = instance {
            if self.analysis.checked_body(instance.def_id())
                && instance.args.iter().any(|argument| {
                    !matches!(
                        argument.kind(),
                        rustc_middle::ty::GenericArgKind::Lifetime(_)
                    )
                })
                && !instance.args.has_non_region_param()
                && self.seen.insert(instance)
            {
                let span = self.origin.unwrap_or(expression.span);
                let fixed_operands: Vec<_> =
                    self.analysis
                        .call(expression)
                        .map_or_else(Vec::new, |(_, operands)| {
                            operands
                                .into_iter()
                                .map(|operand| self.analysis.constant(operand, &mut Vec::new()))
                                .collect()
                        });
                let mut fixed_parameters = instance
                    .def_id()
                    .as_local()
                    .map_or_else(HashSet::new, |local| {
                        crate::fixed::bindings(self.analysis.tcx, local, &fixed_operands)
                    });
                if matches!(
                    self.analysis.tcx.def_kind(instance.def_id()),
                    rustc_hir::def::DefKind::Closure
                ) {
                    fixed_parameters.extend(self.analysis.fixed_parameters.iter().copied());
                }
                let admitted_operands = self
                    .analysis
                    .findings
                    .conversions
                    .get(&expression.hir_id)
                    .cloned()
                    .unwrap_or_default();
                let admitted_parameters = instance
                    .def_id()
                    .as_local()
                    .map_or_else(HashSet::new, |local| {
                        crate::fixed::bindings(self.analysis.tcx, local, &admitted_operands)
                    });
                self.result.push(Instantiation {
                    instance,
                    caller: self.caller,
                    span,
                    enumerated: self.seen.len() <= self.analysis.tcx.recursion_limit().0,
                    fixed_operands,
                    admitted_operands,
                    admitted_parameters: admitted_parameters.clone(),
                    fixed_parameters: fixed_parameters.clone(),
                });
                if let Some(local) = instance.def_id().as_local() {
                    let tcx = self.analysis.tcx;
                    let limit = tcx.recursion_limit().0;
                    if self.seen.len() <= limit {
                        let mut findings = Findings::default();
                        Analysis {
                            tcx,
                            typeck: tcx.typeck(local),
                            typing_owner: self.caller,
                            arguments: Some(instance.args),
                            fixed_parameters: fixed_parameters.clone(),
                            flow: flow::Flow::with_parameters(&admitted_parameters),
                            findings: &mut findings,
                        }
                        .visit_body(tcx.hir_body_owned_by(local));
                        Collector {
                            analysis: Analysis {
                                tcx,
                                typeck: tcx.typeck(local),
                                typing_owner: self.caller,
                                arguments: Some(instance.args),
                                fixed_parameters,
                                flow: flow::Flow::default(),
                                findings: &mut findings,
                            },
                            caller: self.caller,
                            origin: Some(span),
                            seen: self.seen.clone(),
                            result: self.result,
                        }
                        .visit_body(tcx.hir_body_owned_by(local));
                    }
                }
                self.seen.remove(&instance);
            }
        }
        if !matches!(expression.kind, ExprKind::Closure(_)) {
            walk_expr(self, expression);
            self.analysis.mutation(expression);
        }
    }
}

fn array_origin<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &rustc_middle::mir::Body<'tcx>,
    operand: &rustc_middle::mir::Operand<'tcx>,
    seen: &mut HashSet<rustc_middle::mir::Local>,
) -> Option<rustc_middle::ty::Ty<'tcx>> {
    let value = operand.ty(body, tcx).peel_refs();
    if matches!(value.kind(), rustc_middle::ty::Array(..)) {
        return Some(value);
    }
    let place = match operand {
        rustc_middle::mir::Operand::Copy(place) | rustc_middle::mir::Operand::Move(place) => *place,
        _ => return None,
    };
    if place.projection.len() == 1
        && matches!(
            place.projection[0],
            rustc_middle::mir::ProjectionElem::Deref
        )
    {
        return array_origin(
            tcx,
            body,
            &rustc_middle::mir::Operand::Copy(place.local.into()),
            seen,
        );
    }
    if !place.projection.is_empty() || !seen.insert(place.local) {
        return None;
    }
    let mut origin = None;
    for block in body.basic_blocks.iter() {
        if matches!(&block.terminator().kind, rustc_middle::mir::TerminatorKind::Call { destination, .. }
            if *destination == place)
        {
            return None;
        }
        for statement in &block.statements {
            let rustc_middle::mir::StatementKind::Assign(assignment) = &statement.kind else {
                continue;
            };
            let (target, value) = &**assignment;
            if matches!(value, rustc_middle::mir::Rvalue::Ref(_, rustc_middle::mir::BorrowKind::Mut { .. }, source)
                if source.local == place.local)
            {
                return None;
            }
            if *target != place {
                continue;
            }
            let candidate = match value {
                rustc_middle::mir::Rvalue::Use(source, _) => {
                    array_origin(tcx, body, source, &mut seen.clone())
                }
                rustc_middle::mir::Rvalue::Cast(
                    rustc_middle::mir::CastKind::PointerCoercion(
                        rustc_middle::ty::adjustment::PointerCoercion::Unsize,
                        _,
                    ),
                    source,
                    _,
                ) => array_origin(tcx, body, source, &mut seen.clone()),
                rustc_middle::mir::Rvalue::Ref(_, _, source) => array_origin(
                    tcx,
                    body,
                    &rustc_middle::mir::Operand::Copy(*source),
                    &mut seen.clone(),
                ),
                _ => None,
            }?;
            origin = Some(candidate);
        }
    }
    origin
}

pub(crate) fn check_imported<'tcx>(
    tcx: TyCtxt<'tcx>,
    root: &Instantiation<'tcx>,
    findings: &mut Findings,
) {
    let key_work_proofs: HashSet<String> = std::env::var_os("CADMPEG_POLICY_KEY_WORK_PROOFS")
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map_or_else(HashSet::new, |source| {
            source.lines().map(str::to_owned).collect()
        });
    let recursion_limit = tcx.recursion_limit().0;
    let Some(state_limit) = recursion_limit.checked_mul(recursion_limit) else {
        Analysis {
            tcx,
            typeck: tcx.typeck(root.caller),
            typing_owner: root.caller,
            arguments: None,
            fixed_parameters: HashSet::new(),
            flow: flow::Flow::default(),
            findings,
        }
        .report(
            root.span,
            "unproven_decode_charge",
            "generic instantiation state cap overflows the squared compiler recursion limit",
        );
        return;
    };
    if state_limit == 0 {
        Analysis {
            tcx,
            typeck: tcx.typeck(root.caller),
            typing_owner: root.caller,
            arguments: None,
            fixed_parameters: HashSet::new(),
            flow: flow::Flow::default(),
            findings,
        }
        .report(
            root.span,
            "unproven_decode_charge",
            "generic instantiation state cap is zero",
        );
        return;
    }
    let initial = (
        root.instance,
        root.fixed_operands.clone(),
        root.admitted_operands.clone(),
    );
    let mut scheduled = HashSet::from([initial.clone()]);
    let mut pending = VecDeque::from([(
        root.instance,
        root.fixed_operands.clone(),
        root.admitted_operands.clone(),
        0_usize,
    )]);
    let mut state_limit_reported = false;
    while let Some((instance, fixed_operands, admitted_operands, depth)) = pending.pop_front() {
        let mut reporter = Analysis {
            tcx,
            typeck: tcx.typeck(root.caller),
            typing_owner: root.caller,
            arguments: None,
            fixed_parameters: HashSet::new(),
            flow: flow::Flow::default(),
            findings,
        };
        if depth > recursion_limit {
            reporter.report(
                root.span,
                "unproven_decode_charge",
                "generic instantiation chain exceeds the compiler recursion limit",
            );
            continue;
        }
        if !tcx.is_mir_available(instance.def_id()) {
            reporter.report(
                root.span,
                "unproven_decode_charge",
                "generic instantiations cannot be enumerated: checked dependency body unavailable",
            );
            continue;
        }
        let body = tcx.instance_mir(instance.def);
        let has_context = body
            .local_decls
            .iter()
            .any(|local| types::has_context(tcx, local.ty, &mut Vec::new()));
        let has_work_charge = body.basic_blocks.iter().any(|block| {
            let rustc_middle::mir::TerminatorKind::Call { func, .. } = &block.terminator().kind else { return false; };
            matches!(func.ty(body, tcx).kind(), rustc_middle::ty::FnDef(id, _)
                if tcx.crate_name(id.krate).as_str() == "cadmpeg_core"
                    && tcx.opt_item_name(*id).is_some_and(|name| matches!(name.as_str(), "charge_work" | "charge_work_limit")))
        });
        for (block_index, block) in body.basic_blocks.iter_enumerated() {
            let rustc_middle::mir::TerminatorKind::Call {
                func,
                args,
                destination,
                ..
            } = &block.terminator().kind
            else {
                continue;
            };
            let raw = func.ty(body, tcx);
            if let rustc_middle::ty::FnDef(id, _) = raw.kind() {
                let name = tcx.opt_item_name(*id);
                let dependent = tcx.trait_of_assoc(*id).is_some()
                    || name.is_some_and(|name| {
                        matches!(
                            name.as_str(),
                            "to_vec"
                                | "to_owned"
                                | "clone"
                                | "from_elem"
                                | "resize"
                                | "resize_with"
                                | "contains_key"
                                | "contains"
                                | "get"
                                | "get_mut"
                                | "insert"
                                | "remove"
                                | "binary_search"
                                | "sort"
                                | "sort_unstable"
                        )
                    });
                if types::standard(tcx, *id) && !dependent {
                    continue;
                }
            }
            if !raw.has_non_region_param() {
                continue;
            }
            let concrete =
                instance.instantiate_mir(tcx, rustc_middle::ty::EarlyBinder::bind(tcx, raw));
            let rustc_middle::ty::FnDef(definition, arguments) = concrete.kind() else {
                reporter.report(
                    root.span,
                    "unproven_decode_charge",
                    "generic instantiation contains an indirect call",
                );
                continue;
            };
            let Some(arguments) = arguments.no_bound_vars() else {
                reporter.report(
                    root.span,
                    "unproven_decode_charge",
                    "generic instantiation has late-bound arguments",
                );
                continue;
            };
            let receiver = args.first().map(|operand| {
                instance.instantiate_mir(
                    tcx,
                    rustc_middle::ty::EarlyBinder::bind(tcx, operand.node.ty(body, tcx)),
                )
            });
            if let Some(bounded) = receiver.and_then(|receiver| {
                charged_reference_equality(
                    tcx,
                    reporter.typing_env(),
                    instance,
                    body,
                    block_index,
                    *definition,
                    receiver,
                )
            }) {
                if !bounded {
                    reporter.report(
                        root.span,
                        "unproven_decode_charge",
                        "charged comparison has an unproven concrete equality callback",
                    );
                }
                continue;
            }
            let query = args.get(1).map(|operand| {
                instance.instantiate_mir(
                    tcx,
                    rustc_middle::ty::EarlyBinder::bind(tcx, operand.node.ty(body, tcx)),
                )
            });
            let key_work_proof = key_work_proofs.contains(&crate::key_work::proof_key(
                tcx,
                instance.def_id(),
                block.terminator().source_info.span,
            ));
            let equal_hash_set =
                types::decode_context_method(tcx, instance.def_id(), "equal_hash_set");
            let normalized_lookup = receiver.zip(query).and_then(|(receiver, query)| {
                let receiver = tcx
                    .try_normalize_erasing_regions(
                        reporter.typing_env(),
                        rustc_middle::ty::Unnormalized::new_wip(receiver),
                    )
                    .ok()?;
                let query = tcx
                    .try_normalize_erasing_regions(
                        reporter.typing_env(),
                        rustc_middle::ty::Unnormalized::new_wip(query),
                    )
                    .ok()?;
                Some((receiver, query))
            });
            let raw_hash_lookup = crate::hash_set_callbacks::is_raw_hash_lookup(tcx, *definition);
            if raw_hash_lookup {
                let bounded_raw = normalized_lookup.and_then(|(receiver, query)| {
                    crate::hash_set_callbacks::bounded_raw_lookup(tcx, *definition, receiver, query)
                });
                match bounded_raw {
                    Some(true) => (),
                    Some(false) => {
                        reporter.report(
                            root.span,
                            "unproven_decode_charge",
                            "raw hash lookup has an unproven stored Borrow, key callback, or builder",
                        );
                        continue;
                    }
                    None => {
                        reporter.report(
                            root.span,
                            "unproven_decode_charge",
                            "raw hash lookup has unresolved receiver or query types",
                        );
                        continue;
                    }
                }
            }
            if crate::hash_set_callbacks::is_hash_lookup_context(tcx, instance.def_id()) {
                let concrete_lookup = normalized_lookup.and_then(|(receiver, query)| {
                    crate::hash_set_callbacks::bounded_context_lookup(
                        tcx,
                        instance.def_id(),
                        *definition,
                        receiver,
                        query,
                    )
                });
                match concrete_lookup {
                    Some(true) if equal_hash_set || key_work_proof => continue,
                    Some(false) => {
                        reporter.report(
                            root.span,
                            "unproven_decode_charge",
                            "hash collection lookup has an unproven stored Borrow, key callback, or builder",
                        );
                        continue;
                    }
                    None if key_work_proof
                        || equal_hash_set
                            && tcx
                                .opt_item_name(*definition)
                                .is_some_and(|name| name.as_str() == "contains") =>
                    {
                        reporter.report(
                            root.span,
                            "unproven_decode_charge",
                            "admitted hash-set lookup has unresolved callback or receiver types",
                        );
                        continue;
                    }
                    _ => (),
                }
            }
            let operation_owned = receiver.is_some_and(|receiver| {
                (types::decode_context_method(tcx, instance.def_id(), "extend_vec")
                    && tcx
                        .opt_item_name(*definition)
                        .is_some_and(|name| name.as_str() == "extend")
                    && tcx.trait_of_assoc(*definition).is_some_and(|trait_id| {
                        types::standard(tcx, trait_id)
                            && tcx.item_name(trait_id).as_str() == "Extend"
                    })
                    && standard_vec(tcx, receiver))
            });
            if operation_owned {
                continue;
            }
            let Ok(arguments) = tcx.try_normalize_erasing_regions(
                reporter.typing_env(),
                rustc_middle::ty::Unnormalized::new_wip(arguments),
            ) else {
                reporter.report(
                    root.span,
                    "unproven_decode_charge",
                    "generic instantiation arguments cannot be normalized",
                );
                continue;
            };
            let resolved =
                Instance::try_resolve(tcx, reporter.typing_env(), *definition, arguments)
                    .ok()
                    .flatten();
            let Some(resolved) = resolved else {
                reporter.report(
                    root.span,
                    "unproven_decode_charge",
                    &format!("generic instantiation unresolved: {concrete}"),
                );
                continue;
            };
            if matches!(resolved.def, rustc_middle::ty::InstanceKind::Virtual(_, _)) {
                reporter.report(
                    root.span,
                    "unproven_decode_charge",
                    &format!("generic instantiation uses trait-object dispatch: {concrete}"),
                );
                continue;
            }
            if crate::conversion::core_conversion_trait(tcx, *definition, "Into")
                && receiver.is_some_and(|source| {
                    let output = instance.instantiate_mir(
                        tcx,
                        rustc_middle::ty::EarlyBinder::bind(tcx, destination.ty(body, tcx).ty),
                    );
                    crate::conversion::forwarded_from(
                        tcx,
                        reporter.typing_env(),
                        resolved,
                        source,
                        output,
                    )
                    .is_some_and(|target| {
                        !types::standard(tcx, target.def_id())
                            && reporter.checked_body(target.def_id())
                    })
                })
            {
                continue;
            }
            if !types::standard(tcx, resolved.def_id()) && reporter.checked_body(resolved.def_id())
            {
                let fixed: Vec<bool> = args
                    .iter()
                    .map(|operand| {
                        crate::fixed::mir_operand(
                            tcx,
                            body,
                            &operand.node,
                            &fixed_operands,
                            &mut HashSet::new(),
                        )
                    })
                    .collect();
                let paid: Vec<bool> = args
                    .iter()
                    .map(|arg| {
                        crate::conversion::operand_admitted(body, &arg.node, &admitted_operands)
                    })
                    .collect();
                let next_state = (resolved, fixed.clone(), paid.clone());
                if scheduled.contains(&next_state) {
                    continue;
                }
                let Some(next_depth) = depth.checked_add(1) else {
                    reporter.report(
                        root.span,
                        "unproven_decode_charge",
                        "generic instantiation chain depth overflow",
                    );
                    continue;
                };
                if next_depth > recursion_limit {
                    reporter.report(
                        root.span,
                        "unproven_decode_charge",
                        "generic instantiation chain exceeds the compiler recursion limit",
                    );
                    continue;
                }
                if scheduled.len() >= state_limit {
                    if !state_limit_reported {
                        reporter.report(
                            root.span,
                            "unproven_decode_charge",
                            "generic instantiation state count exceeds the squared compiler recursion limit",
                        );
                        state_limit_reported = true;
                    }
                    continue;
                }
                scheduled.insert(next_state);
                pending.push_back((resolved, fixed, paid, next_depth));
                continue;
            }
            let receiver = args.first().map(|operand| {
                instance.instantiate_mir(
                    tcx,
                    rustc_middle::ty::EarlyBinder::bind(tcx, operand.node.ty(body, tcx)),
                )
            });
            if std::env::var_os("CADMPEG_POLICY_EXTERNALS").is_some()
                && !types::checked(tcx, resolved.def_id())
            {
                reporter.findings.externals.insert(external::inventory_row(
                    tcx,
                    resolved.def_id(),
                    receiver,
                ));
            }
            let Some(summary) = crate::external::summary(tcx, resolved.def_id(), receiver) else {
                reporter.report(
                    root.span,
                    "unproven_decode_charge",
                    &format!("generic external operation missing summary: {concrete}"),
                );
                continue;
            };
            let fixed_receiver = args.first().is_some_and(|operand| {
                crate::fixed::mir_operand(
                    tcx,
                    body,
                    &operand.node,
                    &fixed_operands,
                    &mut HashSet::new(),
                )
            });
            let fixed_extent = args
                .get(match summary.work {
                    external::Work::Argument(index) => index,
                    external::Work::TextCharacter => 1,
                    _ => 0,
                })
                .is_some_and(|operand| {
                    crate::fixed::mir_operand(
                        tcx,
                        body,
                        &operand.node,
                        &fixed_operands,
                        &mut HashSet::new(),
                    )
                });
            let Some(receiver) = receiver else {
                continue;
            };
            let raw_output = destination.ty(body, tcx).ty;
            let output =
                instance.instantiate_mir(tcx, rustc_middle::ty::EarlyBinder::bind(tcx, raw_output));
            let copy_conversion = crate::conversion::copy_conversion_is_fixed(
                tcx,
                reporter.typing_env(),
                *definition,
                resolved,
                receiver,
                output,
            );
            let raw_receiver = args.first().map(|operand| operand.node.ty(body, tcx));
            let count_index = match summary.allocation {
                external::Allocation::Capacity => Some(0),
                external::Allocation::Repeat => Some(1),
                _ => None,
            };
            let count = count_index
                .and_then(|index| args.get(index))
                .and_then(|operand| match &operand.node {
                    rustc_middle::mir::Operand::Constant(value) => instance
                        .instantiate_mir(
                            tcx,
                            rustc_middle::ty::EarlyBinder::bind(tcx, value.const_),
                        )
                        .try_eval_bits(tcx, reporter.typing_env()),
                    _ => None,
                });
            let operation_name = tcx.opt_item_name(resolved.def_id());
            if operation_name.is_some_and(|name| name.as_str() == "resize") {
                if let Some(fill) = args.get(2) {
                    let child = instance.instantiate_mir(
                        tcx,
                        rustc_middle::ty::EarlyBinder::bind(tcx, fill.node.ty(body, tcx)),
                    );
                    let fixed_fill = crate::fixed::mir_operand(
                        tcx,
                        body,
                        &fill.node,
                        &fixed_operands,
                        &mut HashSet::new(),
                    );
                    let shape = if fixed_fill {
                        types::Shape::Fixed
                    } else {
                        reporter.clone_shape(child)
                    };
                    if shape != types::Shape::Fixed {
                        for rule in ["uncharged_decode_allocation", "uncharged_decode_work"] {
                            reporter.report(root.span, if shape == types::Shape::Unknown || rule == "uncharged_decode_work" && has_work_charge { "unproven_decode_charge" } else { rule }, &format!("concrete instantiation <{child}> of {} reaches resize child Clone", tcx.def_path_str(root.instance.def_id())));
                        }
                    }
                }
                continue;
            }

            let array_iterator = raw_receiver.is_some_and(|value| match value.peel_refs().kind() {
                rustc_middle::ty::Adt(owner, _) => {
                    types::standard(tcx, owner.did())
                        && tcx.item_name(owner.did()).as_str() == "IntoIter"
                        && tcx.def_path_str(owner.did()).contains("array::")
                }
                _ => false,
            });
            let vector_output = matches!(output.kind(), rustc_middle::ty::Adt(owner, _)
                if types::standard(tcx, owner.did()) && tcx.item_name(owner.did()).as_str() == "Vec");
            let array_element = args
                .first()
                .and_then(|operand| array_origin(tcx, body, &operand.node, &mut HashSet::new()))
                .and_then(|value| match value.kind() {
                    rustc_middle::ty::Array(element, _) => Some(*element),
                    _ => None,
                });
            let allocation_shape = |output: rustc_middle::ty::Ty<'tcx>,
                                    receiver: rustc_middle::ty::Ty<'tcx>,
                                    concrete: bool| {
                match summary.allocation {
                    external::Allocation::None => types::Shape::Fixed,
                    external::Allocation::Input(index) => {
                        args.get(index).map_or(types::Shape::Unknown, |operand| {
                            let value = operand.node.ty(body, tcx);
                            let value = if concrete {
                                instance.instantiate_mir(
                                    tcx,
                                    rustc_middle::ty::EarlyBinder::bind(tcx, value),
                                )
                            } else {
                                value
                            };
                            types::work(tcx, value, &mut Vec::new())
                        })
                    }
                    external::Allocation::Clone if concrete => reporter.clone_shape(output),
                    external::Allocation::Collect
                        if vector_output
                            && (array_element.is_some() || array_iterator)
                            && operation_name.is_some_and(|name| {
                                matches!(name.as_str(), "collect" | "from_iter")
                            }) =>
                    {
                        types::Shape::Fixed
                    }
                    external::Allocation::Collect | external::Allocation::Result
                        if array_element.is_some()
                            && operation_name.is_some_and(|name| {
                                matches!(name.as_str(), "to_vec" | "to_owned")
                            }) =>
                    {
                        let Some(element) = array_element else {
                            return types::Shape::Unknown;
                        };
                        if concrete {
                            reporter.clone_shape(instance.instantiate_mir(
                                tcx,
                                rustc_middle::ty::EarlyBinder::bind(tcx, element),
                            ))
                        } else {
                            types::heap(tcx, element, &mut Vec::new())
                        }
                    }
                    external::Allocation::Growth => {
                        types::heap(tcx, receiver.peel_refs(), &mut Vec::new())
                    }
                    external::Allocation::Capacity if count.is_some() => types::Shape::Fixed,
                    external::Allocation::Repeat if count == Some(0) => types::Shape::Fixed,
                    external::Allocation::Repeat if count.is_some() => {
                        if concrete {
                            reporter.clone_shape(receiver.peel_refs())
                        } else {
                            types::heap(tcx, receiver.peel_refs(), &mut Vec::new())
                        }
                    }
                    external::Allocation::Conversion
                        if !concrete && receiver.has_non_region_param() =>
                    {
                        types::Shape::Unknown
                    }
                    external::Allocation::Conversion
                        if output == receiver
                            || matches!(
                                receiver.peel_refs().kind(),
                                rustc_middle::ty::Array(..)
                            ) =>
                    {
                        types::Shape::Fixed
                    }
                    _ => types::heap(tcx, output, &mut Vec::new()),
                }
            };
            let admitted_conversion = operation_name
                .is_some_and(|name| matches!(name.as_str(), "into" | "from" | "to_owned"))
                && args.first().is_some_and(|arg| {
                    crate::conversion::operand_admitted(body, &arg.node, &admitted_operands)
                });
            let fixed_conversion = admitted_conversion || copy_conversion;
            let allocation = if fixed_conversion
                || fixed_receiver
                    && matches!(
                        summary.allocation,
                        external::Allocation::Clone
                            | external::Allocation::Conversion
                            | external::Allocation::Result
                            | external::Allocation::Input(0)
                    ) {
                types::Shape::Fixed
            } else if (summary.work == external::Work::TextCharacter
                || operation_name.is_some_and(|name| name.as_str() == "write_str"))
                && types::standard_string(tcx, receiver)
            {
                types::heap(tcx, receiver.peel_refs(), &mut Vec::new())
            } else if summary.allocation == external::Allocation::Growth {
                // Backing slot bytes are proved in the generic body. Child
                // cloning and key traits remain concrete work obligations.
                types::Shape::Fixed
            } else {
                allocation_shape(output, receiver, true)
            };
            let string_fmt_write = summary.allocation == external::Allocation::Growth
                && operation_name
                    .is_some_and(|name| matches!(name.as_str(), "write_char" | "write_str"));
            // A generic fmt::Write receiver has no String allocation summary.
            let symbolic_formatter = string_fmt_write
                && raw_receiver.is_some_and(|receiver| !types::standard_string(tcx, receiver));
            let symbolic_allocation = if symbolic_formatter {
                types::Shape::Fixed
            } else {
                raw_receiver.map_or(types::Shape::Unknown, |receiver| {
                    allocation_shape(raw_output, receiver, false)
                })
            };
            // A consumer over a prepaid iterator was charged by the admission;
            // its own callbacks must still be checked bodies.
            let prepaid_consumer = summary.work == external::Work::Iterator
                && reporter.prepaid_iterator(receiver)
                && reporter.consumer_callbacks_checked(
                    &args
                        .iter()
                        .skip(1)
                        .map(|operand| {
                            let value = instance.instantiate_mir(
                                tcx,
                                rustc_middle::ty::EarlyBinder::bind(
                                    tcx,
                                    operand.node.ty(body, tcx),
                                ),
                            );
                            tcx.try_normalize_erasing_regions(
                                reporter.typing_env(),
                                rustc_middle::ty::Unnormalized::new_wip(value),
                            )
                            .unwrap_or(value)
                        })
                        .collect::<Vec<_>>(),
                );
            let index = match summary.work {
                external::Work::Argument(index) => index,
                external::Work::TextCharacter => 1,
                _ => 0,
            };
            let raw_extent = args.get(index).map(|operand| operand.node.ty(body, tcx));
            let work_shape = |extent: rustc_middle::ty::Ty<'tcx>, allocation, concrete, fixed| {
                if summary.work == external::Work::TextCharacter {
                    if fixed {
                        types::Shape::Fixed
                    } else {
                        types::Shape::Dynamic
                    }
                } else if concrete && (fixed || fixed_conversion || prepaid_consumer)
                    || summary.work == external::Work::Fixed
                    || summary.work == external::Work::Iterator
                        && vector_output
                        && (array_element.is_some() || array_iterator)
                        && operation_name
                            .is_some_and(|name| matches!(name.as_str(), "collect" | "from_iter"))
                    || summary.allocation == external::Allocation::Clone
                        && allocation == types::Shape::Fixed
                    || summary.work == external::Work::Repeat && count == Some(0)
                {
                    types::Shape::Fixed
                } else {
                    if summary.allocation == external::Allocation::Clone
                        && allocation == types::Shape::Unknown
                    {
                        return types::Shape::Unknown;
                    }
                    if tcx.opt_item_name(resolved.def_id()).is_some_and(|name| {
                        matches!(
                            name.as_str(),
                            "to_vec" | "to_owned" | "copy_from_slice" | "copy_within"
                        )
                    }) {
                        let element = match extent.peel_refs().kind() {
                            rustc_middle::ty::Array(element, _)
                            | rustc_middle::ty::Slice(element) => Some(*element),
                            _ => None,
                        };
                        if let Some(element) = element {
                            match types::slot_storage(tcx, element) {
                                types::Shape::Unknown => return types::Shape::Unknown,
                                types::Shape::Fixed
                                    if tcx.type_is_copy_modulo_regions(
                                        reporter.typing_env(),
                                        element,
                                    ) =>
                                {
                                    return types::Shape::Fixed
                                }
                                _ => (),
                            }
                        }
                        if let Some(element) = array_element {
                            let element = if concrete {
                                instance.instantiate_mir(
                                    tcx,
                                    rustc_middle::ty::EarlyBinder::bind(tcx, element),
                                )
                            } else {
                                element
                            };
                            if reporter.clone_shape(element) == types::Shape::Fixed {
                                return types::Shape::Fixed;
                            }
                        }
                    }
                    let child = if summary.work == external::Work::Iterator {
                        if array_element.is_some() {
                            types::Shape::Fixed
                        } else {
                            types::iteration(tcx, extent)
                        }
                    } else {
                        types::work(tcx, extent, &mut Vec::new())
                    };
                    if summary.work == external::Work::Repeat
                        && child != types::Shape::Unknown
                        && count.is_none()
                    {
                        child.join(types::Shape::Dynamic)
                    } else {
                        child
                    }
                }
            };
            let arguments_work = |indices: &[usize], concrete| {
                indices.iter().fold(types::Shape::Fixed, |shape, index| {
                    let Some(operand) = args.get(*index) else {
                        return shape.join(types::Shape::Unknown);
                    };
                    let raw = operand.node.ty(body, tcx);
                    let extent = if concrete {
                        instance.instantiate_mir(tcx, rustc_middle::ty::EarlyBinder::bind(tcx, raw))
                    } else {
                        raw
                    };
                    let fixed = crate::fixed::mir_operand(
                        tcx,
                        body,
                        &operand.node,
                        &fixed_operands,
                        &mut HashSet::new(),
                    );
                    shape.join(work_shape(
                        extent,
                        if concrete {
                            allocation
                        } else {
                            symbolic_allocation
                        },
                        concrete,
                        fixed,
                    ))
                })
            };
            // A String specialization adds UTF-8 work that the generic trait body does not see.
            let symbolic_work = if summary.work == external::Work::TextCharacter
                && raw_receiver.is_some_and(|receiver| !types::standard_string(tcx, receiver))
            {
                types::Shape::Fixed
            } else if let external::Work::Arguments(indices) = summary.work {
                arguments_work(indices, false)
            } else {
                raw_extent.map_or(types::Shape::Unknown, |extent| {
                    work_shape(extent, symbolic_allocation, false, fixed_extent)
                })
            };
            let work = if key_work_proofs.contains(&crate::key_work::proof_key(
                tcx,
                instance.def_id(),
                block.terminator().source_info.span,
            )) {
                types::Shape::Fixed
            } else if let external::Work::Arguments(indices) = summary.work {
                arguments_work(indices, true)
            } else {
                raw_extent.map_or(types::Shape::Unknown, |extent| {
                    work_shape(
                        instance
                            .instantiate_mir(tcx, rustc_middle::ty::EarlyBinder::bind(tcx, extent)),
                        allocation,
                        true,
                        fixed_extent,
                    )
                })
            };
            for (shape, symbolic, rule) in [
                (
                    allocation,
                    symbolic_allocation,
                    "uncharged_decode_allocation",
                ),
                (work, symbolic_work, "uncharged_decode_work"),
            ] {
                if shape != types::Shape::Fixed && symbolic != types::Shape::Dynamic {
                    let uncertain_admission = (rule == "uncharged_decode_allocation"
                        && summary.allocation == external::Allocation::Growth
                        && has_context)
                        || (rule == "uncharged_decode_work" && has_work_charge);
                    let shape = if uncertain_admission {
                        types::Shape::Unknown
                    } else {
                        shape
                    };
                    reporter.report(
                        root.span,
                        if shape == crate::types::Shape::Unknown {
                            "unproven_decode_charge"
                        } else {
                            rule
                        },
                        &format!(
                            "concrete instantiation <{receiver}> of {} reaches {}",
                            tcx.def_path_str(root.instance.def_id()),
                            tcx.def_path_str(resolved.def_id())
                        ),
                    );
                }
            }
        }
    }
}
