// SPDX-License-Identifier: Apache-2.0
//! Source-bounded scalar `Serialize` implementations admitted by projection.

use crate::{external, types};
use rustc_middle::mir::{
    self, Body, BorrowKind, CastKind, Local, Operand, Place, Rvalue, StatementKind,
    TerminatorKind, UnwindAction, RETURN_PLACE,
};
use rustc_middle::ty::{self, Ty, TyCtxt};
use rustc_span::def_id::DefId;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Origin {
    Source,
    Static,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScalarKind {
    Text,
    Bool,
    Char,
    Integer,
    Float,
}

/// Prove that a handwritten serializer only forwards one borrowed scalar to
/// the concrete Projector protocol. This stays local to structural projection;
/// it is not a summary for arbitrary `Serialize` callbacks.
pub(crate) fn is_bounded_impl(tcx: TyCtxt<'_>, method: DefId) -> bool {
    let Some(body) = tcx.is_mir_available(method).then(|| {
        tcx.instance_mir(ty::InstanceKind::Item(method))
    }) else {
        return false;
    };
    if body.arg_count != 2 || crate::conversion::cyclic(body) {
        return false;
    }
    let Some(blocks) = linear_blocks(body) else {
        return false;
    };

    let mut protocol = None;
    let mut getter_destinations = HashSet::new();
    for block in &blocks {
        for statement in &body.basic_blocks[*block].statements {
            if !projection_statement(statement, false) {
                return false;
            }
        }
        let terminator = &body.basic_blocks[*block].terminator().kind;
        let TerminatorKind::Call {
            func,
            args,
            destination,
            target: Some(_),
            unwind,
            ..
        } = terminator
        else {
            continue;
        };
        if matches!(unwind, UnwindAction::Cleanup(_)) || !destination.projection.is_empty() {
            return false;
        }
        let ty::FnDef(definition, _) = func.ty(body, tcx).kind() else {
            return false;
        };
        if let Some(kind) = serializer_scalar_call(tcx, *definition) {
            if protocol.replace((*block, kind, destination.local, args)).is_some() {
                return false;
            }
        } else {
            if destination.local == RETURN_PLACE || args.len() != 1 {
                return false;
            }
            getter_destinations.insert(destination.local);
        }
    }

    let Some((protocol_block, kind, protocol_result, protocol_args)) = protocol else {
        return false;
    };
    if protocol_args.len() != 2
        || !parameter_place(&protocol_args[0].node, body, 2)
        || scalar_kind(protocol_args[1].node.ty(body, tcx)) != Some(kind)
    {
        return false;
    }
    let mut used_getters = HashSet::new();
    let Some(_origin) = trace_operand(
        tcx,
        body,
        &protocol_args[1].node,
        Local::from_usize(1),
        &mut HashSet::new(),
        &mut used_getters,
        &mut Vec::new(),
    ) else {
        return false;
    };
    if used_getters != getter_destinations {
        return false;
    }
    result_is_returned(body, &blocks, protocol_block, protocol_result)
}

fn scalar_kind(value: Ty<'_>) -> Option<ScalarKind> {
    match value.kind() {
        ty::Ref(_, inner, ty::Mutability::Not) if matches!(inner.kind(), ty::Str) => {
            Some(ScalarKind::Text)
        }
        ty::Bool => Some(ScalarKind::Bool),
        ty::Char => Some(ScalarKind::Char),
        ty::Int(_) | ty::Uint(_) => Some(ScalarKind::Integer),
        ty::Float(_) => Some(ScalarKind::Float),
        _ => None,
    }
}

fn serializer_scalar_call(tcx: TyCtxt<'_>, definition: DefId) -> Option<ScalarKind> {
    let trait_id = tcx.trait_of_assoc(definition)?;
    if !serde_serializer_trait(tcx, trait_id) {
        return None;
    }
    match tcx.item_name(definition).as_str() {
        "serialize_str" => Some(ScalarKind::Text),
        "serialize_bool" => Some(ScalarKind::Bool),
        "serialize_char" => Some(ScalarKind::Char),
        "serialize_i8" | "serialize_i16" | "serialize_i32" | "serialize_i64" | "serialize_i128"
        | "serialize_u8" | "serialize_u16" | "serialize_u32" | "serialize_u64"
        | "serialize_u128" => {
            Some(ScalarKind::Integer)
        }
        "serialize_f32" | "serialize_f64" => Some(ScalarKind::Float),
        _ => None,
    }
}

fn serde_serializer_trait(tcx: TyCtxt<'_>, definition: DefId) -> bool {
    ["serde", "serde_core"].iter().any(|crate_name| {
        types::physical_item_path(tcx, definition, crate_name, &["ser", "Serializer"])
    })
}

fn linear_blocks(body: &Body<'_>) -> Option<Vec<mir::BasicBlock>> {
    let mut blocks = Vec::new();
    let mut seen = HashSet::new();
    let mut block = mir::START_BLOCK;
    loop {
        if !seen.insert(block) {
            return None;
        }
        blocks.push(block);
        match &body.basic_blocks[block].terminator().kind {
            TerminatorKind::Call {
                target: Some(next),
                unwind,
                ..
            } if !matches!(unwind, UnwindAction::Cleanup(_)) => block = *next,
            TerminatorKind::Goto { target } => block = *target,
            TerminatorKind::Return => return Some(blocks),
            _ => return None,
        }
    }
}

fn projection_statement(statement: &mir::Statement<'_>, allow_return: bool) -> bool {
    match &statement.kind {
        StatementKind::StorageLive(_)
        | StatementKind::StorageDead(_)
        | StatementKind::Nop => true,
        StatementKind::Assign(assignment) => {
            let (target, value) = &**assignment;
            target.projection.is_empty()
                && (allow_return || target.local != RETURN_PLACE)
                && matches!(
                    value,
                    Rvalue::Use(..)
                        | Rvalue::Ref(_, BorrowKind::Shared, _)
                        | Rvalue::Cast(
                            CastKind::PointerCoercion(
                                ty::adjustment::PointerCoercion::Unsize,
                                _,
                            ),
                            _,
                            _
                        )
                )
        }
        _ => false,
    }
}

fn parameter_place(operand: &Operand<'_>, body: &Body<'_>, index: usize) -> bool {
    let place = match operand {
        Operand::Copy(place) | Operand::Move(place) => place,
        _ => return false,
    };
    place.local.as_usize() == index && place.projection.is_empty() && index <= body.arg_count
}

fn result_is_returned(
    body: &Body<'_>,
    blocks: &[mir::BasicBlock],
    protocol_block: mir::BasicBlock,
    result: Local,
) -> bool {
    let Some(start) = blocks.iter().position(|block| *block == protocol_block) else {
        return false;
    };
    let mut value = result;
    for block in blocks.iter().skip(start + 1) {
        for statement in &body.basic_blocks[*block].statements {
            match &statement.kind {
                StatementKind::StorageLive(_)
                | StatementKind::StorageDead(_)
                | StatementKind::Nop => (),
                StatementKind::Assign(assignment) => {
                    let (target, rvalue) = &**assignment;
                    if target.projection.is_empty()
                        && target.local == RETURN_PLACE
                        && matches!(
                            rvalue,
                            Rvalue::Use(Operand::Copy(place) | Operand::Move(place), _)
                                if place.local == value && place.projection.is_empty()
                        )
                    {
                        value = RETURN_PLACE;
                    } else {
                        return false;
                    }
                }
                _ => return false,
            }
        }
        match &body.basic_blocks[*block].terminator().kind {
            TerminatorKind::Goto { .. } => (),
            TerminatorKind::Return => return value == RETURN_PLACE || value == result,
            _ => return false,
        }
    }
    false
}

fn trace_operand<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &Body<'tcx>,
    operand: &Operand<'tcx>,
    source: Local,
    active: &mut HashSet<Local>,
    used_getters: &mut HashSet<Local>,
    getter_stack: &mut Vec<DefId>,
) -> Option<Origin> {
    match operand {
        Operand::Copy(place) => trace_place(
            tcx,
            body,
            place,
            source,
            active,
            used_getters,
            getter_stack,
        ),
        Operand::Move(place)
            if tcx.type_is_copy_modulo_regions(
                ty::TypingEnv::fully_monomorphized(),
                operand.ty(body, tcx),
            ) => trace_place(
            tcx,
            body,
            place,
            source,
            active,
            used_getters,
            getter_stack,
        ),
        Operand::Constant(_) => {
            scalar_kind(operand.ty(body, tcx))?;
            Some(Origin::Static)
        }
        Operand::Move(_) | Operand::RuntimeChecks(_) => None,
    }
}

