// SPDX-License-Identifier: Apache-2.0
//! Concrete fixed-record serializers admitted by structural projection.

use crate::{conversion, types};
use rustc_middle::mir::{
    self, BasicBlock, BorrowKind, Local, Operand, Place, Rvalue, StatementKind, TerminatorKind,
    UnwindAction, RETURN_PLACE,
};
use rustc_middle::ty::{self, Instance, Ty, TyCtxt, TypeVisitableExt};
use rustc_span::def_id::DefId;
use rustc_span::DUMMY_SP;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, PartialEq, Eq)]
enum CallKind {
    SerializeStruct,
    SerializeField,
    End,
    TryBranch,
    FromResidual,
    FixedHelper,
}

struct Call<'tcx> {
    kind: CallKind,
    block: BasicBlock,
    destination: Local,
    arguments: Vec<Operand<'tcx>>,
    helper: Option<Instance<'tcx>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Origin {
    Data { source: bool, fixed: bool },
    Serializer,
    OperationResult(Local),
    BranchResult(Local),
    BranchOutput(Local),
    BranchResidual(Local),
    BranchTag(Local),
}

/// Return the value types emitted by a fixed `SerializeStruct` implementation.
/// The projection owner must check each returned type with its existing tree
/// and emission state; this function proves only the serializer's MIR shape.
pub(crate) fn record_field_types<'tcx>(
    tcx: TyCtxt<'tcx>,
    source: Ty<'tcx>,
    method: DefId,
) -> Option<Vec<Ty<'tcx>>> {
    if !tcx.is_mir_available(method) {
        return None;
    }
    let instance = crate::structural_derived::serialize_instance(tcx, source, method)?;
    let concrete_body = crate::structural_derived::instantiated_body(tcx, instance);
    let body = &concrete_body;
    if body.arg_count != 2 || conversion::cyclic(body) {
        return None;
    }
    let receiver = body.local_decls[Local::from_usize(1)].ty;
    if !matches!(receiver.kind(), ty::Ref(_, inner, ty::Mutability::Not)
        if *inner == source.peel_refs())
    {
        return None;
    }

    let reachable = reachable_blocks(body);
    let mut calls = Vec::new();
    let mut cleanup_entries = Vec::new();
    for block in &reachable {
        match &body.basic_blocks[*block].terminator().kind {
            TerminatorKind::Call {
                func,
                args,
                destination,
                target: Some(_),
                unwind,
                ..
            } => {
                if !destination.projection.is_empty() {
                    return None;
                }
                let ty::FnDef(definition, generic_arguments) = func.ty(body, tcx).kind() else {
                    return None;
                };
                let (kind, helper) = match classify_call(tcx, *definition) {
                    Some(kind) => (kind, None),
                    None => (
                        CallKind::FixedHelper,
                        Some(resolve_fixed_helper(tcx, *definition, (*generic_arguments).no_bound_vars()?)?),
                    ),
                };
                let destination = destination.local;
                match kind {
                    CallKind::SerializeStruct
                    | CallKind::SerializeField
                    | CallKind::FixedHelper => {
                        if destination == RETURN_PLACE {
                            return None;
                        }
                    }
                    CallKind::End | CallKind::FromResidual => {
                        if destination != RETURN_PLACE {
                            return None;
                        }
                    }
                    CallKind::TryBranch if destination == RETURN_PLACE => return None,
                    CallKind::TryBranch => (),
                }
                calls.push(Call {
                    kind,
                    block: *block,
                    destination,
                    arguments: args.iter().map(|argument| argument.node.clone()).collect(),
                    helper,
                });
                if let UnwindAction::Cleanup(cleanup) = unwind {
                    cleanup_entries.push(*cleanup);
                }
            }
            TerminatorKind::SwitchInt { .. }
            | TerminatorKind::Goto { .. }
            | TerminatorKind::Drop { .. }
            | TerminatorKind::Return
            | TerminatorKind::Unreachable => (),
            _ => return None,
        }
    }

    let structure = exactly_one(&calls, CallKind::SerializeStruct)?;
    let end = exactly_one(&calls, CallKind::End)?;
    if structure.arguments.len() != 3
        || !parameter_operand(&structure.arguments[0], 2)
        || !static_str_operand(tcx, &structure.arguments[1])
    {
        return None;
    }
    let declared_fields = constant_usize(tcx, &structure.arguments[2])?;

    let mut fields: Vec<_> = calls
        .iter()
        .filter(|call| call.kind == CallKind::SerializeField)
        .collect();
    if u64::try_from(fields.len()).ok()? != declared_fields {
        return None;
    }
    let branches: Vec<_> = calls
        .iter()
        .filter(|call| call.kind == CallKind::TryBranch)
        .collect();
    let residuals: Vec<_> = calls
        .iter()
        .filter(|call| call.kind == CallKind::FromResidual)
        .collect();
    if branches.len() != fields.len() + 1 || residuals.len() != branches.len() {
        return None;
    }

    let dominators = dominators(body, &reachable);
    if !dominates(&dominators, structure.block, end.block)
        || fields
            .iter()
            .any(|field| !dominates(&dominators, structure.block, field.block))
        || fields
            .iter()
            .any(|field| !dominates(&dominators, field.block, end.block))
    {
        return None;
    }
    order_by_dominance(&mut fields, &dominators)?;

    let mut operation_destinations = HashSet::new();
    operation_destinations.insert(structure.destination);
    if fields
        .iter()
        .any(|field| !operation_destinations.insert(field.destination))
    {
        return None;
    }

    let mut branch_for_operation = HashMap::new();
    for branch in &branches {
        if branch.arguments.len() != 1 {
            return None;
        }
        let Origin::OperationResult(operation) = operand_origin(
            tcx,
            body,
            &branch.arguments[0],
            &reachable,
            &calls,
            &mut HashSet::new(),
        )?
        else {
            return None;
        };
        if !operation_destinations.contains(&operation)
            || branch_for_operation
                .insert(operation, branch.destination)
                .is_some()
        {
            return None;
        }
    }
    if branch_for_operation.len() != operation_destinations.len() {
        return None;
    }
    let state_branch = *branch_for_operation.get(&structure.destination)?;

    let mut switch_for_branch = HashMap::new();
    for block in &reachable {
        let TerminatorKind::SwitchInt { discr, targets } =
            &body.basic_blocks[*block].terminator().kind
        else {
            continue;
        };
        let Origin::BranchTag(branch) = operand_origin(
            tcx,
            body,
            discr,
            &reachable,
            &calls,
            &mut HashSet::new(),
        )?
        else {
            if fixed_bool_operand(tcx, body, discr, &reachable, &calls) {
                continue;
            }
            return None;
        };
        let (break_target, continue_target) = control_flow_targets(tcx, body, branch, targets)?;
        if switch_for_branch
            .insert(branch, (*block, break_target, continue_target))
            .is_some()
        {
            return None;
        }
    }
    if switch_for_branch.len() != branches.len() {
        return None;
    }

    let mut residual_for_branch = HashMap::new();
    for residual in &residuals {
        if residual.arguments.len() != 1 {
            return None;
        }
        let Origin::BranchResidual(branch) = operand_origin(
            tcx,
            body,
            &residual.arguments[0],
            &reachable,
            &calls,
            &mut HashSet::new(),
        )?
        else {
            return None;
        };
        if residual_for_branch.insert(branch, residual.block).is_some() {
            return None;
        }
    }
    if residual_for_branch.len() != branches.len() {
        return None;
    }

    if !cleanup_paths_are_bounded(
        tcx,
        body,
        &reachable,
        &cleanup_entries,
        &calls,
        structure,
        end,
        &branch_for_operation,
        &switch_for_branch,
        &branches,
    ) {
        return None;
    }

    let mut operations = Vec::with_capacity(fields.len() + 2);
    operations.push(structure);
    operations.extend(fields.iter().copied());
    operations.push(end);
    let mut field_types = Vec::with_capacity(fields.len());
    for operation_index in 0..operations.len() {
        let operation = operations[operation_index];
        match operation.kind {
            CallKind::SerializeStruct => {
                if operation.arguments.len() != 3
                    || !parameter_operand(&operation.arguments[0], 2)
                    || !static_str_operand(tcx, &operation.arguments[1])
                    || constant_usize(tcx, &operation.arguments[2])? != declared_fields
                {
                    return None;
                }
            }
            CallKind::SerializeField => {
                if operation.arguments.len() != 3
                    || !static_str_operand(tcx, &operation.arguments[1])
                    || operand_origin(
                        tcx,
                        body,
                        &operation.arguments[0],
                        &reachable,
                        &calls,
                        &mut HashSet::new(),
                    )? != Origin::BranchOutput(state_branch)
                {
                    return None;
                }
                let field_ty = operation.arguments[2].ty(body, tcx);
                if field_ty.has_non_region_param() {
                    return None;
                }
                if !matches!(
                    operand_origin(
                        tcx,
                        body,
                        &operation.arguments[2],
                        &reachable,
                        &calls,
                        &mut HashSet::new(),
                    )?,
                    Origin::Data { source: true, .. } | Origin::Data { fixed: true, .. }
                ) {
                    return None;
                }
                field_types.push(field_ty);
            }
            CallKind::End => {
                if operation.arguments.len() != 1
                    || operand_origin(
                        tcx,
                        body,
                        &operation.arguments[0],
                        &reachable,
                        &calls,
                        &mut HashSet::new(),
                    )? != Origin::BranchOutput(state_branch)
                {
                    return None;
                }
            }
            _ => return None,
        }

        if operation.kind != CallKind::End {
            let branch = *branch_for_operation.get(&operation.destination)?;
            let (switch, break_target, continue_target) = *switch_for_branch.get(&branch)?;
            if !dominates(&dominators, operation.block, switch) {
                return None;
            }
            let residual_block = *residual_for_branch.get(&branch)?;
            if !can_reach(body, break_target, residual_block, &reachable)
                || can_reach(body, continue_target, residual_block, &reachable)
            {
                return None;
            }
            let next = *operations.get(operation_index + 1)?;
            if !can_reach(body, continue_target, next.block, &reachable)
                || can_reach(body, break_target, next.block, &reachable)
            {
                return None;
            }
        }
    }

    if !return_values_are_unchanged(tcx, body, &reachable, &calls) {
        return None;
    }
    if !statements_are_bounded(tcx, body, &reachable, &calls) {
        return None;
    }
    Some(field_types)
}

