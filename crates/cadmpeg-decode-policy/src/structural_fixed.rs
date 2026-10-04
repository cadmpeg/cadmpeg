// SPDX-License-Identifier: Apache-2.0
//! Fixed-work MIR bodies used by structural serde admission.

use crate::{conversion, external, types};
use rustc_middle::mir::{
    self, BorrowKind, Local, Operand, Rvalue, StatementKind, TerminatorKind, UnwindAction,
    RETURN_PLACE,
};
use rustc_middle::ty::{self, Instance, Ty, TyCtxt};
use rustc_span::def_id::DefId;
use rustc_span::Spanned;

/// Prove a direct callback body and every direct helper that it invokes.
///
/// The proof admits only acyclic MIR, fixed MIR operations, exact fixed
/// standard metadata operations, and resolved direct calls with the same
/// properties. It does not prove where an argument or returned value came
/// from; the serializer proof owns that data flow.
pub(crate) fn fixed_callback_body<'tcx>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
) -> bool {
    fixed_instance(tcx, instance, &mut Vec::new())
}

/// Prove that a serde skip callback returns `bool` and has fixed work.
pub(crate) fn predicate_call_is_bounded<'tcx>(
    tcx: TyCtxt<'tcx>,
    definition: DefId,
    arguments: ty::GenericArgsRef<'tcx>,
) -> bool {
    let signature = tcx.fn_sig(definition).instantiate(tcx, arguments).skip_binder();
    if signature.inputs().len() != 1 || !matches!(signature.output().kind(), ty::Bool) {
        return false;
    }
    if bool_not_call(tcx, definition, arguments) {
        return true;
    }
    let receiver = signature.inputs().first().copied();
    if fixed_standard_leaf(tcx, definition, receiver) {
        return true;
    }
    let Some(instance) = resolve_instance(tcx, definition, arguments) else {
        return false;
    };
    if !tcx.is_mir_available(instance.def_id()) {
        return false;
    }
    let body = tcx.instance_mir(instance.def);
    let result = instance.instantiate_mir(
        tcx,
        ty::EarlyBinder::bind(tcx, body.local_decls[RETURN_PLACE].ty),
    );
    body.arg_count == 1
        && matches!(result.kind(), ty::Bool)
        && fixed_callback_body(tcx, instance)
}