fn trace_place<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &Body<'tcx>,
    place: &Place<'tcx>,
    source: Local,
    active: &mut HashSet<Local>,
    used_getters: &mut HashSet<Local>,
    getter_stack: &mut Vec<DefId>,
) -> Option<Origin> {
    if place.local == source {
        return bounded_source_projection(tcx, body, place).then_some(Origin::Source);
    }
    if !place.projection.is_empty() || !active.insert(place.local) {
        return None;
    }
    let mut assignment = None;
    let mut call = None;
    for block in body.basic_blocks.iter() {
        for statement in &block.statements {
            if let StatementKind::Assign(value) = &statement.kind {
                if value.0.local == place.local {
                    if !value.0.projection.is_empty() || assignment.is_some() {
                        return None;
                    }
                    assignment = Some(&value.1);
                }
            }
        }
        if let TerminatorKind::Call {
            func,
            args,
            destination,
            target: Some(_),
            unwind,
            ..
        } = &block.terminator().kind
        {
            if destination.local == place.local {
                if !destination.projection.is_empty() || call.is_some() || !args.len().eq(&1) {
                    return None;
                }
                if matches!(unwind, UnwindAction::Cleanup(_)) {
                    return None;
                }
                let ty::FnDef(definition, _) = func.ty(body, tcx).kind() else {
                    return None;
                };
                call = Some((*definition, args));
            }
        }
    }
    if assignment.is_some() && call.is_some() {
        return None;
    }
    let origin = if let Some(rvalue) = assignment {
        match rvalue {
            Rvalue::Use(operand, _) => trace_operand(
                tcx,
                body,
                operand,
                source,
                active,
                used_getters,
                getter_stack,
            ),
            Rvalue::Ref(_, BorrowKind::Shared, referenced) => trace_place(
                tcx,
                body,
                referenced,
                source,
                active,
                used_getters,
                getter_stack,
            ),
            Rvalue::Cast(
                CastKind::PointerCoercion(
                    ty::adjustment::PointerCoercion::Unsize,
                    _,
                ),
                operand,
                _,
            ) => trace_operand(
                tcx,
                body,
                operand,
                source,
                active,
                used_getters,
                getter_stack,
            ),
            _ => None,
        }
    } else if let Some((definition, arguments)) = call {
        let argument = &arguments[0].node;
        let receiver_ty = argument.ty(body, tcx);
        let input = trace_operand(
            tcx,
            body,
            argument,
            source,
            active,
            used_getters,
            getter_stack,
        )?;
        let output = local_getter(
            tcx,
            definition,
            receiver_ty,
            destination_output_ty(body, place.local)?,
            input,
            getter_stack,
        )?;
        used_getters.insert(place.local);
        Some(output)
    } else {
        None
    };
    active.remove(&place.local);
    origin
}