fn classify_call(tcx: TyCtxt<'_>, definition: DefId) -> Option<CallKind> {
    let name = tcx.opt_item_name(definition)?.as_str().to_owned();
    if associated_trait_method(
        tcx,
        definition,
        &["serde", "serde_core"],
        &["ser", "Serializer"],
        "serialize_struct",
    ) {
        Some(CallKind::SerializeStruct)
    } else if associated_trait_method(
        tcx,
        definition,
        &["serde", "serde_core"],
        &["ser", "SerializeStruct"],
        "serialize_field",
    ) {
        Some(CallKind::SerializeField)
    } else if associated_trait_method(
        tcx,
        definition,
        &["serde", "serde_core"],
        &["ser", "SerializeStruct"],
        "end",
    ) {
        Some(CallKind::End)
    } else if name == "branch"
        && types::physical_item_path(tcx, tcx.trait_of_assoc(definition)?, "core", &["ops", "try_trait", "Try"])
    {
        Some(CallKind::TryBranch)
    } else if name == "from_residual"
        && types::physical_item_path(
            tcx,
            tcx.trait_of_assoc(definition)?,
            "core",
            &["ops", "try_trait", "FromResidual"],
        )
    {
        Some(CallKind::FromResidual)
    } else {
        None
    }
}

fn associated_trait_method(
    tcx: TyCtxt<'_>,
    definition: DefId,
    crates: &[&str],
    trait_path: &[&str],
    method: &str,
) -> bool {
    tcx.opt_item_name(definition)
        .is_some_and(|name| name.as_str() == method)
        && tcx.trait_of_assoc(definition).is_some_and(|trait_id| {
            crates
                .iter()
                .any(|crate_name| types::physical_item_path(tcx, trait_id, crate_name, trait_path))
        })
}