/// Return the converted wire type only for the exact body emitted by
/// `#[serde(into = "...")]`: clone `self`, convert the clone through the
/// resolved `Into` implementation, then return that value's `Serialize`
/// result unchanged. The clone and conversion call graphs must have fixed
/// MIR bodies.
pub(crate) fn derived_into_source<'tcx>(
    tcx: TyCtxt<'tcx>,
    source: Ty<'tcx>,
    method: DefId,
) -> Option<Ty<'tcx>> {
    if !tcx.is_mir_available(method) {
        return None;
    }
    let instance = crate::structural_derived::serialize_instance(tcx, source, method)?;
    let concrete_body = crate::structural_derived::instantiated_body(tcx, instance);
    let body = &concrete_body;
    if body.arg_count != 2 || conversion::cyclic(body) {
        return None;
    }
    let blocks = linear_blocks(body)?;
    if !into_cleanup_is_bounded(tcx, body, &blocks) { return None; }
    let mut calls = Vec::new();
    for block in &blocks {
        for statement in &body.basic_blocks[*block].statements {
            if !fixed_alias_statement(body, statement) {
                return None;
            }
        }
        match &body.basic_blocks[*block].terminator().kind {
            TerminatorKind::Call {
                func,
                args,
                destination,
                target: Some(_),
                ..
            } => {
                if destination.projection.is_empty() {
                    let function = func.ty(body, tcx);
                    let ty::FnDef(definition, arguments) = function.kind() else {
                        return None;
                    };
                    let arguments = (*arguments).no_bound_vars()?;
                    let instance = if clone_call(tcx, *definition) || into_call(tcx, *definition) {
                        Some(resolve_instance(tcx, *definition, arguments)?)
                    } else if serde_serialize_call(tcx, *definition) {
                        None
                    } else {
                        return None;
                    };
                    calls.push((
                        *block,
                        *definition,
                        args.as_ref(),
                        destination.local,
                        instance,
                    ));
                } else {
                    return None;
                }
            }
            TerminatorKind::Goto { .. } | TerminatorKind::Return => (),
            _ => return None,
        }
    }
    if calls.len() != 3 {
        return None;
    }

    let (_, clone_method, clone_args, clone_result, clone_instance) = calls[0];
    let (_, into_method, into_args, into_result, into_instance) = calls[1];
    let (serialize_block, serialize_method, serialize_args, _, _) = calls[2];
    if !clone_call(tcx, clone_method)
        || !into_call(tcx, into_method)
        || !serde_serialize_call(tcx, serialize_method)
        || clone_args.len() != 1
        || into_args.len() != 1
        || serialize_args.len() != 2
        || !operand_aliases_local(body, &clone_args[0].node, Local::from_usize(1))
        || !operand_aliases_local(body, &into_args[0].node, clone_result)
        || !operand_aliases_local(body, &serialize_args[0].node, into_result)
        || !operand_aliases_local(body, &serialize_args[1].node, Local::from_usize(2))
    {
        return None;
    }

    let source_argument = body.local_decls.get(Local::from_usize(1))?.ty.peel_refs();
    let clone_input = clone_args[0].node.ty(body, tcx).peel_refs();
    let clone_output = body.local_decls.get(clone_result)?.ty.peel_refs();
    let into_input = into_args[0].node.ty(body, tcx).peel_refs();
    let into_output = body.local_decls.get(into_result)?.ty.peel_refs();
    let wire = serialize_args[0].node.ty(body, tcx).peel_refs();
    if source_argument != source.peel_refs()
        || clone_input != source_argument
        || clone_output != source_argument
        || into_input != source_argument
        || into_output != wire
        || !fixed_clone_instance(tcx, clone_instance?, source_argument)
        || !fixed_callback_body(tcx, into_instance?)
        || !result_is_returned(body, &blocks, serialize_block, calls[2].3)
    {
        return None;
    }
    (wire != source.peel_refs()).then_some(wire)
}

fn fixed_alias_statement(body: &mir::Body<'_>, statement: &mir::Statement<'_>) -> bool {
    match &statement.kind {
        StatementKind::StorageLive(_) | StatementKind::StorageDead(_) | StatementKind::Nop => true,
        StatementKind::Assign(assignment) => {
            let (target, value) = &**assignment;
            target.projection.is_empty()
                && target.local.as_usize() > body.arg_count
                && matches!(
                    value,
                    Rvalue::Use(..)
                        | Rvalue::Ref(_, BorrowKind::Shared, _)
                        | Rvalue::Cast(
                            mir::CastKind::PointerCoercion(
                                ty::adjustment::PointerCoercion::Unsize,
                                _,
                            ),
                            _,
                            _,
                        )
                )
        }
        _ => false,
    }
}

fn linear_blocks(body: &mir::Body<'_>) -> Option<Vec<mir::BasicBlock>> {
    let mut blocks = Vec::new();
    let mut seen = Vec::new();
    let mut block = mir::START_BLOCK;
    loop {
        if seen.contains(&block) {
            return None;
        }
        seen.push(block);
        blocks.push(block);
        match &body.basic_blocks[block].terminator().kind {
            TerminatorKind::Call {
                target: Some(next),
                ..
            } => block = *next,
            TerminatorKind::Goto { target } => block = *target,
            TerminatorKind::Return => return Some(blocks),
            _ => return None,
        }
    }
}