fn local_getter(
    tcx: TyCtxt<'_>,
    definition: DefId,
    receiver: Ty<'_>,
    output: Ty<'_>,
    input: Origin,
    getter_stack: &mut Vec<DefId>,
) -> Option<Origin> {
    if fixed_string_borrow(tcx, definition, receiver, output) {
        return Some(input);
    }
    if getter_stack.contains(&definition)
        || getter_stack.len() >= tcx.recursion_limit().0
        || !tcx.is_mir_available(definition)
    {
        return None;
    }
    getter_stack.push(definition);
    let body = tcx.instance_mir(ty::InstanceKind::Item(definition));
    let output = projection_getter_body(tcx, body, input, getter_stack)
        .or_else(|| finite_static_getter_body(tcx, body));
    getter_stack.pop();
    output
}

pub(crate) fn fixed_string_borrow(
    tcx: TyCtxt<'_>,
    definition: DefId,
    receiver: Ty<'_>,
    output: Ty<'_>,
) -> bool {
    let ty::Ref(_, inner, ty::Mutability::Not) = receiver.kind() else {
        return false;
    };
    let ty::Adt(string, _) = inner.kind() else {
        return false;
    };
    if !types::physical_item_path(tcx, string.did(), "alloc", &["string", "String"]) {
        return false;
    }
    let method_name = tcx.item_name(definition);
    let method = method_name.as_str();
    let exact_method = if method == "as_str" {
        types::physical_inherent_method(
            tcx,
            definition,
            "alloc",
            &["string", "String"],
            "as_str",
        )
    } else if method == "deref" {
        tcx.trait_of_assoc(definition).is_some_and(|trait_id| {
            tcx.item_name(trait_id).as_str() == "Deref"
                && (types::physical_item_path(tcx, trait_id, "core", &["ops", "Deref"])
                    || types::physical_item_path(
                        tcx,
                        trait_id,
                        "core",
                        &["ops", "deref", "Deref"],
                    ))
        })
    } else {
        false
    };
    if !exact_method {
        return false;
    }
    if !matches!(
        output.kind(),
        ty::Ref(_, inner, ty::Mutability::Not) if matches!(inner.kind(), ty::Str)
    ) {
        return false;
    }
    external::summary(tcx, definition, Some(*inner)).is_some_and(|summary| {
        summary.allocation == external::Allocation::None && summary.work == external::Work::Fixed
    })
}