fn protocol_trait(tcx: TyCtxt<'_>, definition: DefId) -> bool {
    let Some(trait_id) = tcx.trait_of_assoc(definition) else {
        return false;
    };
    ["serde", "serde_core"].iter().any(|crate_name| {
        types::physical_item_path(tcx, trait_id, crate_name, &["ser", "Serialize"])
            || types::physical_item_path(tcx, trait_id, crate_name, &["ser", "Serializer"])
            || types::physical_item_path(
                tcx,
                trait_id,
                crate_name,
                &["ser", "SerializeStruct"],
            )
    }) || types::physical_item_path(
        tcx,
        trait_id,
        "core",
        &["ops", "try_trait", "Try"],
    ) || types::physical_item_path(
        tcx,
        trait_id,
        "core",
        &["ops", "try_trait", "FromResidual"],
    )
}

fn resolve_fixed_helper<'tcx>(
    tcx: TyCtxt<'tcx>,
    definition: DefId,
    arguments: ty::GenericArgsRef<'tcx>,
) -> Option<Instance<'tcx>> {
    if protocol_trait(tcx, definition)
        || !matches!(
            tcx.def_kind(definition),
            rustc_hir::def::DefKind::Fn | rustc_hir::def::DefKind::AssocFn
        )
    {
        return None;
    }
    let environment = ty::TypingEnv::fully_monomorphized();
    let normalized = tcx
        .try_normalize_erasing_regions(environment, ty::Unnormalized::new_wip(arguments))
        .ok()?;
    Instance::try_resolve(tcx, environment, definition, normalized)
        .ok()
        .flatten()
        .or_else(|| {
            tcx.is_mir_available(definition).then_some(Instance {
                def: ty::InstanceKind::Item(definition),
                args: arguments,
            })
        })
}

/// Trace a call-free helper body from its arguments to its return value.
/// This admits direct getters and fixed field assembly. A helper with any
/// call, loop, drop, dynamic projection, or owned temporary is unresolved.
fn fixed_helper_result<'tcx>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
    arguments: &[Origin],
    active: &mut Vec<Instance<'tcx>>,
) -> Option<Origin> {
    if active.contains(&instance)
        || active.len() >= tcx.recursion_limit().0
        || !tcx.is_mir_available(instance.def_id())
        || arguments.iter().any(|origin| !matches!(origin, Origin::Data { .. }))
    {
        return None;
    }
    let body = tcx.instance_mir(instance.def);
    if body.arg_count != arguments.len() || conversion::cyclic(body) {
        return None;
    }
    active.push(instance);
    let reachable = reachable_blocks(body);
    for block in &reachable {
        for statement in &body.basic_blocks[*block].statements {
            match &statement.kind {
                StatementKind::StorageLive(_)
                | StatementKind::StorageDead(_)
                | StatementKind::Nop
                | StatementKind::FakeRead(_) => (),
                StatementKind::Assign(assignment) => {
                    let (target, value) = &**assignment;
                    if !target.projection.is_empty()
                        || (target.local != RETURN_PLACE
                            && target.local.as_usize() <= body.arg_count)
                        || !matches!(
                            helper_rvalue_origin(
                                tcx,
                                body,
                                value,
                                arguments,
                                &reachable,
                                &mut HashSet::new(),
                            ),
                            Some(Origin::Data { .. })
                        )
                    {
                        active.pop();
                        return None;
                    }
                }
                _ => {
                    active.pop();
                    return None;
                }
            }
        }
        match &body.basic_blocks[*block].terminator().kind {
            TerminatorKind::Goto { .. }
            | TerminatorKind::Return
            | TerminatorKind::Unreachable => (),
            TerminatorKind::SwitchInt { discr, .. }
                if matches!(
                    helper_operand_origin(
                        tcx,
                        body,
                        discr,
                        arguments,
                        &reachable,
                        &mut HashSet::new(),
                    ),
                    Some(Origin::Data { .. })
                ) => (),
            _ => {
                active.pop();
                return None;
            }
        }
    }
    let result = helper_local_origin(
        tcx,
        body,
        RETURN_PLACE,
        arguments,
        &reachable,
        &mut HashSet::new(),
    );
    active.pop();
    match result? {
        value @ Origin::Data { .. } => Some(value),
        _ => None,
    }
}

fn helper_operand_origin<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    operand: &Operand<'tcx>,
    arguments: &[Origin],
    reachable: &HashSet<BasicBlock>,
    active: &mut HashSet<Local>,
) -> Option<Origin> {
    match operand {
        Operand::Copy(place) | Operand::Move(place) => helper_place_origin(
            tcx, body, place, arguments, reachable, active,
        ),
        Operand::Constant(constant)
            if constant
                .const_
                .eval(tcx, ty::TypingEnv::fully_monomorphized(), DUMMY_SP)
                .is_ok()
                && fixed_constant_type(tcx, operand.ty(body, tcx)) =>
        {
            Some(Origin::Data {
                source: false,
                fixed: true,
            })
        }
        _ => None,
    }
}