fn into_cleanup_is_bounded<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    normal: &[mir::BasicBlock],
) -> bool {
    for (index, block) in body.basic_blocks.iter_enumerated() {
        if normal.contains(&index) { continue; }
        if !block.is_cleanup || block.statements.iter().any(|statement| {
            !matches!(&statement.kind,
                StatementKind::StorageLive(_) | StatementKind::StorageDead(_) | StatementKind::Nop)
                && !drop_flag_assignment(body, statement)
        }) { return false; }
        match &block.terminator().kind {
            TerminatorKind::Goto { .. }
            | TerminatorKind::UnwindResume
            | TerminatorKind::UnwindTerminate(_)
            | TerminatorKind::Unreachable => (),
            TerminatorKind::SwitchInt { discr, .. } => {
                let (Operand::Copy(place) | Operand::Move(place)) = discr else { return false; };
                if !place.projection.is_empty() || !fixed_drop_flag(body, place.local) {
                    return false;
                }
            }
            TerminatorKind::Drop { place, .. } => {
                let value = place.ty(body, tcx).ty;
                let serializer = value == body.local_decls[Local::from_usize(2)].ty
                    && local_aliases_local(body, place.local, Local::from_usize(2), &mut Vec::new());
                if !place.projection.is_empty() || !(serializer || drop_is_fixed(tcx, value)) {
                    return false;
                }
            }
            _ => return false,
        }
    }
    true
}

fn drop_flag_assignment(body: &mir::Body<'_>, statement: &mir::Statement<'_>) -> bool {
    let StatementKind::Assign(assignment) = &statement.kind else { return false; };
    let (target, value) = &**assignment;
    target.local.as_usize() > body.arg_count
        && target.projection.is_empty()
        && matches!(body.local_decls[target.local].ty.kind(), ty::Bool)
        && matches!(value, Rvalue::Use(Operand::Constant(constant), _)
            if matches!(constant.const_.ty().kind(), ty::Bool))
}

fn fixed_drop_flag(body: &mir::Body<'_>, local: Local) -> bool {
    if !matches!(body.local_decls[local].ty.kind(), ty::Bool) { return false; }
    let mut assigned = false;
    for block in body.basic_blocks.iter() {
        if matches!(&block.terminator().kind, TerminatorKind::Call { destination, .. }
            if destination.local == local) { return false; }
        for statement in &block.statements {
            if matches!(&statement.kind, StatementKind::Assign(assignment)
                if assignment.0.local == local) {
                if !drop_flag_assignment(body, statement) { return false; }
                assigned = true;
            }
        }
    }
    assigned
}

fn operand_aliases_local(body: &mir::Body<'_>, operand: &Operand<'_>, expected: mir::Local) -> bool {
    match operand {
        Operand::Copy(place) | Operand::Move(place) => {
            place.projection.is_empty()
                && local_aliases_local(body, place.local, expected, &mut Vec::new())
        }
        _ => false,
    }
}

fn local_aliases_local(
    body: &mir::Body<'_>,
    local: mir::Local,
    expected: mir::Local,
    active: &mut Vec<mir::Local>,
) -> bool {
    if local == expected {
        return true;
    }
    if active.contains(&local) {
        return false;
    }
    active.push(local);
    let mut origin = None;
    for block in body.basic_blocks.iter() {
        for statement in &block.statements {
            let StatementKind::Assign(assignment) = &statement.kind else {
                continue;
            };
            let (target, value) = &**assignment;
            if target.local != local {
                continue;
            }
            if !target.projection.is_empty() {
                active.pop();
                return false;
            }
            let candidate = match value {
                Rvalue::Use(Operand::Copy(place) | Operand::Move(place), _)
                    if place.projection.is_empty() => place.local,
                Rvalue::Ref(_, BorrowKind::Shared, place) if place.projection.is_empty() => {
                    place.local
                }
                Rvalue::Cast(
                    mir::CastKind::PointerCoercion(
                        ty::adjustment::PointerCoercion::Unsize,
                        _,
                    ),
                    Operand::Copy(place) | Operand::Move(place),
                    _,
                ) if place.projection.is_empty() => place.local,
                _ => {
                    active.pop();
                    return false;
                }
            };
            if origin.replace(candidate).is_some() {
                active.pop();
                return false;
            }
        }
    }
    let result = origin.is_some_and(|source| local_aliases_local(body, source, expected, active));
    active.pop();
    result
}