fn destination_output_ty<'tcx>(body: &Body<'tcx>, local: Local) -> Option<Ty<'tcx>> {
    body.local_decls.get(local).map(|declaration| declaration.ty)
}

fn projection_getter_body<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &Body<'tcx>,
    input: Origin,
    getter_stack: &mut Vec<DefId>,
) -> Option<Origin> {
    if body.arg_count != 1 || crate::conversion::cyclic(body) {
        return None;
    }
    let blocks = linear_blocks(body)?;
    for block in &blocks {
        for statement in &body.basic_blocks[*block].statements {
            if !projection_statement(statement, true) {
                return None;
            }
        }
    }
    let mut used = HashSet::new();
    let result = trace_operand(
        tcx,
        body,
        &Operand::Copy(Place::from(RETURN_PLACE)),
        Local::from_usize(1),
        &mut HashSet::new(),
        &mut used,
        getter_stack,
    )?;
    let called: HashSet<_> = blocks
        .iter()
        .filter_map(|block| match &body.basic_blocks[*block].terminator().kind {
            TerminatorKind::Call { destination, .. } => Some(destination.local),
            _ => None,
        })
        .collect();
    (used == called).then_some(match (input, result) {
        (Origin::Static, _) | (_, Origin::Static) => Origin::Static,
        _ => Origin::Source,
    })
}

fn bounded_source_projection<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &Body<'tcx>,
    place: &Place<'tcx>,
) -> bool {
    place.as_ref().iter_projections().all(|(base, element)| match element {
        mir::ProjectionElem::Deref => {
            matches!(base.to_place(tcx).ty(body, tcx).ty.kind(), ty::Ref(..))
        }
        mir::ProjectionElem::Field(..) => true,
        _ => false,
    })
}

/// Admit finite scalar-to-literal lookups such as `CodecFormat::as_str`.
/// Every executable operation is a fixed scalar projection or a branch in an
/// acyclic MIR graph. Calls, loops, allocations, drops, and scans are rejected.
fn finite_static_getter_body<'tcx, 'body>(
    tcx: TyCtxt<'tcx>,
    body: &'body Body<'tcx>,
) -> Option<Origin> {
    if body.arg_count != 1 || crate::conversion::cyclic(body) {
        return None;
    }
    let source = Local::from_usize(1);
    if !selector_type(body.local_decls.get(source)?.ty.peel_refs())
        || !is_static_text_type(body.local_decls[RETURN_PLACE].ty)
    {
        return None;
    }

    let mut selectors = HashMap::new();
    let mut returned_text = false;
    for block in body.basic_blocks.iter() {
        for statement in &block.statements {
            match &statement.kind {
                StatementKind::StorageLive(_)
                | StatementKind::StorageDead(_)
                | StatementKind::Nop => (),
                StatementKind::Assign(assignment) => {
                    let (target, value) = &**assignment;
                    if !target.projection.is_empty() {
                        return None;
                    }
                    if target.local == RETURN_PLACE {
                        if !static_text_constant(tcx, body, value) {
                            return None;
                        }
                        returned_text = true;
                    } else {
                        if target.local.as_usize() <= body.arg_count
                            || !selector_type(body.local_decls.get(target.local)?.ty)
                            || selectors.insert(target.local, value).is_some()
                        {
                            return None;
                        }
                    }
                }
                _ => return None,
            }
        }
        if !matches!(
            &block.terminator().kind,
            TerminatorKind::Goto { .. }
                | TerminatorKind::Return
                | TerminatorKind::Unreachable
                | TerminatorKind::SwitchInt { .. }
        ) {
            return None;
        }
    }
    for block in body.basic_blocks.iter() {
        if let TerminatorKind::SwitchInt { discr, .. } = &block.terminator().kind {
            if !selector_operand(
                tcx,
                body,
                &discr,
                source,
                &selectors,
                &mut HashSet::new(),
            ) {
                return None;
            }
        }
    }
    if !returned_text
        || !selectors.iter().all(|(local, value)| {
            selector_type(body.local_decls[*local].ty)
                && selector_value(tcx, body, value, source, &selectors, &mut HashSet::new())
        })
    {
        return None;
    }
    Some(Origin::Static)
}