fn helper_place_origin<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    place: &Place<'tcx>,
    arguments: &[Origin],
    reachable: &HashSet<BasicBlock>,
    active: &mut HashSet<Local>,
) -> Option<Origin> {
    let local = place.local;
    if local.as_usize() > 0 && local.as_usize() <= body.arg_count {
        let mut current = body.local_decls[local].ty;
        for projection in place.projection.iter() {
            match projection {
                mir::ProjectionElem::Deref => {
                    let ty::Ref(_, inner, ty::Mutability::Not) = current.kind() else {
                        return None;
                    };
                    current = *inner;
                }
                mir::ProjectionElem::Field(_, field_ty) => current = field_ty,
                mir::ProjectionElem::Downcast(_, _) => (),
                _ => return None,
            }
        }
        return arguments.get(local.as_usize() - 1).copied();
    }
    if !place.projection.is_empty() {
        return None;
    }
    helper_local_origin(tcx, body, local, arguments, reachable, active)
}

fn helper_local_origin<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    local: Local,
    arguments: &[Origin],
    reachable: &HashSet<BasicBlock>,
    active: &mut HashSet<Local>,
) -> Option<Origin> {
    if local.as_usize() > 0 && local.as_usize() <= body.arg_count {
        return arguments.get(local.as_usize() - 1).copied();
    }
    if !active.insert(local) {
        return None;
    }
    let mut origin = None;
    let mut found = false;
    for block in reachable {
        for statement in &body.basic_blocks[*block].statements {
            let StatementKind::Assign(assignment) = &statement.kind else {
                continue;
            };
            let (target, value) = &**assignment;
            if target.local != local {
                continue;
            }
            if !target.projection.is_empty() {
                active.remove(&local);
                return None;
            }
            origin = Some(merge(
                origin,
                helper_rvalue_origin(tcx, body, value, arguments, reachable, active)?,
            )?);
            found = true;
        }
    }
    active.remove(&local);
    found.then_some(origin?)
}

fn helper_rvalue_origin<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    value: &Rvalue<'tcx>,
    arguments: &[Origin],
    reachable: &HashSet<BasicBlock>,
    active: &mut HashSet<Local>,
) -> Option<Origin> {
    match value {
        Rvalue::Use(operand, _) => helper_operand_origin(
            tcx, body, operand, arguments, reachable, active,
        ),
        Rvalue::Ref(_, BorrowKind::Shared, place) | Rvalue::CopyForDeref(place) => {
            helper_place_origin(tcx, body, place, arguments, reachable, active)
        }
        Rvalue::Cast(
            mir::CastKind::PointerCoercion(ty::adjustment::PointerCoercion::Unsize, _),
            operand,
            _,
        ) => helper_operand_origin(tcx, body, operand, arguments, reachable, active),
        Rvalue::Aggregate(kind, operands) if !matches!(**kind, mir::AggregateKind::Closure(..)) => {
            let mut origin = None;
            for operand in operands {
                let next = helper_operand_origin(
                    tcx, body, operand, arguments, reachable, active,
                )?;
                origin = Some(merge(origin, next)?);
            }
            origin.or(Some(Origin::Data {
                source: false,
                fixed: true,
            }))
        }
        Rvalue::Repeat(operand, count) if count.try_to_target_usize(tcx).is_some() => {
            helper_operand_origin(tcx, body, operand, arguments, reachable, active)
        }
        Rvalue::Discriminant(place) => helper_place_origin(
            tcx, body, place, arguments, reachable, active,
        )
        .map(|origin| match origin {
            Origin::Data { source, .. } => Origin::Data {
                source,
                fixed: true,
            },
            other => other,
        }),
        Rvalue::UnaryOp(_, operand) => helper_operand_origin(
            tcx, body, operand, arguments, reachable, active,
        ),
        Rvalue::BinaryOp(_, operands) => {
            let (left, right) = &**operands;
            combine_data(
                helper_operand_origin(tcx, body, left, arguments, reachable, active)?,
                helper_operand_origin(tcx, body, right, arguments, reachable, active)?,
            )
        }
        _ => None,
    }
}

fn exactly_one<'call, 'tcx>(calls: &'call [Call<'tcx>], kind: CallKind) -> Option<&'call Call<'tcx>> {
    let mut matching = calls.iter().filter(|call| call.kind == kind);
    let call = matching.next()?;
    matching.next().is_none().then_some(call)
}

fn parameter_operand(operand: &Operand<'_>, index: usize) -> bool {
    matches!(operand,
        Operand::Copy(place) | Operand::Move(place)
            if place.local.as_usize() == index && place.projection.is_empty())
}

fn static_str_operand<'tcx>(tcx: TyCtxt<'tcx>, operand: &Operand<'tcx>) -> bool {
    let Operand::Constant(constant) = operand else {
        return false;
    };
    let ty::Ref(_, inner, ty::Mutability::Not) = constant.const_.ty().kind() else {
        return false;
    };
    if !matches!(inner.kind(), ty::Str) {
        return false;
    }
    matches!(
        constant
            .const_
            .eval(tcx, ty::TypingEnv::fully_monomorphized(), DUMMY_SP),
        Ok(mir::ConstValue::Slice { .. })
    )
}

fn constant_usize<'tcx>(tcx: TyCtxt<'tcx>, operand: &Operand<'tcx>) -> Option<u64> {
    let Operand::Constant(constant) = operand else {
        return None;
    };
    constant
        .const_
        .try_eval_target_usize(tcx, ty::TypingEnv::fully_monomorphized())
}