fn result_is_returned(
    body: &mir::Body<'_>,
    blocks: &[mir::BasicBlock],
    result_block: mir::BasicBlock,
    result_local: mir::Local,
) -> bool {
    let Some(start) = blocks.iter().position(|block| *block == result_block) else {
        return false;
    };
    let mut returned = false;
    for block in blocks.iter().skip(start + 1) {
        for statement in &body.basic_blocks[*block].statements {
            match &statement.kind {
                StatementKind::StorageLive(_) | StatementKind::StorageDead(_) | StatementKind::Nop => (),
                StatementKind::Assign(assignment) => {
                    let (target, value) = &**assignment;
                    if target.local == RETURN_PLACE
                        && target.projection.is_empty()
                        && matches!(
                            value,
                            Rvalue::Use(Operand::Copy(place) | Operand::Move(place), _)
                                if place.local == result_local && place.projection.is_empty()
                        )
                    {
                        returned = true;
                    } else if !drop_flag_assignment(body, statement) {
                        return false;
                    }
                }
                _ => return false,
            }
        }
        match &body.basic_blocks[*block].terminator().kind {
            TerminatorKind::Return => return returned || result_local == RETURN_PLACE,
            TerminatorKind::Goto { .. } => (),
            _ => return false,
        }
    }
    false
}

fn clone_call(tcx: TyCtxt<'_>, definition: DefId) -> bool {
    let definition = tcx.trait_item_of(definition).unwrap_or(definition);
    tcx.opt_item_name(definition)
        .is_some_and(|name| name.as_str() == "clone")
        && tcx.trait_of_assoc(definition).is_some_and(|trait_id| {
            types::physical_item_path(tcx, trait_id, "core", &["clone", "Clone"])
        })
}

fn into_call(tcx: TyCtxt<'_>, definition: DefId) -> bool {
    let definition = tcx.trait_item_of(definition).unwrap_or(definition);
    tcx.opt_item_name(definition)
        .is_some_and(|name| name.as_str() == "into")
        && tcx.trait_of_assoc(definition).is_some_and(|trait_id| {
            types::physical_item_path(tcx, trait_id, "core", &["convert", "Into"])
        })
}

fn serde_serialize_call(tcx: TyCtxt<'_>, definition: DefId) -> bool {
    let definition = tcx.trait_item_of(definition).unwrap_or(definition);
    tcx.opt_item_name(definition)
        .is_some_and(|name| name.as_str() == "serialize")
        && tcx.trait_of_assoc(definition).is_some_and(|trait_id| {
            ["serde", "serde_core"].iter().any(|crate_name| {
                types::physical_item_path(tcx, trait_id, crate_name, &["ser", "Serialize"])
            })
        })
}

fn fixed_clone_instance<'tcx>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
    source: Ty<'tcx>,
) -> bool {
    if tcx.is_mir_available(instance.def_id()) {
        return fixed_callback_body(tcx, instance);
    }
    matches!(source.kind(),
        ty::Bool | ty::Char | ty::Int(_) | ty::Uint(_) | ty::Float(_) | ty::Ref(..)
            | ty::RawPtr(..) | ty::FnPtr(..))
}

fn fixed_instance<'tcx>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
    active: &mut Vec<Instance<'tcx>>,
) -> bool {
    if active.contains(&instance)
        || active.len() >= tcx.recursion_limit().0
        || !tcx.is_mir_available(instance.def_id())
        || matches!(instance.def, ty::InstanceKind::Virtual(..))
        || matches!(
            tcx.def_kind(instance.def_id()),
            rustc_hir::def::DefKind::Closure
        )
    {
        return false;
    }
    let body = tcx.instance_mir(instance.def);
    if conversion::cyclic(body) {
        return false;
    }
    active.push(instance);
    let fixed = body.basic_blocks.iter().all(|block| {
        block
            .statements
            .iter()
            .all(|statement| fixed_statement(body, statement))
            && fixed_terminator(tcx, instance, body, &block.terminator().kind, active)
    });
    active.pop();
    fixed
}