fn selector_type(value: Ty<'_>) -> bool {
    match value.kind() {
        ty::Bool | ty::Char | ty::Int(_) | ty::Uint(_) => true,
        ty::Adt(owner, _) => {
            owner.is_enum() && owner.variants().iter().all(|variant| variant.fields.is_empty())
        }
        _ => false,
    }
}

fn is_static_text_type(value: Ty<'_>) -> bool {
    matches!(
        value.kind(),
        ty::Ref(_, inner, ty::Mutability::Not) if matches!(inner.kind(), ty::Str)
    )
}

fn static_text_constant<'tcx>(tcx: TyCtxt<'tcx>, body: &Body<'tcx>, value: &Rvalue<'tcx>) -> bool {
    matches!(value, Rvalue::Use(Operand::Constant(_), _))
        && matches!(value, Rvalue::Use(operand, _) if is_static_text_type(operand.ty(body, tcx)))
}

fn selector_value<'tcx, 'body>(
    tcx: TyCtxt<'tcx>,
    body: &'body Body<'tcx>,
    value: &'body Rvalue<'tcx>,
    source: Local,
    selectors: &HashMap<Local, &'body Rvalue<'tcx>>,
    active: &mut HashSet<Local>,
) -> bool {
    match value {
        Rvalue::Discriminant(place) => {
            place.local == source
                && unit_enum_type(place.ty(body, tcx).ty)
                && bounded_source_projection(tcx, body, place)
        }
        Rvalue::Use(operand, _) => {
            selector_operand(tcx, body, operand, source, selectors, active)
        }
        _ => false,
    }
}

fn selector_operand<'tcx, 'body>(
    tcx: TyCtxt<'tcx>,
    body: &'body Body<'tcx>,
    operand: &Operand<'tcx>,
    source: Local,
    selectors: &HashMap<Local, &'body Rvalue<'tcx>>,
    active: &mut HashSet<Local>,
) -> bool {
    match operand {
        Operand::Constant(_) => selector_type(operand.ty(body, tcx)),
        Operand::Copy(place) | Operand::Move(place)
            if place.local == source
                && tcx.type_is_copy_modulo_regions(
                    ty::TypingEnv::fully_monomorphized(),
                    place.ty(body, tcx).ty,
                ) =>
        {
            selector_type(place.ty(body, tcx).ty) && bounded_source_projection(tcx, body, place)
        }
        Operand::Copy(place) | Operand::Move(place)
            if place.projection.is_empty()
                && tcx.type_is_copy_modulo_regions(
                    ty::TypingEnv::fully_monomorphized(),
                    place.ty(body, tcx).ty,
                )
                && active.insert(place.local) =>
        {
            let accepted = selectors.get(&place.local).is_some_and(|value| {
                selector_value(tcx, body, value, source, selectors, active)
            });
            active.remove(&place.local);
            accepted
        }
        _ => false,
    }
}

fn unit_enum_type(value: Ty<'_>) -> bool {
    matches!(value.kind(), ty::Adt(owner, _)
        if owner.is_enum() && owner.variants().iter().all(|variant| variant.fields.is_empty()))
}