fn cleanup_paths_are_bounded<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    normal: &HashSet<BasicBlock>,
    entries: &[BasicBlock],
    calls: &[Call<'tcx>],
    structure: &Call<'tcx>,
    end: &Call<'tcx>,
    branch_for_operation: &HashMap<Local, Local>,
    switch_for_branch: &HashMap<Local, (BasicBlock, BasicBlock, BasicBlock)>,
    branches: &[&Call<'tcx>],
) -> bool {
    let Some(state_branch) = branch_for_operation.get(&structure.destination).copied() else {
        return false;
    };
    let state_ty = end.arguments.first().map(|argument| argument.ty(body, tcx).peel_refs());
    let Some(state_ty) = state_ty else {
        return false;
    };
    let mut allowed_drop_locals = HashMap::new();
    let serializer = Local::from_usize(2);
    allowed_drop_locals.insert(serializer, body.local_decls[serializer].ty);
    for index in 1..body.local_decls.len() {
        let local = Local::from_usize(index);
        if body.local_decls[local].ty == body.local_decls[serializer].ty
            && local_origin(tcx, body, local, normal, calls, &mut HashSet::new())
                == Some(Origin::Serializer)
        {
            allowed_drop_locals.insert(local, body.local_decls[serializer].ty);
        }
        if body.local_decls[local].ty == state_ty
            && local_origin(
                tcx,
                body,
                local,
                normal,
                calls,
                &mut HashSet::new(),
            ) == Some(Origin::BranchOutput(state_branch))
        {
            allowed_drop_locals.insert(local, state_ty);
        }
    }
    for call in calls {
        let ty = body.local_decls[call.destination].ty;
        let allowed = match call.kind {
            CallKind::SerializeStruct | CallKind::SerializeField | CallKind::End => {
                is_result_type(tcx, ty)
            }
            CallKind::TryBranch => is_control_flow_type(tcx, ty),
            CallKind::FixedHelper | CallKind::FromResidual => false,
        };
        if allowed && call.destination != RETURN_PLACE {
            allowed_drop_locals.insert(call.destination, ty);
        }
    }

    let mut pending = entries.to_vec();
    for block in normal {
        let TerminatorKind::Drop {
            place,
            unwind,
            ..
        } = &body.basic_blocks[*block].terminator().kind
        else {
            continue;
        };
        if !allowed_drop_place(tcx, body, place, &allowed_drop_locals) {
            return false;
        }
        let on_error_path = branches.iter().any(|branch| {
            let Some((_, break_target, continue_target)) = switch_for_branch.get(&branch.destination)
            else {
                return false;
            };
            can_reach(body, *break_target, *block, normal)
                && !can_reach(body, *continue_target, *block, normal)
        });
        if !on_error_path {
            return false;
        }
        if let UnwindAction::Cleanup(cleanup) = unwind {
            pending.push(*cleanup);
        }
    }

    let mut seen = HashSet::new();
    while let Some(block) = pending.pop() {
        if normal.contains(&block) || !seen.insert(block) {
            if normal.contains(&block) {
                return false;
            }
            continue;
        }
        let data = &body.basic_blocks[block];
        if data.statements.iter().any(|statement| {
            !drop_flag_statement(body, statement) && !matches!(
                &statement.kind,
                StatementKind::StorageLive(_)
                    | StatementKind::StorageDead(_)
                    | StatementKind::Nop
                    | StatementKind::FakeRead(_)
            )
        }) {
            return false;
        }
        match &data.terminator().kind {
            TerminatorKind::Goto { target } => pending.push(*target),
            TerminatorKind::SwitchInt { discr, targets }
                if fixed_bool_operand(tcx, body, discr, normal, calls) =>
            {
                pending.extend(targets.all_targets());
            }
            TerminatorKind::Drop {
                place,
                target,
                unwind,
                ..
            } => {
                if !allowed_drop_place(tcx, body, place, &allowed_drop_locals) {
                    return false;
                }
                pending.push(*target);
                match unwind {
                    UnwindAction::Cleanup(cleanup) => pending.push(*cleanup),
                    UnwindAction::Continue
                    | UnwindAction::Unreachable
                    | UnwindAction::Terminate(_) => (),
                }
            }
            TerminatorKind::UnwindResume
            | TerminatorKind::UnwindTerminate(_)
            | TerminatorKind::Unreachable => (),
            _ => return false,
        }
    }
    true
}

fn allowed_drop_place<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    place: &Place<'tcx>,
    allowed: &HashMap<Local, Ty<'tcx>>,
) -> bool {
    let Some(allowed_ty) = allowed.get(&place.local) else {
        return false;
    };
    place.projection.is_empty()
        && place.ty(body, tcx).ty == *allowed_ty
        && body.local_decls[place.local].ty == *allowed_ty
}

fn is_result_type(tcx: TyCtxt<'_>, value: Ty<'_>) -> bool {
    matches!(value.kind(), ty::Adt(owner, _) if types::physical_item_path(
        tcx,
        owner.did(),
        "core",
        &["result", "Result"],
    ))
}

fn is_control_flow_type(tcx: TyCtxt<'_>, value: Ty<'_>) -> bool {
    matches!(value.kind(), ty::Adt(owner, _) if types::physical_item_path(
        tcx,
        owner.did(),
        "core",
        &["ops", "control_flow", "ControlFlow"],
    ))
}

fn reachable_blocks(body: &mir::Body<'_>) -> HashSet<BasicBlock> {
    let mut reachable = HashSet::new();
    let mut pending = vec![mir::START_BLOCK];
    while let Some(block) = pending.pop() {
        if reachable.insert(block) {
            pending.extend(normal_successors(
                &body.basic_blocks[block].terminator().kind,
            ));
        }
    }
    reachable
}