fn fixed_statement(body: &mir::Body<'_>, statement: &mir::Statement<'_>) -> bool {
    match &statement.kind {
        StatementKind::StorageLive(_) | StatementKind::StorageDead(_) | StatementKind::Nop => true,
        StatementKind::Assign(assignment) => {
            let (target, value) = &**assignment;
            target.projection.is_empty()
                && (target.local == RETURN_PLACE
                    || target.local.as_usize() > body.arg_count)
                && fixed_rvalue(value)
        }
        _ => false,
    }
}

fn fixed_rvalue(value: &Rvalue<'_>) -> bool {
    match value {
        Rvalue::Use(operand, _) | Rvalue::UnaryOp(_, operand) | Rvalue::Repeat(operand, _) => {
            fixed_operand(operand)
        }
        Rvalue::Ref(_, BorrowKind::Shared, place)
        | Rvalue::CopyForDeref(place)
        | Rvalue::Discriminant(place) => fixed_place(place),
        Rvalue::BinaryOp(_, operands) => {
            fixed_operand(&operands.0) && fixed_operand(&operands.1)
        }
        Rvalue::Cast(_, operand, _) => fixed_operand(operand),
        Rvalue::Aggregate(_, operands) => operands.iter().all(fixed_operand),
        _ => false,
    }
}

fn fixed_operand(operand: &Operand<'_>) -> bool {
    match operand {
        Operand::Copy(place) | Operand::Move(place) => fixed_place(place),
        Operand::Constant(_) => true,
        Operand::RuntimeChecks(_) => false,
    }
}

fn fixed_place(place: &mir::Place<'_>) -> bool {
    place.projection.iter().all(|element| {
        matches!(
            element,
            mir::ProjectionElem::Deref
                | mir::ProjectionElem::Field(..)
                | mir::ProjectionElem::Downcast(..)
        )
    })
}

fn fixed_terminator<'tcx>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
    body: &mir::Body<'tcx>,
    terminator: &TerminatorKind<'tcx>,
    active: &mut Vec<Instance<'tcx>>,
) -> bool {
    match terminator {
        TerminatorKind::Goto { .. }
        | TerminatorKind::Return
        | TerminatorKind::Unreachable => true,
        TerminatorKind::SwitchInt { discr, .. } => fixed_operand(discr),
        TerminatorKind::Call {
            func,
            args,
            destination,
            target: Some(_),
            unwind,
            ..
        } => {
            if matches!(unwind, UnwindAction::Cleanup(_))
                || !fixed_place(destination)
                || !args.iter().all(|argument| fixed_operand(&argument.node))
            {
                return false;
            }
            let function = instance.instantiate_mir(
                tcx,
                ty::EarlyBinder::bind(tcx, func.ty(body, tcx)),
            );
            let ty::FnDef(definition, arguments) = function.kind() else {
                return false;
            };
            let Some(arguments) = (*arguments).no_bound_vars() else {
                return false;
            };
            if option_map_call(tcx, *definition) || array_map_call(tcx, *definition) {
                return fixed_map_callback(tcx, *definition, args, body, instance, active);
            }
            if bool_not_call(tcx, *definition, arguments)
                || fixed_standard_leaf(
                    tcx,
                    *definition,
                    args.first().map(|argument| {
                        instance.instantiate_mir(
                            tcx,
                            ty::EarlyBinder::bind(tcx, argument.node.ty(body, tcx)),
                        )
                    }),
                )
            {
                return true;
            }
            resolve_instance(tcx, *definition, arguments)
                .is_some_and(|callee| fixed_instance(tcx, callee, active))
        }
        _ => false,
    }
}