fn normal_successors(terminator: &TerminatorKind<'_>) -> Vec<BasicBlock> {
    match terminator {
        TerminatorKind::Goto { target } => vec![*target],
        TerminatorKind::Call {
            target: Some(target),
            ..
        } => vec![*target],
        TerminatorKind::Drop { target, .. } => vec![*target],
        TerminatorKind::SwitchInt { targets, .. } => targets.all_targets().to_vec(),
        _ => Vec::new(),
    }
}

fn dominators(
    body: &mir::Body<'_>,
    reachable: &HashSet<BasicBlock>,
) -> HashMap<BasicBlock, HashSet<BasicBlock>> {
    let mut result: HashMap<_, _> = reachable
        .iter()
        .copied()
        .map(|block| {
            let initial = if block == mir::START_BLOCK {
                HashSet::from([block])
            } else {
                reachable.clone()
            };
            (block, initial)
        })
        .collect();
    loop {
        let mut changed = false;
        for block in reachable.iter().copied().filter(|block| *block != mir::START_BLOCK) {
            let predecessors: Vec<_> = reachable
                .iter()
                .copied()
                .filter(|candidate| {
                    normal_successors(&body.basic_blocks[*candidate].terminator().kind)
                        .into_iter()
                        .any(|successor| successor == block)
                })
                .collect();
            if predecessors.is_empty() {
                continue;
            }
            let mut next = result[&predecessors[0]].clone();
            for predecessor in predecessors.iter().skip(1) {
                next.retain(|candidate| result[predecessor].contains(candidate));
            }
            next.insert(block);
            if next != result[&block] {
                result.insert(block, next);
                changed = true;
            }
        }
        if !changed {
            return result;
        }
    }
}

fn dominates(
    dominators: &HashMap<BasicBlock, HashSet<BasicBlock>>,
    first: BasicBlock,
    second: BasicBlock,
) -> bool {
    dominators
        .get(&second)
        .is_some_and(|set| set.contains(&first))
}

fn order_by_dominance<'call>(
    calls: &mut Vec<&'call Call<'_>>,
    dominators: &HashMap<BasicBlock, HashSet<BasicBlock>>,
) -> Option<()> {
    let mut ordered = Vec::with_capacity(calls.len());
    let mut remaining = std::mem::take(calls);
    while !remaining.is_empty() {
        let first: Vec<_> = remaining
            .iter()
            .enumerate()
            .filter(|(_, candidate)| {
                !remaining.iter().any(|other| {
                    other.block != candidate.block
                        && dominates(dominators, other.block, candidate.block)
                })
            })
            .map(|(index, _)| index)
            .collect();
        if first.len() != 1 {
            return None;
        }
        ordered.push(remaining.remove(first[0]));
    }
    for pair in ordered.windows(2) {
        if !dominates(dominators, pair[0].block, pair[1].block) {
            return None;
        }
    }
    *calls = ordered;
    Some(())
}

fn can_reach(
    body: &mir::Body<'_>,
    start: BasicBlock,
    target: BasicBlock,
    allowed: &HashSet<BasicBlock>,
) -> bool {
    let mut seen = HashSet::new();
    let mut pending = vec![start];
    while let Some(block) = pending.pop() {
        if block == target {
            return true;
        }
        if allowed.contains(&block) && seen.insert(block) {
            pending.extend(normal_successors(
                &body.basic_blocks[block].terminator().kind,
            ));
        }
    }
    false
}

fn control_flow_targets<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    branch: Local,
    targets: &mir::SwitchTargets,
) -> Option<(BasicBlock, BasicBlock)> {
    let ty::Adt(owner, _) = body.local_decls[branch].ty.kind() else {
        return None;
    };
    if !types::physical_item_path(tcx, owner.did(), "core", &["ops", "control_flow", "ControlFlow"])
        || owner.variants().len() != 2
    {
        return None;
    }
    let continue_variant = owner
        .variants()
        .iter_enumerated()
        .find(|(_, variant)| variant.name.as_str() == "Continue")?
        .0;
    let break_variant = owner
        .variants()
        .iter_enumerated()
        .find(|(_, variant)| variant.name.as_str() == "Break")?
        .0;
    let continue_value = owner.discriminant_for_variant(tcx, continue_variant).val;
    let break_value = owner.discriminant_for_variant(tcx, break_variant).val;
    let continue_target = targets.target_for_value(continue_value);
    let break_target = targets.target_for_value(break_value);
    (continue_target != break_target).then_some((break_target, continue_target))
}

fn operand_origin<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    operand: &Operand<'tcx>,
    reachable: &HashSet<BasicBlock>,
    calls: &[Call<'tcx>],
    active: &mut HashSet<Local>,
) -> Option<Origin> {
    match operand {
        Operand::Copy(place) | Operand::Move(place) => {
            place_origin(tcx, body, place, reachable, calls, active)
        }
        Operand::Constant(constant) => {
            let value_ty = operand.ty(body, tcx);
            (constant
                .const_
                .eval(tcx, ty::TypingEnv::fully_monomorphized(), DUMMY_SP)
                .is_ok()
                && fixed_constant_type(tcx, value_ty))
            .then_some(Origin::Data {
                source: false,
                fixed: true,
            })
        }
        Operand::RuntimeChecks(_) => None,
    }
}

fn place_origin<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    place: &Place<'tcx>,
    reachable: &HashSet<BasicBlock>,
    calls: &[Call<'tcx>],
    active: &mut HashSet<Local>,
) -> Option<Origin> {
    if place.local == Local::from_usize(1) {
        return source_projection(body, place).then_some(Origin::Data {
            source: true,
            fixed: false,
        });
    }
    if place.local == Local::from_usize(2) && place.projection.is_empty() {
        return Some(Origin::Serializer);
    }
    if let Some(call) = calls
        .iter()
        .find(|call| call.destination == place.local)
    {
        return match call.kind {
            CallKind::SerializeStruct | CallKind::SerializeField if place.projection.is_empty() => {
                Some(Origin::OperationResult(place.local))
            }
            CallKind::TryBranch => branch_projection(tcx, body, place),
            CallKind::FixedHelper if place.projection.is_empty() => {
                let instance = call.helper?;
                let arguments: Option<Vec<_>> = call
                    .arguments
                    .iter()
                    .map(|argument| {
                        operand_origin(
                            tcx,
                            body,
                            argument,
                            reachable,
                            calls,
                            active,
                        )
                    })
                    .collect();
                fixed_helper_result(tcx, instance, &arguments?, &mut Vec::new())
            }
            _ => None,
        };
    }
    if !place.projection.is_empty() {
        return None;
    }
    local_origin(tcx, body, place.local, reachable, calls, active)
}

fn branch_projection(tcx: TyCtxt<'_>, body: &mir::Body<'_>, place: &Place<'_>) -> Option<Origin> {
    if place.projection.is_empty() {
        return Some(Origin::BranchResult(place.local));
    }
    let ty::Adt(owner, _) = body.local_decls[place.local].ty.kind() else {
        return None;
    };
    if !types::physical_item_path(tcx, owner.did(), "core", &["ops", "control_flow", "ControlFlow"])
    {
        return None;
    }
    let mut variant = None;
    let mut field = false;
    for projection in place.projection.iter() {
        match projection {
            mir::ProjectionElem::Downcast(_, index) if variant.is_none() => {
                variant = Some(owner.variant(index).name.as_str().to_owned());
            }
            mir::ProjectionElem::Field(index, _) if variant.is_some() && !field => {
                if index.as_usize() != 0 {
                    return None;
                }
                field = true;
            }
            _ => return None,
        }
    }
    if !field {
        return None;
    }
    match variant.as_deref()? {
        "Continue" => Some(Origin::BranchOutput(place.local)),
        "Break" => Some(Origin::BranchResidual(place.local)),
        _ => None,
    }
}

fn local_origin<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    local: Local,
    reachable: &HashSet<BasicBlock>,
    calls: &[Call<'tcx>],
    active: &mut HashSet<Local>,
) -> Option<Origin> {
    if !active.insert(local) {
        return None;
    }
    let mut origin = None;
    let mut found = false;
    for block in reachable {
        for statement in &body.basic_blocks[*block].statements {
            let StatementKind::Assign(assignment) = &statement.kind else {
                continue;
            };
            let (target, value) = &**assignment;
            if target.local != local {
                continue;
            }
            if !target.projection.is_empty() {
                active.remove(&local);
                return None;
            }
            let value = rvalue_origin(tcx, body, value, reachable, calls, active)?;
            origin = Some(merge(origin, value)?);
            found = true;
        }
    }
    active.remove(&local);
    found.then_some(origin?)
}

fn rvalue_origin<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    value: &Rvalue<'tcx>,
    reachable: &HashSet<BasicBlock>,
    calls: &[Call<'tcx>],
    active: &mut HashSet<Local>,
) -> Option<Origin> {
    match value {
        Rvalue::Use(operand, _) => operand_origin(tcx, body, operand, reachable, calls, active),
        Rvalue::Ref(_, BorrowKind::Shared | BorrowKind::Mut { .. }, place) => {
            place_origin(tcx, body, place, reachable, calls, active)
        }
        Rvalue::Cast(
            mir::CastKind::PointerCoercion(ty::adjustment::PointerCoercion::Unsize, _),
            operand,
            _,
        ) => operand_origin(tcx, body, operand, reachable, calls, active),
        Rvalue::Aggregate(kind, operands) if !matches!(**kind, mir::AggregateKind::Closure(..)) => {
            let mut combined = None;
            for operand in operands {
                let origin = operand_origin(tcx, body, operand, reachable, calls, active)?;
                if !matches!(origin, Origin::Data { .. }) {
                    return None;
                }
                combined = Some(merge(combined, origin)?);
            }
            Some(combined.unwrap_or(Origin::Data {
                source: false,
                fixed: true,
            }))
        }
        Rvalue::Repeat(operand, count) if count.try_to_target_usize(tcx).is_some() => {
            let origin = operand_origin(tcx, body, operand, reachable, calls, active)?;
            matches!(origin, Origin::Data { .. }).then_some(origin)
        }
        Rvalue::Discriminant(place) => match place_origin(
            tcx, body, place, reachable, calls, active,
        )? {
            Origin::BranchResult(branch) => Some(Origin::BranchTag(branch)),
            Origin::Data { source, .. } => Some(Origin::Data {
                source,
                fixed: true,
            }),
            _ => None,
        },
        Rvalue::CopyForDeref(place) => {
            place_origin(tcx, body, place, reachable, calls, active)
        }
        Rvalue::UnaryOp(_, operand) => {
            let origin = operand_origin(tcx, body, operand, reachable, calls, active)?;
            matches!(origin, Origin::Data { .. }).then_some(origin)
        }
        Rvalue::BinaryOp(_, operands) => {
            let (left, right) = &**operands;
            let left = operand_origin(tcx, body, left, reachable, calls, active)?;
            let right = operand_origin(tcx, body, right, reachable, calls, active)?;
            combine_data(left, right)
        }
        _ => None,
    }
}

fn source_projection(body: &mir::Body<'_>, place: &Place<'_>) -> bool {
    if place.projection.is_empty() {
        return false;
    }
    let mut has_root_deref = false;
    let mut has_field = false;
    let mut current_ty = body.local_decls[place.local].ty;
    for projection in place.projection.iter() {
        match projection {
            mir::ProjectionElem::Deref if !has_root_deref => {
                let ty::Ref(_, inner, ty::Mutability::Not) = current_ty.kind() else {
                    return false;
                };
                current_ty = *inner;
                has_root_deref = true;
            }
            mir::ProjectionElem::Field(_, _) if has_root_deref => has_field = true,
            _ => return false,
        }
    }
    has_root_deref && has_field
}