fn fixed_map_callback<'tcx>(
    tcx: TyCtxt<'tcx>,
    definition: DefId,
    call_arguments: &[Spanned<Operand<'tcx>>],
    body: &mir::Body<'tcx>,
    caller: Instance<'tcx>,
    active: &mut Vec<Instance<'tcx>>,
) -> bool {
    if call_arguments.len() != 2 {
        return false;
    }
    let callback_type = caller.instantiate_mir(
        tcx,
        ty::EarlyBinder::bind(tcx, call_arguments[1].node.ty(body, tcx)),
    );
    let Some(callback) = function_item(tcx, callback_type) else {
        return false;
    };
    let receiver_type = caller.instantiate_mir(
        tcx,
        ty::EarlyBinder::bind(tcx, call_arguments[0].node.ty(body, tcx)),
    );
    let element = if option_map_call(tcx, definition) {
        let ty::Adt(option, arguments) = receiver_type.peel_refs().kind() else {
            return false;
        };
        if !types::physical_item_path(tcx, option.did(), "core", &["option", "Option"]) {
            return false;
        }
        let Some(element) = arguments.types().next() else {
            return false;
        };
        element
    } else {
        let ty::Array(element, _) = receiver_type.peel_refs().kind() else {
            return false;
        };
        *element
    };
    if !tcx.type_is_copy_modulo_regions(ty::TypingEnv::fully_monomorphized(), element) {
        return false;
    }
    if !matches!(
        tcx.def_kind(callback.def_id()),
        rustc_hir::def::DefKind::Fn | rustc_hir::def::DefKind::AssocFn
    ) || !tcx.is_mir_available(callback.def_id())
    {
        return false;
    }
    if tcx.instance_mir(callback.def).arg_count != 1 {
        return false;
    }
    fixed_instance(tcx, callback, active)
}

fn function_item<'tcx>(tcx: TyCtxt<'tcx>, value: Ty<'tcx>) -> Option<Instance<'tcx>> {
    match value.kind() {
        ty::FnDef(definition, arguments)
            if matches!(
                tcx.def_kind(*definition),
                rustc_hir::def::DefKind::Fn | rustc_hir::def::DefKind::AssocFn
            ) =>
        {
            resolve_instance(tcx, *definition, (*arguments).no_bound_vars()?)
        }
        _ => None,
    }
}

fn resolve_instance<'tcx>(
    tcx: TyCtxt<'tcx>,
    definition: DefId,
    arguments: ty::GenericArgsRef<'tcx>,
) -> Option<Instance<'tcx>> {
    let environment = ty::TypingEnv::fully_monomorphized();
    let normalized = tcx
        .try_normalize_erasing_regions(
            environment,
            ty::Unnormalized::new_wip(arguments),
        )
        .ok();
    normalized
        .and_then(|arguments| {
            Instance::try_resolve(tcx, environment, definition, arguments)
                .ok()
                .flatten()
        })
        .or_else(|| {
            tcx.is_mir_available(definition).then_some(Instance {
                def: ty::InstanceKind::Item(definition),
                args: arguments,
            })
        })
}

fn option_map_call(tcx: TyCtxt<'_>, definition: DefId) -> bool {
    types::physical_inherent_method(
        tcx,
        definition,
        "core",
        &["option", "Option"],
        "map",
    )
}

fn array_map_call(tcx: TyCtxt<'_>, definition: DefId) -> bool {
    if tcx.crate_name(definition.krate).as_str() != "core"
        || tcx.opt_item_name(definition).is_none_or(|name| name.as_str() != "map")
        || !matches!(
            tcx.def_kind(tcx.parent(definition)),
            rustc_hir::def::DefKind::Impl { of_trait: false }
        )
    {
        return false;
    }
    let parent = tcx.parent(definition);
    matches!(
        tcx.type_of(parent)
            .instantiate_identity()
            .skip_norm_wip()
            .kind(),
        ty::Array(..)
    )
}