fn fixed_constant_type(tcx: TyCtxt<'_>, value: Ty<'_>) -> bool {
    match value.kind() {
        ty::Bool | ty::Char | ty::Int(_) | ty::Uint(_) | ty::Float(_) => true,
        ty::Ref(_, referent, ty::Mutability::Not) => match referent.kind() {
            ty::Str => true,
            ty::Slice(element) | ty::Array(element, _) => fixed_constant_type(tcx, *element),
            _ => false,
        },
        ty::Array(element, _) => fixed_constant_type(tcx, *element),
        ty::Tuple(fields) => fields.iter().all(|field| fixed_constant_type(tcx, field)),
        ty::Adt(owner, arguments)
            if types::physical_item_path(tcx, owner.did(), "core", &["option", "Option"]) =>
        {
            arguments
                .types()
                .next()
                .is_some_and(|element| fixed_constant_type(tcx, element))
        }
        _ => false,
    }
}

fn merge(current: Option<Origin>, next: Origin) -> Option<Origin> {
    match (current, next) {
        (None, value) => Some(value),
        (
            Some(Origin::Data { source, fixed }),
            Origin::Data {
                source: next_source,
                fixed: next_fixed,
            },
        ) => Some(Origin::Data {
            source: source || next_source,
            fixed: fixed || next_fixed,
        }),
        (Some(left), right) if left == right => Some(left),
        _ => None,
    }
}

fn combine_data(left: Origin, right: Origin) -> Option<Origin> {
    match (left, right) {
        (
            Origin::Data { source, fixed },
            Origin::Data {
                source: next_source,
                fixed: next_fixed,
            },
        ) => Some(Origin::Data {
            source: source || next_source,
            fixed: fixed || next_fixed,
        }),
        _ => None,
    }
}

fn statements_are_bounded<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    reachable: &HashSet<BasicBlock>,
    calls: &[Call<'tcx>],
) -> bool {
    reachable.iter().all(|block| {
        body.basic_blocks[*block].statements.iter().all(|statement| match &statement.kind {
            StatementKind::StorageLive(_) | StatementKind::StorageDead(_) | StatementKind::Nop => {
                true
            }
            StatementKind::Assign(assignment) => {
                let (target, value) = &**assignment;
                target.projection.is_empty()
                    && target.local.as_usize() > body.arg_count
                    && rvalue_origin(
                        tcx,
                        body,
                        value,
                        reachable,
                        calls,
                        &mut HashSet::new(),
                    )
                    .is_some()
            }
            StatementKind::FakeRead(_) => true,
            _ => false,
        })
    })
}

fn fixed_bool_operand<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    operand: &Operand<'tcx>,
    reachable: &HashSet<BasicBlock>,
    calls: &[Call<'tcx>],
) -> bool {
    matches!(operand.ty(body, tcx).kind(), ty::Bool)
        && operand_origin(tcx, body, operand, reachable, calls, &mut HashSet::new())
            == Some(Origin::Data { source: false, fixed: true })
}

fn drop_flag_statement(body: &mir::Body<'_>, statement: &mir::Statement<'_>) -> bool {
    let StatementKind::Assign(assignment) = &statement.kind else { return false; };
    let (target, value) = &**assignment;
    target.local.as_usize() > body.arg_count
        && target.projection.is_empty()
        && matches!(body.local_decls[target.local].ty.kind(), ty::Bool)
        && matches!(value, Rvalue::Use(Operand::Constant(constant), _)
            if matches!(constant.const_.ty().kind(), ty::Bool))
}

fn return_values_are_unchanged<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    reachable: &HashSet<BasicBlock>,
    calls: &[Call<'tcx>],
) -> bool {
    let terminal: Vec<_> = calls
        .iter()
        .filter(|call| matches!(call.kind, CallKind::End | CallKind::FromResidual))
        .collect();
    if terminal.is_empty()
        || calls
            .iter()
            .filter(|call| call.destination == RETURN_PLACE)
            .count()
            != terminal.len()
    {
        return false;
    }
    let mut returned = HashSet::new();
    for call in terminal {
        let TerminatorKind::Call {
            target: Some(target),
            ..
        } = &body.basic_blocks[call.block].terminator().kind
        else {
            return false;
        };
        let mut pending = vec![*target];
        let mut path = HashSet::new();
        while let Some(block) = pending.pop() {
            if !reachable.contains(&block) {
                return false;
            }
            if !path.insert(block) { continue; }
            let data = &body.basic_blocks[block];
            if data.statements.iter().any(|statement| {
                !drop_flag_statement(body, statement) && !matches!(
                    &statement.kind,
                    StatementKind::StorageLive(_)
                        | StatementKind::StorageDead(_)
                        | StatementKind::Nop
                )
            }) {
                return false;
            }
            match &data.terminator().kind {
                TerminatorKind::Return => {
                    returned.insert(block);
                }
                TerminatorKind::Goto { target }
                | TerminatorKind::Drop { target, .. } => pending.push(*target),
                TerminatorKind::SwitchInt { discr, targets }
                    if fixed_bool_operand(tcx, body, discr, reachable, calls) =>
                {
                    pending.extend(targets.all_targets());
                }
                _ => return false,
            }
        }
    }
    let actual: HashSet<_> = reachable
        .iter()
        .copied()
        .filter(|block| {
            matches!(
                &body.basic_blocks[*block].terminator().kind,
                TerminatorKind::Return
            )
        })
        .collect();
    returned == actual
}