fn bool_not_call<'tcx>(
    tcx: TyCtxt<'tcx>,
    definition: DefId,
    arguments: ty::GenericArgsRef<'tcx>,
) -> bool {
    let trait_method = tcx.trait_item_of(definition).unwrap_or(definition);
    let Some(trait_id) = tcx.trait_of_assoc(trait_method) else {
        return false;
    };
    if tcx.opt_item_name(definition).is_none_or(|name| name.as_str() != "not")
        || !types::physical_item_path(tcx, trait_id, "core", &["ops", "bit", "Not"])
    {
        return false;
    }
    let signature = tcx.fn_sig(definition).instantiate(tcx, arguments).skip_binder();
    matches!(signature.output().kind(), ty::Bool)
        && signature.inputs().first().is_some_and(|input| match input.kind() {
            ty::Bool => true,
            ty::Ref(_, inner, ty::Mutability::Not) => matches!(inner.kind(), ty::Bool),
            _ => false,
        })
}

fn fixed_standard_leaf(tcx: TyCtxt<'_>, definition: DefId, receiver: Option<Ty<'_>>) -> bool {
    let Some(name) = tcx.opt_item_name(definition) else {
        return false;
    };
    let fixed_owner = [
        ("core", &["option", "Option"][..]),
        ("alloc", &["string", "String"][..]),
        ("alloc", &["vec", "Vec"][..]),
        ("alloc", &["collections", "btree", "map", "BTreeMap"][..]),
        ("alloc", &["collections", "btree", "set", "BTreeSet"][..]),
        ("std", &["collections", "hash", "map", "HashMap"][..]),
        ("std", &["collections", "hash", "set", "HashSet"][..]),
    ]
    .iter()
    .any(|(crate_name, owner)| {
        ["len", "is_empty", "is_none", "is_some", "as_str", "as_slice"]
            .iter()
            .any(|method| {
                name.as_str() == *method
                    && types::physical_inherent_method(
                        tcx,
                        definition,
                        crate_name,
                        owner,
                        method,
                    )
            })
    });
    let string_metadata = types::physical_inherent_method(
        tcx,
        definition,
        "alloc",
        &["string", "String"],
        "is_empty",
    ) || types::physical_inherent_method(
        tcx,
        definition,
        "alloc",
        &["string", "String"],
        "len",
    );
    fixed_owner
        && !string_metadata
        && external::summary(tcx, definition, receiver).is_some_and(|summary| {
            summary.allocation == external::Allocation::None
                && summary.work == external::Work::Fixed
        })
}

/// Returns whether a temporary has no source-sized destructor work.
pub(crate) fn drop_is_fixed<'tcx>(tcx: TyCtxt<'tcx>, value: Ty<'tcx>) -> bool {
    fixed_drop_type(tcx, value, &mut Vec::new())
}

/// Prove that dropping a fixed wire temporary cannot call a user destructor
/// or walk an input-sized owned value.
fn fixed_drop_type<'tcx>(
    tcx: TyCtxt<'tcx>,
    value: Ty<'tcx>,
    active: &mut Vec<Ty<'tcx>>,
) -> bool {
    let value = types::reveal_opaque(tcx, value);
    if active.contains(&value) || active.len() >= tcx.recursion_limit().0 {
        return false;
    }
    let depth = active.len();
    active.push(value);
    let bounded = match value.kind() {
        ty::Bool
        | ty::Char
        | ty::Float(_)
        | ty::Int(_)
        | ty::Uint(_)
        | ty::Str
        | ty::Ref(..)
        | ty::RawPtr(..)
        | ty::FnDef(..)
        | ty::FnPtr(..) => true,
        ty::Array(element, _) => fixed_drop_type(tcx, *element, active),
        ty::Tuple(fields) => fields
            .iter()
            .all(|field| fixed_drop_type(tcx, field, active)),
        ty::Adt(owner, arguments) => {
            let option = types::physical_item_path(
                tcx,
                owner.did(),
                "core",
                &["option", "Option"],
            );
            !owner.is_union()
                && !owner.has_dtor(tcx)
                && (!types::standard(tcx, owner.did()) || option)
                && owner.all_fields().all(|field| {
                    fixed_drop_type(
                        tcx,
                        field.ty(tcx, arguments).skip_norm_wip(),
                        active,
                    )
                })
        }
        _ => false,
    };
    active.truncate(depth);
    bounded
}
