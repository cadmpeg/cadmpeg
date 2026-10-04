// SPDX-License-Identifier: Apache-2.0
//! Prove declared-length serializers that emit every borrowed map entry once.

use crate::types;
use rustc_middle::mir::{
    self, BasicBlock, BorrowKind, Local, Operand, Place, Rvalue, StatementKind, TerminatorKind,
    UnwindAction, RETURN_PLACE,
};
use rustc_middle::ty::{self, Instance, Ty, TyCtxt};
use rustc_span::def_id::DefId;
use rustc_span::DUMMY_SP;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, PartialEq, Eq)]
enum MapKind {
    BTree,
    Json,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CallKind {
    SerializeMap,
    SerializeEntry,
    End,
    TryBranch,
    FromResidual,
    Length,
    IntoIterator,
    Next,
    FixedGetter,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Projection {
    Deref,
    Field(usize),
    Variant(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SourcePlace(Vec<Projection>);

#[derive(Clone, PartialEq, Eq)]
enum Origin {
    Source(SourcePlace),
    Fixed,
    Serializer,
    Cardinality(SourcePlace, usize),
    Iterator(Local),
    NextOption(Local),
    NextItem(Local),
    NextKey(Local),
    NextValue(Local),
    OperationResult(Local),
    BranchResult(Local),
    BranchOutput(Local),
    BranchResidual(Local),
    BranchTag(Local),
    NextTag(Local),
    CheckedOverflow,
    Tuple(Vec<Origin>),
    Some(Box<Origin>),
}

struct Call<'tcx> {
    kind: CallKind,
    definition: DefId,
    generic_args: ty::GenericArgsRef<'tcx>,
    block: BasicBlock,
    destination: Local,
    arguments: Vec<Operand<'tcx>>,
    helper: Option<Instance<'tcx>>,
    unwind: UnwindAction,
}

struct Drop<'tcx> {
    block: BasicBlock,
    place: Place<'tcx>,
    unwind: UnwindAction,
}

struct Branch {
    operation_block: BasicBlock,
    block: BasicBlock,
    switch: BasicBlock,
    break_target: BasicBlock,
    continue_target: BasicBlock,
    residual: BasicBlock,
}

/// Return the source value types emitted by a declared-length map serializer.
/// The projection owner checks these types with its active recursion state.
pub(crate) fn declared_map_field_types<'tcx>(
    tcx: TyCtxt<'tcx>,
    source: Ty<'tcx>,
    method: DefId,
) -> Option<Vec<Ty<'tcx>>> {
    let instance = crate::structural_derived::serialize_instance(tcx, source, method)?;
    let concrete_body = crate::structural_derived::instantiated_body(tcx, instance);
    let body = &concrete_body;
    if body.arg_count != 2
        || !matches!(
            body.local_decls.get(Local::from_usize(1))?.ty.kind(),
            ty::Ref(_, inner, ty::Mutability::Not) if *inner == source.peel_refs()
        )
    {
        return None;
    }

    let reachable = reachable_blocks(body);
    let (calls, drops) = collect_calls(tcx, body, &reachable)?;
    let start = exactly_one(&calls, CallKind::SerializeMap)?;
    let end = exactly_one(&calls, CallKind::End)?;
    let length = exactly_one(&calls, CallKind::Length)?;
    let into_iter = exactly_one(&calls, CallKind::IntoIterator)?;
    let next = exactly_one(&calls, CallKind::Next)?;
    let entries: Vec<_> = calls
        .iter()
        .filter(|call| call.kind == CallKind::SerializeEntry)
        .collect();
    if entries.is_empty() || start.arguments.len() != 2 || end.arguments.len() != 1 {
        return None;
    }

    let map_place = source_map_place(tcx, body, length, into_iter, &reachable, &calls)?;
    let (map_kind, key_ty, value_ty) = map_type(tcx, map_place.1)?;
    if !map_calls_match(tcx, body, map_kind, key_ty, value_ty, length, into_iter, next) {
        return None;
    }
    let cardinality = option_cardinality(tcx, body, &start.arguments[1], &reachable, &calls)?;
    if cardinality.0 != map_place.0
        || !count_assertions_are_bounded(
            tcx,
            body,
            &reachable,
            &calls,
            &cardinality.0,
            cardinality.1,
            start,
        )
    {
        return None;
    }

    let loop_blocks = loop_component(body, next.block, &reachable);
    if loop_blocks.is_empty()
        || !cycle_through(body, next.block, &reachable)
        || loop_blocks.contains(&length.block)
        || loop_blocks.contains(&start.block)
        || loop_blocks.contains(&into_iter.block)
        || loop_blocks.contains(&end.block)
        || calls
            .iter()
            .filter(|call| call.kind == CallKind::Next)
            .any(|call| !loop_blocks.contains(&call.block))
    {
        return None;
    }
    let entry = entries
        .iter()
        .find(|call| loop_blocks.contains(&call.block))
        .copied()?;
    if entries
        .iter()
        .filter(|call| loop_blocks.contains(&call.block))
        .count()
        != 1
    {
        return None;
    }
    let prefix_entries: Vec<_> = entries
        .iter()
        .copied()
        .filter(|call| !loop_blocks.contains(&call.block))
        .collect();
    if cardinality.1 != prefix_entries.len() {
        return None;
    }

    if !protocol_call_shapes(tcx, body, start, end, entry, &entries, &reachable, &calls)
        || !fixed_getter_calls_are_bounded(tcx, body, &reachable, &calls)
    {
        return None;
    }

    let dominators = dominators(body, &reachable);
    if !dominates(&dominators, length.block, start.block)
        || !dominates(&dominators, start.block, into_iter.block)
        || !dominates(&dominators, into_iter.block, next.block)
        || !dominates(&dominators, into_iter.block, end.block)
        || !dominates(&dominators, start.block, next.block)
        || !dominates(&dominators, start.block, end.block)
        || prefix_entries
            .iter()
            .any(|field| !dominates(&dominators, start.block, field.block)
                || !dominates(&dominators, field.block, into_iter.block)
                || !dominates(&dominators, field.block, next.block))
        || !dominates(&dominators, next.block, entry.block)
    {
        return None;
    }

    let mut result_calls = Vec::with_capacity(entries.len().checked_add(1)?);
    result_calls.push(start);
    result_calls.extend(entries.iter().copied());
    let branches = result_branches(tcx, body, &reachable, &calls, &result_calls)?;
    if !branch_refusals_propagate(
        tcx,
        body,
        &reachable,
        &branches,
        next,
        start,
        &calls,
    )
        || !protocol_success_paths(
            tcx,
            body,
            start,
            into_iter,
            next,
            entry,
            &prefix_entries,
            &branches,
            &reachable,
            &calls,
        )
        || !loop_emits_every_item(
            tcx,
            body,
            start,
            next,
            entry,
            end,
            &loop_blocks,
            &branches,
            &reachable,
            &calls,
        )
        || !acyclic_without_block(body, &reachable, next.block)
        || !terminal_results_are_unchanged(
            tcx, body, &reachable, end, &calls, start, next,
        )
    {
        return None;
    }

    let mut emitted = Vec::with_capacity(cardinality.1.checked_add(2)?);
    for prefix in &prefix_entries {
        if !fixed_prefix_entry(tcx, body, prefix, start, &reachable, &calls) {
            return None;
        }
        emitted.push(prefix.arguments[2].ty(body, tcx).peel_refs());
    }
    if !loop_entry_types(tcx, body, entry, next, key_ty, value_ty, &reachable, &calls) {
        return None;
    }
    emitted.push(key_ty);
    emitted.push(value_ty);

    if !statements_are_bounded(tcx, body, &reachable, &calls)
        || !drops_are_bounded(tcx, body, &reachable, &drops, &calls, start, next)
        || !cleanup_paths_are_bounded(
            tcx,
            body,
            &reachable,
            &drops,
            start,
            next,
            &calls,
        )
    {
        return None;
    }
    Some(emitted)
}

fn collect_calls<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    reachable: &HashSet<BasicBlock>,
) -> Option<(Vec<Call<'tcx>>, Vec<Drop<'tcx>>)> {
    let mut calls = Vec::new();
    let mut drops = Vec::new();
    for block in reachable {
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
                let generic_args = (*generic_arguments).no_bound_vars()?;
                let kind = classify_call(tcx, body, *definition, args)?;
                let helper = if kind == CallKind::FixedGetter {
                    resolve_helper(tcx, *definition, generic_args)
                } else {
                    None
                };
                if kind == CallKind::FixedGetter && helper.is_none() {
                    return None;
                }
                let destination = destination.local;
                match kind {
                    CallKind::SerializeMap
                    | CallKind::SerializeEntry
                    | CallKind::Length
                    | CallKind::IntoIterator
                    | CallKind::Next
                    | CallKind::FixedGetter
                        if destination == RETURN_PLACE =>
                    {
                        return None;
                    }
                    CallKind::End | CallKind::FromResidual
                        if destination != RETURN_PLACE =>
                    {
                        return None;
                    }
                    CallKind::TryBranch if destination == RETURN_PLACE => return None,
                    _ => (),
                }
                calls.push(Call {
                    kind,
                    definition: *definition,
                    generic_args,
                    block: *block,
                    destination,
                    arguments: args.iter().map(|argument| argument.node.clone()).collect(),
                    helper,
                    unwind: *unwind,
                });
            }
            TerminatorKind::Drop {
                place,
                unwind,
                ..
            } => drops.push(Drop {
                block: *block,
                place: place.clone(),
                unwind: *unwind,
            }),
            TerminatorKind::Goto { .. }
            | TerminatorKind::SwitchInt { .. }
            | TerminatorKind::Return
            | TerminatorKind::Unreachable
            | TerminatorKind::Assert { .. } => (),
            _ => return None,
        }
    }
    Some((calls, drops))
}

fn classify_call<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    definition: DefId,
    arguments: &[rustc_span::Spanned<Operand<'tcx>>],
) -> Option<CallKind> {
    let receiver = arguments.first()?.node.ty(body, tcx);
    if associated_trait_method(
        tcx,
        definition,
        &["serde", "serde_core"],
        &["ser", "Serializer"],
        "serialize_map",
    ) {
        Some(CallKind::SerializeMap)
    } else if associated_trait_method(
        tcx,
        definition,
        &["serde", "serde_core"],
        &["ser", "SerializeMap"],
        "serialize_entry",
    ) {
        Some(CallKind::SerializeEntry)
    } else if associated_trait_method(
        tcx,
        definition,
        &["serde", "serde_core"],
        &["ser", "SerializeMap"],
        "end",
    ) {
        Some(CallKind::End)
    } else if is_trait_method(
        tcx,
        definition,
        "branch",
        "core",
        &["ops", "try_trait", "Try"],
    )
    {
        Some(CallKind::TryBranch)
    } else if is_trait_method(
        tcx,
        definition,
        "from_residual",
        "core",
        &["ops", "try_trait", "FromResidual"],
    )
    {
        Some(CallKind::FromResidual)
    } else if is_map_length_method(tcx, definition, receiver) {
        Some(CallKind::Length)
    } else if is_trait_method(
        tcx,
        definition,
        "into_iter",
        "core",
        &["iter", "traits", "collect", "IntoIterator"],
    ) {
        Some(CallKind::IntoIterator)
    } else if is_trait_method(
        tcx,
        definition,
        "next",
        "core",
        &["iter", "traits", "iterator", "Iterator"],
    ) {
        Some(CallKind::Next)
    } else if matches!(
        tcx.def_kind(definition),
        rustc_hir::def::DefKind::Fn | rustc_hir::def::DefKind::AssocFn
    ) && tcx.is_mir_available(definition)
    {
        Some(CallKind::FixedGetter)
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
    let Some(trait_item) = trait_item_for_method(tcx, definition) else {
        return false;
    };
    tcx.opt_item_name(trait_item)
        .is_some_and(|name| name.as_str() == method)
        && tcx.trait_of_assoc(trait_item).is_some_and(|trait_id| {
            crates
                .iter()
                .any(|crate_name| types::physical_item_path(tcx, trait_id, crate_name, trait_path))
        })
}

fn trait_item_for_method(tcx: TyCtxt<'_>, definition: DefId) -> Option<DefId> {
    if tcx.trait_of_assoc(definition).is_some() {
        Some(definition)
    } else {
        tcx.trait_item_of(definition)
    }
}

fn is_trait_method(
    tcx: TyCtxt<'_>,
    definition: DefId,
    method: &str,
    crate_name: &str,
    trait_path: &[&str],
) -> bool {
    let Some(trait_item) = trait_item_for_method(tcx, definition) else {
        return false;
    };
    tcx.opt_item_name(trait_item)
        .is_some_and(|name| name.as_str() == method)
        && tcx.trait_of_assoc(trait_item).is_some_and(|trait_id| {
            types::physical_item_path(tcx, trait_id, crate_name, trait_path)
        })
}

fn is_map_length_method<'tcx>(tcx: TyCtxt<'tcx>, definition: DefId, receiver: Ty<'tcx>) -> bool {
    if map_type(tcx, receiver).is_none() {
        return false;
    }
    types::physical_inherent_method(
        tcx,
        definition,
        "alloc",
        &["collections", "btree", "map", "BTreeMap"],
        "len",
    ) || types::physical_inherent_method(
        tcx,
        definition,
        "serde_json",
        &["map", "Map"],
        "len",
    )
}

fn map_type<'tcx>(tcx: TyCtxt<'tcx>, map_ty: Ty<'tcx>) -> Option<(MapKind, Ty<'tcx>, Ty<'tcx>)> {
    let ty::Adt(owner, arguments) = map_ty.peel_refs().kind() else {
        return None;
    };
    let kind = if types::physical_item_path(
        tcx,
        owner.did(),
        "alloc",
        &["collections", "btree", "map", "BTreeMap"],
    ) {
        MapKind::BTree
    } else if types::physical_item_path(tcx, owner.did(), "serde_json", &["map", "Map"]) {
        MapKind::Json
    } else {
        return None;
    };
    let mut type_arguments = arguments.types();
    let key = type_arguments.next()?;
    let value_ty = type_arguments.next()?;
    if kind == MapKind::Json {
        let children = crate::structural_json::source_children(tcx, map_ty.peel_refs())?;
        if !children.charged_parent
            || children.children.len() != 2
            || children.children[0] != key
            || children.children[1] != value_ty
        {
            return None;
        }
    }
    Some((kind, key, value_ty))
}

fn supported_iterator_type(tcx: TyCtxt<'_>, kind: MapKind, value: Ty<'_>) -> bool {
    let ty::Adt(owner, _) = value.peel_refs().kind() else {
        return false;
    };
    match kind {
        MapKind::BTree => types::physical_item_path(
            tcx,
            owner.did(),
            "alloc",
            &["collections", "btree", "map", "Iter"],
        ),
        MapKind::Json => types::physical_item_path(tcx, owner.did(), "serde_json", &["map", "Iter"]),
    }
}

fn map_calls_match<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    kind: MapKind,
    key: Ty<'tcx>,
    value: Ty<'tcx>,
    length: &Call<'tcx>,
    into_iter: &Call<'tcx>,
    next: &Call<'tcx>,
) -> bool {
    if length.arguments.len() != 1 || into_iter.arguments.len() != 1 || next.arguments.len() != 1 {
        return false;
    }
    let receiver = into_iter.arguments[0].ty(body, tcx).peel_refs();
    if map_type(tcx, receiver).is_none_or(|(receiver_kind, receiver_key, receiver_value)| {
        receiver_kind != kind || receiver_key != key || receiver_value != value
    }) {
        return false;
    }
    let iterator = body.local_decls[into_iter.destination].ty;
    if !supported_iterator_type(tcx, kind, iterator) {
        return false;
    }
    let next_receiver = next.arguments[0].ty(body, tcx).peel_refs();
    if next_receiver != iterator.peel_refs()
        || !iterator_impl_matches(tcx, body, into_iter, kind, true)
        || !iterator_impl_matches(tcx, body, next, kind, false)
    {
        return false;
    }
    let next_result = body.local_decls[next.destination].ty;
    let ty::Adt(option, option_args) = next_result.kind() else {
        return false;
    };
    if !types::physical_item_path(tcx, option.did(), "core", &["option", "Option"]) {
        return false;
    }
    let Some(item) = option_args.types().next() else {
        return false;
    };
    let ty::Tuple(fields) = item.kind() else {
        return false;
    };
    fields.len() == 2
        && matches!(fields[0].kind(), ty::Ref(_, inner, ty::Mutability::Not) if *inner == key)
        && matches!(fields[1].kind(), ty::Ref(_, inner, ty::Mutability::Not) if *inner == value)
}

fn iterator_impl_matches<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    call: &Call<'tcx>,
    kind: MapKind,
    into_iterator: bool,
) -> bool {
    let Some(trait_item) = trait_item_for_method(tcx, call.definition) else {
        return false;
    };
    let Some(trait_id) = tcx.trait_of_assoc(trait_item) else {
        return false;
    };
    let trait_path = if into_iterator {
        &["iter", "traits", "collect", "IntoIterator"][..]
    } else {
        &["iter", "traits", "iterator", "Iterator"][..]
    };
    let method = if into_iterator { "into_iter" } else { "next" };
    if !types::physical_item_path(tcx, trait_id, "core", trait_path)
        || tcx
            .opt_item_name(trait_item)
            .is_none_or(|name| name.as_str() != method)
    {
        return false;
    }
    let environment = ty::TypingEnv::fully_monomorphized();
    let Some(arguments) = tcx
        .try_normalize_erasing_regions(
            environment,
            ty::Unnormalized::new_wip(call.generic_args),
        )
        .ok()
    else {
        return false;
    };
    let Ok(Some(instance)) = Instance::try_resolve(
        tcx,
        environment,
        call.definition,
        arguments,
    ) else {
        return false;
    };
    if tcx.trait_item_of(instance.def_id()) != Some(trait_item) {
        return false;
    }
    let Some(implementation) = tcx.trait_impl_of_assoc(instance.def_id()) else {
        return false;
    };
    let implementation_arg_count = tcx.generics_of(implementation).count();
    if tcx.generics_of(instance.def_id()).parent_count != implementation_arg_count
        || instance.args.len() < implementation_arg_count
    {
        return false;
    }
    let implementation_args =
        tcx.mk_args_from_iter(instance.args.iter().take(implementation_arg_count));
    let trait_ref = tcx
        .impl_trait_ref(implementation)
        .instantiate(tcx, implementation_args)
        .skip_norm_wip();
    if trait_ref.def_id != trait_id {
        return false;
    }
    let self_ty = trait_ref.self_ty();
    let Some(receiver) = call.arguments.first() else {
        return false;
    };
    let receiver_ty = receiver.ty(body, tcx);
    let expected_self_ty = if into_iterator {
        receiver_ty
    } else {
        receiver_ty.peel_refs()
    };
    if tcx.erase_and_anonymize_regions(self_ty)
        != tcx.erase_and_anonymize_regions(expected_self_ty)
    {
        return false;
    }
    if into_iterator {
        let ty::Ref(_, map, ty::Mutability::Not) = self_ty.kind() else {
            return false;
        };
        map_type(tcx, *map).is_some_and(|(actual, _, _)| actual == kind)
    } else {
        supported_iterator_type(tcx, kind, self_ty)
    }
}

fn source_map_place<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    length: &Call<'tcx>,
    into_iter: &Call<'tcx>,
    reachable: &HashSet<BasicBlock>,
    calls: &[Call<'tcx>],
) -> Option<(SourcePlace, Ty<'tcx>)> {
    if length.arguments.len() != 1 || into_iter.arguments.len() != 1 {
        return None;
    }
    let mut active = HashSet::new();
    let Origin::Source(length_place) = operand_origin(
        tcx,
        body,
        &length.arguments[0],
        reachable,
        calls,
        &mut active,
    )?
    else {
        return None;
    };
    let Origin::Source(iterator_place) = operand_origin(
        tcx,
        body,
        &into_iter.arguments[0],
        reachable,
        calls,
        &mut HashSet::new(),
    )?
    else {
        return None;
    };
    if length_place != iterator_place {
        return None;
    }
    let map_ty = into_iter.arguments[0].ty(body, tcx).peel_refs();
    map_type(tcx, map_ty)?;
    Some((length_place, map_ty))
}

fn option_cardinality<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    operand: &Operand<'tcx>,
    reachable: &HashSet<BasicBlock>,
    calls: &[Call<'tcx>],
) -> Option<(SourcePlace, usize)> {
    let origin = operand_origin(
        tcx,
        body,
        operand,
        reachable,
        calls,
        &mut HashSet::new(),
    )?;
    match origin {
        Origin::Some(cardinality) => match *cardinality {
            Origin::Cardinality(source, extra) => Some((source, extra)),
            _ => None,
        },
        _ => cardinality_from_option_local(tcx, body, operand, reachable, calls),
    }
}

fn cardinality_from_option_local<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    operand: &Operand<'tcx>,
    reachable: &HashSet<BasicBlock>,
    calls: &[Call<'tcx>],
) -> Option<(SourcePlace, usize)> {
    let (Operand::Copy(place) | Operand::Move(place)) = operand else {
        return None;
    };
    if !place.projection.is_empty() {
        return None;
    }
    let value = unique_assignment(body, reachable, place.local)?;
    let Rvalue::Aggregate(kind, operands) = value else {
        return None;
    };
    let mir::AggregateKind::Adt(definition, variant, _, _, _) = &**kind else {
        return None;
    };
    if !types::physical_item_path(tcx, *definition, "core", &["option", "Option"])
        || tcx.adt_def(*definition).variant(*variant).name.as_str() != "Some"
        || operands.len() != 1
    {
        return None;
    }
    cardinality_of_operand(
        tcx,
        body,
        operands.iter().next()?,
        reachable,
        calls,
        &mut HashSet::new(),
    )
}

fn cardinality_of_operand<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    operand: &Operand<'tcx>,
    reachable: &HashSet<BasicBlock>,
    calls: &[Call<'tcx>],
    active: &mut HashSet<Local>,
) -> Option<(SourcePlace, usize)> {
    let (Operand::Copy(place) | Operand::Move(place)) = operand else {
        return None;
    };
    if place.projection.is_empty() {
        if let Some(call) = calls
            .iter()
            .find(|call| call.destination == place.local && call.kind == CallKind::Length)
        {
            if call.arguments.len() != 1 {
                return None;
            }
            return match operand_origin(
                tcx,
                body,
                &call.arguments[0],
                reachable,
                calls,
                &mut HashSet::new(),
            )? {
                Origin::Source(source) => Some((source, 0)),
                _ => None,
            };
        }
        if !active.insert(place.local) {
            return None;
        }
        let result = unique_assignment(body, reachable, place.local).and_then(|assignment| {
            match assignment {
                Rvalue::Use(source, _) => {
                    cardinality_of_operand(tcx, body, source, reachable, calls, active)
                }
                _ => None,
            }
        });
        active.remove(&place.local);
        return result;
    }

    let [mir::ProjectionElem::Field(field, _)] = place.projection.as_slice() else {
        return None;
    };
    if field.as_usize() != 0
        || !matches!(
            body.local_decls[place.local].ty.kind(),
            ty::Tuple(fields)
                if fields.len() == 2
                    && fields[0] == tcx.types.usize
                    && matches!(fields[1].kind(), ty::Bool)
        )
        || !active.insert(place.local)
    {
        return None;
    }
    let Some(Rvalue::BinaryOp(mir::BinOp::AddWithOverflow, operands)) =
        unique_assignment(body, reachable, place.local)
    else {
        active.remove(&place.local);
        return None;
    };
    let result = sum_cardinality(tcx, body, &operands.0, &operands.1, reachable, calls, active);
    active.remove(&place.local);
    result
}

fn unique_assignment<'a, 'tcx>(
    body: &'a mir::Body<'tcx>,
    reachable: &HashSet<BasicBlock>,
    local: Local,
) -> Option<&'a Rvalue<'tcx>> {
    let mut assignment = None;
    for block in reachable {
        for statement in &body.basic_blocks[*block].statements {
            let StatementKind::Assign(value) = &statement.kind else {
                continue;
            };
            let (target, rvalue) = &**value;
            if target.local != local {
                continue;
            }
            if !target.projection.is_empty() || assignment.replace(rvalue).is_some() {
                return None;
            }
        }
    }
    assignment
}

fn constant_usize<'tcx>(tcx: TyCtxt<'tcx>, operand: &Operand<'tcx>) -> Option<usize> {
    let Operand::Constant(constant) = operand else {
        return None;
    };
    constant
        .const_
        .try_eval_target_usize(tcx, ty::TypingEnv::fully_monomorphized())
        .and_then(|value| usize::try_from(value).ok())
}

fn count_assertions_are_bounded<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    reachable: &HashSet<BasicBlock>,
    calls: &[Call<'tcx>],
    source: &SourcePlace,
    extra: usize,
    start: &Call<'tcx>,
) -> bool {
    let dominators = dominators(body, reachable);
    let mut additions = HashMap::new();
    for block in reachable {
        for statement in &body.basic_blocks[*block].statements {
            let StatementKind::Assign(assignment) = &statement.kind else {
                continue;
            };
            let (target, value) = &**assignment;
            let Rvalue::BinaryOp(mir::BinOp::AddWithOverflow, operands) = value else {
                continue;
            };
            if !target.projection.is_empty()
                || !matches!(
                    body.local_decls[target.local].ty.kind(),
                    ty::Tuple(fields)
                        if fields.len() == 2
                            && fields[0] == tcx.types.usize
                            && matches!(fields[1].kind(), ty::Bool)
                )
            {
                return false;
            }
            let Some(addition) = sum_cardinality(
                tcx,
                body,
                &operands.0,
                &operands.1,
                reachable,
                calls,
                &mut HashSet::new(),
            ) else {
                return false;
            };
            if addition.0 != *source
                || addition.1 > extra
                || additions.insert(target.local, (*block, addition)).is_some()
            {
                return false;
            }
        }
    }

    for block in reachable {
        let TerminatorKind::Assert {
            cond,
            expected,
            msg,
            unwind,
            ..
        } = &body.basic_blocks[*block].terminator().kind
        else {
            continue;
        };
        let Some(local) = overflow_flag_local(cond) else {
            return false;
        };
        let Some((recorded_block, addition)) = additions.remove(&local) else {
            return false;
        };
        if *block != recorded_block
            || cond.ty(body, tcx) != tcx.types.bool
            || addition.0 != *source
            || addition.1 > extra
        {
            return false;
        }
        if *expected
            || !dominates(&dominators, *block, start.block)
            || !matches!(
                unwind,
                UnwindAction::Cleanup(_)
                    | UnwindAction::Unreachable
                    | UnwindAction::Terminate(_)
            )
        {
            return false;
        }
        let mir::AssertKind::Overflow(mir::BinOp::Add, assert_left, assert_right) = &**msg else {
            return false;
        };
        let mut checked_operands = None;
        for addition_block in reachable {
            for statement in &body.basic_blocks[*addition_block].statements {
                let StatementKind::Assign(assignment) = &statement.kind else {
                    continue;
                };
                let (target, value) = &**assignment;
                if target.local != local {
                    continue;
                }
                if !target.projection.is_empty() || checked_operands.is_some() {
                    return false;
                }
                let Rvalue::BinaryOp(mir::BinOp::AddWithOverflow, operands) = value else {
                    return false;
                };
                checked_operands = Some((*addition_block, &operands.0, &operands.1));
            }
        }
        let Some((addition_block, checked_left, checked_right)) = checked_operands else {
            return false;
        };
        if addition_block != *block
            || !same_count_operand(tcx, checked_left, assert_left)
            || !same_count_operand(tcx, checked_right, assert_right)
        {
            return false;
        }
    }
    additions.is_empty()
}

fn overflow_flag_local(condition: &Operand<'_>) -> Option<Local> {
    let (Operand::Copy(place) | Operand::Move(place)) = condition else {
        return None;
    };
    if place.projection.len() != 1 {
        return None;
    }
    matches!(
        place.projection.first(),
        Some(mir::ProjectionElem::Field(index, _)) if index.as_usize() == 1
    )
    .then_some(place.local)
}

fn same_count_operand<'tcx>(
    tcx: TyCtxt<'tcx>,
    left: &Operand<'tcx>,
    right: &Operand<'tcx>,
) -> bool {
    match (left, right) {
        (Operand::Copy(left), Operand::Copy(right))
        | (Operand::Copy(left), Operand::Move(right))
        | (Operand::Move(left), Operand::Copy(right))
        | (Operand::Move(left), Operand::Move(right)) => left == right,
        (Operand::Constant(left), Operand::Constant(right)) => {
            left.const_.ty() == right.const_.ty()
                && left.const_.ty() == tcx.types.usize
                && matches!(
                    (
                        left.const_.try_eval_target_usize(
                            tcx,
                            ty::TypingEnv::fully_monomorphized(),
                        ),
                        right.const_.try_eval_target_usize(
                            tcx,
                            ty::TypingEnv::fully_monomorphized(),
                        ),
                    ),
                    (Some(left), Some(right)) if left == right
                )
        }
        _ => false,
    }
}

fn sum_cardinality<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    left: &Operand<'tcx>,
    right: &Operand<'tcx>,
    reachable: &HashSet<BasicBlock>,
    calls: &[Call<'tcx>],
    active: &mut HashSet<Local>,
) -> Option<(SourcePlace, usize)> {
    let left_constant = constant_usize(tcx, left);
    let right_constant = constant_usize(tcx, right);
    let left_count = left_constant
        .is_none()
        .then(|| cardinality_of_operand(tcx, body, left, reachable, calls, active))
        .flatten();
    let right_count = right_constant
        .is_none()
        .then(|| cardinality_of_operand(tcx, body, right, reachable, calls, active))
        .flatten();
    match (left_count, right_count, left_constant, right_constant) {
        (Some((source, previous)), None, None, Some(extra))
        | (None, Some((source, previous)), Some(extra), None) => {
            previous.checked_add(extra).map(|extra| (source, extra))
        }
        _ => None,
    }
}

fn protocol_call_shapes<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    start: &Call<'tcx>,
    end: &Call<'tcx>,
    loop_entry: &Call<'tcx>,
    entries: &[&Call<'tcx>],
    reachable: &HashSet<BasicBlock>,
    calls: &[Call<'tcx>],
) -> bool {
    if start.arguments.len() != 2
        || end.arguments.len() != 1
        || !parameter_operand(&start.arguments[0], 2)
    {
        return false;
    }
    let Some(branch) = branch_for_operation(tcx, body, start, reachable, calls) else {
        return false;
    };
    let state = Origin::BranchOutput(branch.destination);
    if !operand_has_origin(tcx, body, &end.arguments[0], state.clone(), reachable, calls)
        || !operand_has_origin(tcx, body, &loop_entry.arguments[0], state.clone(), reachable, calls)
    {
        return false;
    }
    entries.iter().all(|entry| {
        entry.arguments.len() == 3
            && operand_has_origin(tcx, body, &entry.arguments[0], state.clone(), reachable, calls)
    })
}

fn branch_for_operation<'a, 'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    operation: &Call<'tcx>,
    reachable: &HashSet<BasicBlock>,
    calls: &'a [Call<'tcx>],
) -> Option<&'a Call<'tcx>> {
    let mut branches = calls.iter().filter(|call| {
        call.kind == CallKind::TryBranch
            && call.arguments.len() == 1
            && operand_has_origin(
                tcx,
                body,
                &call.arguments[0],
                Origin::OperationResult(operation.destination),
                reachable,
                calls,
            )
    });
    let branch = branches.next()?;
    branches.next().is_none().then_some(branch)
}

fn operand_has_origin<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    operand: &Operand<'tcx>,
    expected: Origin,
    reachable: &HashSet<BasicBlock>,
    calls: &[Call<'tcx>],
) -> bool {
    operand_origin(
        tcx,
        body,
        operand,
        reachable,
        calls,
        &mut HashSet::new(),
    ) == Some(expected)
}

fn fixed_prefix_entry<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    entry: &Call<'tcx>,
    start: &Call<'tcx>,
    reachable: &HashSet<BasicBlock>,
    calls: &[Call<'tcx>],
) -> bool {
    if entry.arguments.len() != 3 || !static_str_operand(tcx, &entry.arguments[1]) {
        return false;
    }
    let Some(branch) = branch_for_operation(tcx, body, start, reachable, calls) else {
        return false;
    };
    operand_has_origin(
        tcx,
        body,
        &entry.arguments[0],
        Origin::BranchOutput(branch.destination),
        reachable,
        calls,
    ) && matches!(
        operand_origin(
            tcx,
            body,
            &entry.arguments[2],
            reachable,
            calls,
            &mut HashSet::new(),
        ),
        Some(Origin::Source(_)) | Some(Origin::Fixed)
    )
}

fn loop_entry_types<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    entry: &Call<'tcx>,
    next: &Call<'tcx>,
    key_ty: Ty<'tcx>,
    value_ty: Ty<'tcx>,
    reachable: &HashSet<BasicBlock>,
    calls: &[Call<'tcx>],
) -> bool {
    if entry.arguments.len() != 3 {
        return false;
    }
    let key = operand_origin(
        tcx,
        body,
        &entry.arguments[1],
        reachable,
        calls,
        &mut HashSet::new(),
    );
    let value = operand_origin(
        tcx,
        body,
        &entry.arguments[2],
        reachable,
        calls,
        &mut HashSet::new(),
    );
    key == Some(Origin::NextKey(next.destination))
        && value == Some(Origin::NextValue(next.destination))
        && entry.arguments[1].ty(body, tcx).peel_refs() == key_ty
        && entry.arguments[2].ty(body, tcx).peel_refs() == value_ty
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
        Operand::Constant(constant)
            if constant
                .const_
                .eval(tcx, ty::TypingEnv::fully_monomorphized(), DUMMY_SP)
                .is_ok()
                && fixed_constant_type(operand.ty(body, tcx)) =>
        {
            Some(Origin::Fixed)
        }
        Operand::Constant(_) | Operand::RuntimeChecks(_) => None,
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
        return source_place(body, place);
    }
    if place.local == Local::from_usize(2) && place.projection.is_empty() {
        return Some(Origin::Serializer);
    }
    if let Some(call) = calls
        .iter()
        .find(|call| call.destination == place.local)
    {
        let origin = call_origin(tcx, body, call, reachable, calls, active)?;
        return project_origin(tcx, body, place.local, &place.projection, origin);
    }
    let origin = local_origin(tcx, body, place.local, reachable, calls, active)?;
    project_origin(tcx, body, place.local, &place.projection, origin)
}

fn source_place<'tcx>(body: &mir::Body<'tcx>, place: &Place<'tcx>) -> Option<Origin> {
    let mut current = body.local_decls[place.local].ty;
    let mut path = Vec::new();
    let mut source_deref = false;
    let mut source_field = false;
    for projection in place.projection.iter() {
        match projection {
            mir::ProjectionElem::Deref => {
                let ty::Ref(_, inner, ty::Mutability::Not) = current.kind() else {
                    return None;
                };
                current = *inner;
                source_deref = true;
                path.push(Projection::Deref);
            }
            mir::ProjectionElem::Field(index, field_ty) => {
                if !source_deref {
                    return None;
                }
            current = field_ty;
                source_field = true;
                path.push(Projection::Field(index.as_usize()));
            }
            mir::ProjectionElem::Downcast(_, index) if source_deref => {
                path.push(Projection::Variant(index.as_usize()));
            }
            _ => return None,
        }
    }
    (source_deref && source_field).then_some(Origin::Source(SourcePlace(path)))
}

fn project_origin<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    local: Local,
    projections: &[mir::PlaceElem<'tcx>],
    mut origin: Origin,
) -> Option<Origin> {
    if projections.is_empty() {
        return Some(origin);
    }
    let mut next_option_variant = None;
    let mut branch_variant = None;
    for projection in projections {
        match projection {
            mir::ProjectionElem::Deref => match &origin {
                Origin::Source(_) | Origin::Iterator(_) => (),
                _ => return None,
            },
            mir::ProjectionElem::Downcast(_, variant) => {
                match &origin {
                    Origin::BranchResult(branch) if *branch == local => {
                        let ty::Adt(owner, _) = body.local_decls[local].ty.kind() else {
                            return None;
                        };
                        if !types::physical_item_path(
                            tcx,
                            owner.did(),
                            "core",
                            &["ops", "control_flow", "ControlFlow"],
                        ) {
                            return None;
                        }
                        branch_variant = Some(owner.variant(*variant).name.as_str().to_owned());
                    }
                    Origin::NextOption(next) if *next == local => {
                        let ty::Adt(owner, _) = body.local_decls[local].ty.kind() else {
                            return None;
                        };
                        if !types::physical_item_path(
                            tcx,
                            owner.did(),
                            "core",
                            &["option", "Option"],
                        ) {
                            return None;
                        }
                        let name = owner.variant(*variant).name.as_str().to_owned();
                        if name != "Some" {
                            return None;
                        }
                        next_option_variant = Some(name);
                    }
                    _ => return None,
                }
            }
            mir::ProjectionElem::Field(index, _) => {
                let field = index.as_usize();
                origin = match origin {
                    Origin::BranchResult(branch) if branch == local => match branch_variant.as_deref() {
                        Some("Continue") if field == 0 => Origin::BranchOutput(branch),
                        Some("Break") if field == 0 => Origin::BranchResidual(branch),
                        _ => return None,
                    },
                    Origin::NextOption(next) if next == local => {
                        if next_option_variant.as_deref() != Some("Some") || field != 0 {
                            return None;
                        }
                        Origin::NextItem(next)
                    }
                    Origin::NextItem(next) => match field {
                        0 => Origin::NextKey(next),
                        1 => Origin::NextValue(next),
                        _ => return None,
                    },
                    Origin::Tuple(fields) => fields.get(field)?.clone(),
                    Origin::Source(mut source) => {
                        source.0.push(Projection::Field(field));
                        Origin::Source(source)
                    }
                    _ => return None,
                };
            }
            _ => return None,
        }
    }
    Some(origin)
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
    let mut current = None;
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
            let next = rvalue_origin(tcx, body, value, reachable, calls, active)?;
            current = Some(merge_origin(current, next)?);
            found = true;
        }
    }
    active.remove(&local);
    found.then_some(current?)
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
        Rvalue::Ref(_, BorrowKind::Shared, place) | Rvalue::CopyForDeref(place) => {
            place_origin(tcx, body, place, reachable, calls, active)
        }
        Rvalue::Ref(_, BorrowKind::Mut { .. }, place) => {
            let origin = place_origin(tcx, body, place, reachable, calls, active)?;
            (!matches!(origin, Origin::Source(_))).then_some(origin)
        }
        Rvalue::Cast(
            mir::CastKind::PointerCoercion(ty::adjustment::PointerCoercion::Unsize, _),
            operand,
            _,
        ) => operand_origin(tcx, body, operand, reachable, calls, active),
        Rvalue::Aggregate(kind, operands) if !matches!(**kind, mir::AggregateKind::Closure(..)) => {
            match &**kind {
                mir::AggregateKind::Tuple => Some(Origin::Tuple(
                    operands
                        .iter()
                        .map(|operand| {
                            operand_origin(tcx, body, operand, reachable, calls, active)
                        })
                        .collect::<Option<Vec<_>>>()?,
                )),
                mir::AggregateKind::Adt(definition, variant, _, _, _)
                    if types::physical_item_path(
                        tcx,
                        *definition,
                        "core",
                        &["option", "Option"],
                    ) =>
                {
                    let name = tcx.adt_def(*definition).variant(*variant).name.as_str();
                    if name == "Some" && operands.len() == 1 {
                        Some(Origin::Some(Box::new(operand_origin(
                            tcx,
                            body,
                            operands.iter().next()?,
                            reachable,
                            calls,
                            active,
                        )?)))
                    } else if name == "None" && operands.is_empty() {
                        Some(Origin::Fixed)
                    } else {
                        None
                    }
                }
                mir::AggregateKind::Adt(definition, _, _, _, _)
                    if tcx.adt_def(*definition).is_struct()
                        && tcx.adt_def(*definition).non_enum_variant().fields.len()
                            == operands.len() =>
                {
                    Some(Origin::Tuple(
                        operands
                            .iter()
                            .map(|operand| {
                                operand_origin(tcx, body, operand, reachable, calls, active)
                            })
                            .collect::<Option<Vec<_>>>()?,
                    ))
                }
                _ => None,
            }
        }
        Rvalue::Discriminant(place) => match place_origin(
            tcx, body, place, reachable, calls, active,
        )? {
            Origin::BranchResult(branch) => Some(Origin::BranchTag(branch)),
            Origin::NextOption(next) => Some(Origin::NextTag(next)),
            _ => None,
        },
        Rvalue::BinaryOp(mir::BinOp::Add, operands) => {
            let left = operand_origin(tcx, body, &operands.0, reachable, calls, active)?;
            let right = operand_origin(tcx, body, &operands.1, reachable, calls, active)?;
            add_cardinality(tcx, &operands.0, left, &operands.1, right)
        }
        Rvalue::BinaryOp(mir::BinOp::AddWithOverflow, operands) => {
            let left = operand_origin(tcx, body, &operands.0, reachable, calls, active)?;
            let right = operand_origin(tcx, body, &operands.1, reachable, calls, active)?;
            let Origin::Cardinality(source, count) =
                add_cardinality(tcx, &operands.0, left, &operands.1, right)?
            else {
                return None;
            };
            Some(Origin::Tuple(vec![
                Origin::Cardinality(source.clone(), count),
                Origin::CheckedOverflow,
            ]))
        }
        Rvalue::UnaryOp(_, operand) => {
            let origin = operand_origin(tcx, body, operand, reachable, calls, active)?;
            matches!(origin, Origin::Fixed).then_some(origin)
        }
        _ => None,
    }
}

fn add_cardinality<'tcx>(
    tcx: TyCtxt<'tcx>,
    left_operand: &Operand<'tcx>,
    left: Origin,
    right_operand: &Operand<'tcx>,
    right: Origin,
) -> Option<Origin> {
    match (left, right, constant_usize(tcx, left_operand), constant_usize(tcx, right_operand)) {
        (Origin::Cardinality(source, previous), Origin::Fixed, None, Some(extra))
        | (Origin::Fixed, Origin::Cardinality(source, previous), Some(extra), None) => {
            previous.checked_add(extra).map(|extra| Origin::Cardinality(source, extra))
        }
        _ => None,
    }
}

fn call_origin<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    call: &Call<'tcx>,
    reachable: &HashSet<BasicBlock>,
    calls: &[Call<'tcx>],
    active: &mut HashSet<Local>,
) -> Option<Origin> {
    match call.kind {
        CallKind::SerializeMap | CallKind::SerializeEntry => {
            Some(Origin::OperationResult(call.destination))
        }
        CallKind::TryBranch => Some(Origin::BranchResult(call.destination)),
        CallKind::Length => {
            let argument = call.arguments.first()?;
            match operand_origin(tcx, body, argument, reachable, calls, active)? {
                Origin::Source(source) => Some(Origin::Cardinality(source, 0)),
                _ => None,
            }
        }
        CallKind::IntoIterator => Some(Origin::Iterator(call.destination)),
        CallKind::Next => Some(Origin::NextOption(call.destination)),
        CallKind::FixedGetter => fixed_getter_origin(tcx, body, call, reachable, calls),
        CallKind::End | CallKind::FromResidual => None,
    }
}

fn fixed_getter_origin<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    call: &Call<'tcx>,
    reachable: &HashSet<BasicBlock>,
    calls: &[Call<'tcx>],
) -> Option<Origin> {
    if call.arguments.len() != 1 {
        return None;
    }
    let receiver = operand_origin(
        tcx,
        body,
        &call.arguments[0],
        reachable,
        calls,
        &mut HashSet::new(),
    )?;
    let Origin::Source(_) = receiver else {
        return None;
    };
    let definition = call.helper?.def_id();
    let output = body.local_decls[call.destination].ty;
    if crate::structural_scalar::fixed_string_borrow(
        tcx,
        definition,
        call.arguments[0].ty(body, tcx),
        output,
    ) {
        return Some(receiver);
    }
    fixed_getter_body(tcx, call.helper?, receiver)
}

fn fixed_getter_body<'tcx>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
    receiver: Origin,
) -> Option<Origin> {
    if !matches!(receiver, Origin::Source(_)) || !tcx.is_mir_available(instance.def_id()) {
        return None;
    }
    let body = tcx.instance_mir(instance.def);
    if body.arg_count != 1
        || crate::conversion::cyclic(body)
        || !matches!(
            body.local_decls.get(Local::from_usize(1))?.ty.kind(),
            ty::Ref(_, _, ty::Mutability::Not)
        )
    {
        return None;
    }
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
                        || target.local.as_usize() <= body.arg_count
                        || getter_rvalue_origin(
                            tcx,
                            body,
                            value,
                            &receiver,
                            &mut HashSet::new(),
                        )
                        .is_none()
                    {
                        return None;
                    }
                }
                _ => return None,
            }
        }
        if !matches!(
            &body.basic_blocks[*block].terminator().kind,
            TerminatorKind::Goto { .. } | TerminatorKind::Return
        ) {
            return None;
        }
    }
    let output = getter_local_origin(tcx, body, RETURN_PLACE, &receiver, &mut HashSet::new())?;
    matches!(output, Origin::Source(_)).then_some(output)
}

fn getter_local_origin<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    local: Local,
    receiver: &Origin,
    active: &mut HashSet<Local>,
) -> Option<Origin> {
    if local.as_usize() == 1 {
        return Some(receiver.clone());
    }
    if !active.insert(local) {
        return None;
    }
    let mut result = None;
    let mut found = false;
    for block in body.basic_blocks.indices() {
        for statement in &body.basic_blocks[block].statements {
            let StatementKind::Assign(assignment) = &statement.kind else {
                continue;
            };
            let (target, value) = &**assignment;
            if target.local == local {
                if !target.projection.is_empty() {
                    active.remove(&local);
                    return None;
                }
                let next = getter_rvalue_origin(tcx, body, value, receiver, active)?;
                result = Some(merge_origin(result, next)?);
                found = true;
            }
        }
    }
    active.remove(&local);
    found.then_some(result?)
}

fn getter_rvalue_origin<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    value: &Rvalue<'tcx>,
    receiver: &Origin,
    active: &mut HashSet<Local>,
) -> Option<Origin> {
    match value {
        Rvalue::Use(operand, _) => getter_operand_origin(tcx, body, operand, receiver, active),
        Rvalue::Ref(_, BorrowKind::Shared, place) | Rvalue::CopyForDeref(place) => {
            getter_place_origin(tcx, body, place, receiver, active)
        }
        Rvalue::Cast(
            mir::CastKind::PointerCoercion(ty::adjustment::PointerCoercion::Unsize, _),
            operand,
            _,
        ) => getter_operand_origin(tcx, body, operand, receiver, active),
        _ => None,
    }
}

fn getter_operand_origin<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    operand: &Operand<'tcx>,
    receiver: &Origin,
    active: &mut HashSet<Local>,
) -> Option<Origin> {
    match operand {
        Operand::Copy(place) | Operand::Move(place) => {
            getter_place_origin(tcx, body, place, receiver, active)
        }
        Operand::Constant(constant)
            if constant
                .const_
                .eval(tcx, ty::TypingEnv::fully_monomorphized(), DUMMY_SP)
                .is_ok()
                && fixed_constant_type(operand.ty(body, tcx)) =>
        {
            Some(Origin::Fixed)
        }
        _ => None,
    }
}

fn getter_place_origin<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    place: &Place<'tcx>,
    receiver: &Origin,
    active: &mut HashSet<Local>,
) -> Option<Origin> {
    if place.local.as_usize() == 1 {
        let mut current = body.local_decls[place.local].ty;
        let mut saw_deref = false;
        let Origin::Source(mut source) = receiver.clone() else {
            return None;
        };
        for projection in place.projection.iter() {
            match projection {
                mir::ProjectionElem::Deref => {
                    let ty::Ref(_, inner, ty::Mutability::Not) = current.kind() else {
                        return None;
                    };
                    current = *inner;
                    saw_deref = true;
                    source.0.push(Projection::Deref);
                }
                mir::ProjectionElem::Field(index, field_ty) if saw_deref => {
                    current = field_ty;
                    source.0.push(Projection::Field(index.as_usize()));
                }
                _ => return None,
            }
        }
        return Some(Origin::Source(source));
    }
    if !place.projection.is_empty() {
        return None;
    }
    getter_local_origin(tcx, body, place.local, receiver, active)
}

fn resolve_helper<'tcx>(
    tcx: TyCtxt<'tcx>,
    definition: DefId,
    arguments: ty::GenericArgsRef<'tcx>,
) -> Option<Instance<'tcx>> {
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

fn merge_origin(current: Option<Origin>, next: Origin) -> Option<Origin> {
    match current {
        None => Some(next),
        Some(previous) if previous == next => Some(previous),
        _ => None,
    }
}

fn fixed_constant_type(value: Ty<'_>) -> bool {
    match value.kind() {
        ty::Bool | ty::Char | ty::Int(_) | ty::Uint(_) | ty::Float(_) => true,
        ty::Ref(_, inner, ty::Mutability::Not) if matches!(inner.kind(), ty::Str) => true,
        _ => false,
    }
}

fn result_branches<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    reachable: &HashSet<BasicBlock>,
    calls: &[Call<'tcx>],
    operations: &[&Call<'tcx>],
) -> Option<Vec<Branch>> {
    if calls.iter().filter(|call| call.kind == CallKind::TryBranch).count() != operations.len()
        || calls
            .iter()
            .filter(|call| call.kind == CallKind::FromResidual)
            .count()
            != operations.len()
    {
        return None;
    }
    let mut branches = Vec::with_capacity(operations.len());
    let mut used_branches = HashSet::new();
    let mut used_residuals = HashSet::new();
    for operation in operations {
        let branch_call = branch_for_operation(tcx, body, operation, reachable, calls)?;
        if !used_branches.insert(branch_call.destination) || branch_call.arguments.len() != 1 {
            return None;
        }
        let switches: Vec<_> = reachable
            .iter()
            .filter_map(|block| {
                let TerminatorKind::SwitchInt { discr, targets } =
                    &body.basic_blocks[*block].terminator().kind
                else {
                    return None;
                };
                (operand_origin(
                    tcx,
                    body,
                    discr,
                    reachable,
                    calls,
                    &mut HashSet::new(),
                ) == Some(Origin::BranchTag(branch_call.destination)))
                .then_some((*block, targets))
            })
            .collect();
        if switches.len() != 1 {
            return None;
        }
        let (switch, targets) = switches[0];
        let (break_target, continue_target) =
            control_flow_targets(tcx, body, branch_call.destination, targets)?;
        let residuals: Vec<_> = calls
            .iter()
            .filter(|call| {
                call.kind == CallKind::FromResidual
                    && call.arguments.len() == 1
                    && operand_has_origin(
                        tcx,
                        body,
                        &call.arguments[0],
                        Origin::BranchResidual(branch_call.destination),
                        reachable,
                        calls,
                    )
            })
            .collect();
        if residuals.len() != 1 || !used_residuals.insert(residuals[0].block) {
            return None;
        }
        branches.push(Branch {
            operation_block: operation.block,
            block: branch_call.block,
            switch,
            break_target,
            continue_target,
            residual: residuals[0].block,
        });
    }
    (used_residuals.len() == operations.len()).then_some(branches)
}

fn branch_refusals_propagate<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    reachable: &HashSet<BasicBlock>,
    branches: &[Branch],
    next: &Call<'tcx>,
    start: &Call<'tcx>,
    calls: &[Call<'tcx>],
) -> bool {
    branches.iter().all(|branch| {
        dominates_in_body(body, reachable, branch.operation_block, branch.block)
            && dominates_in_body(body, reachable, branch.block, branch.switch)
            && every_path_reaches_block(
                tcx,
                body,
                branch.break_target,
                branch.residual,
                Some(next.block),
                reachable,
                calls,
                start,
                next,
                true,
                &mut HashSet::new(),
                &mut HashSet::new(),
            )
    })
}

fn protocol_success_paths<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    start: &Call<'tcx>,
    into_iter: &Call<'tcx>,
    next: &Call<'tcx>,
    loop_entry: &Call<'tcx>,
    prefix_entries: &[&Call<'tcx>],
    branches: &[Branch],
    reachable: &HashSet<BasicBlock>,
    calls: &[Call<'tcx>],
) -> bool {
    let dominators = dominators(body, reachable);
    let mut ordered_prefix = prefix_entries.to_vec();
    ordered_prefix.sort_by_key(|call| {
        dominators.get(&call.block).map_or(0, |set| set.len())
    });
    if ordered_prefix.windows(2).any(|pair| {
        !dominates(&dominators, pair[0].block, pair[1].block)
    }) {
        return false;
    }

    let Some(event_capacity) = ordered_prefix.len().checked_add(2) else {
        return false;
    };
    let mut events = Vec::with_capacity(event_capacity);
    events.push(start.block);
    events.extend(ordered_prefix.iter().map(|call| call.block));
    events.push(into_iter.block);
    for pair in events.windows(2) {
        let Some(branch) = branches
            .iter()
            .find(|branch| branch.operation_block == pair[0])
        else {
            return false;
        };
        if !every_path_reaches_block(
            tcx,
            body,
            branch.continue_target,
            pair[1],
            Some(next.block),
            reachable,
            calls,
            start,
            next,
            false,
            &mut HashSet::new(),
            &mut HashSet::new(),
        ) {
            return false;
        }
    }
    let Some(loop_branch) = branches
        .iter()
        .find(|branch| branch.operation_block == loop_entry.block)
    else {
        return false;
    };
    every_path_reaches_block(
        tcx,
        body,
        loop_branch.continue_target,
        next.block,
        None,
        reachable,
        calls,
        start,
        next,
        false,
        &mut HashSet::new(),
        &mut HashSet::new(),
    )
}

fn fixed_getter_calls_are_bounded<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    reachable: &HashSet<BasicBlock>,
    calls: &[Call<'tcx>],
) -> bool {
    calls
        .iter()
        .filter(|call| call.kind == CallKind::FixedGetter)
        .all(|call| fixed_getter_origin(tcx, body, call, reachable, calls).is_some())
}

fn every_path_reaches_block<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    start_block: BasicBlock,
    target: BasicBlock,
    blocked: Option<BasicBlock>,
    reachable: &HashSet<BasicBlock>,
    calls: &[Call<'tcx>],
    map_start: &Call<'tcx>,
    next: &Call<'tcx>,
    allow_state_drop: bool,
    active: &mut HashSet<BasicBlock>,
    complete: &mut HashSet<BasicBlock>,
) -> bool {
    if start_block == target {
        return true;
    }
    if blocked == Some(start_block)
        || !reachable.contains(&start_block)
        || active.contains(&start_block)
    {
        return false;
    }
    if complete.contains(&start_block) {
        return true;
    }
    if !active.insert(start_block) {
        return false;
    }
    let no_unapproved_call = calls
        .iter()
        .filter(|call| call.block == start_block)
        .all(|call| call.kind == CallKind::FixedGetter);
    let next_blocks: Vec<_> = match &body.basic_blocks[start_block].terminator().kind {
        TerminatorKind::Goto { target } => vec![*target],
        TerminatorKind::Call {
            target: Some(target),
            ..
        } if no_unapproved_call => vec![*target],
        TerminatorKind::SwitchInt { targets, .. } => targets.all_targets().to_vec(),
        TerminatorKind::Drop {
            place,
            target,
            unwind,
            ..
        } => {
            let drop = Drop {
                block: start_block,
                place: place.clone(),
                unwind: *unwind,
            };
            let drop_is_bounded = if allow_state_drop {
                place_drop_is_bounded(tcx, body, &drop, reachable, calls, map_start, next)
            } else {
                place.projection.is_empty()
                    && fixed_drop_type(tcx, place.ty(body, tcx).ty, &mut HashSet::new())
            };
            if !drop_is_bounded {
                active.remove(&start_block);
                return false;
            }
            vec![*target]
        }
        _ => {
            active.remove(&start_block);
            return false;
        }
    };
    let bounded = no_unapproved_call
        && !next_blocks.is_empty()
        && next_blocks.into_iter().all(|successor| {
            every_path_reaches_block(
                tcx,
                body,
                successor,
                target,
                blocked,
                reachable,
                calls,
                map_start,
                next,
                allow_state_drop,
                active,
                complete,
            )
        });
    active.remove(&start_block);
    if bounded {
        complete.insert(start_block);
    }
    bounded
}

fn loop_emits_every_item<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    start: &Call<'tcx>,
    next: &Call<'tcx>,
    entry: &Call<'tcx>,
    end: &Call<'tcx>,
    loop_blocks: &HashSet<BasicBlock>,
    branches: &[Branch],
    reachable: &HashSet<BasicBlock>,
    calls: &[Call<'tcx>],
) -> bool {
    let Some(into_iter) = calls
        .iter()
        .find(|call| call.kind == CallKind::IntoIterator)
    else {
        return false;
    };
    if next.arguments.len() != 1
        || !operand_has_origin(
            tcx,
            body,
            &next.arguments[0],
            Origin::Iterator(into_iter.destination),
            reachable,
            calls,
        )
    {
        return false;
    }
    let switches: Vec<_> = loop_blocks
        .iter()
        .filter_map(|block| {
            let TerminatorKind::SwitchInt { discr, targets } =
                &body.basic_blocks[*block].terminator().kind
            else {
                return None;
            };
            (operand_origin(
                tcx,
                body,
                discr,
                reachable,
                calls,
                &mut HashSet::new(),
            ) == Some(Origin::NextTag(next.destination)))
            .then_some((*block, targets))
        })
        .collect();
    if switches.len() != 1 {
        return false;
    }
    let (switch, targets) = switches[0];
    let Some((some_target, none_target)) =
        option_switch_targets(tcx, body, next.destination, targets)
    else {
        return false;
    };
    if !loop_blocks.contains(&some_target)
        || loop_blocks.contains(&none_target)
        || !every_path_reaches_block(
            tcx,
            body,
            some_target,
            entry.block,
            Some(next.block),
            reachable,
            calls,
            start,
            next,
            false,
            &mut HashSet::new(),
            &mut HashSet::new(),
        )
        || !every_path_reaches_block(
            tcx,
            body,
            none_target,
            end.block,
            Some(next.block),
            reachable,
            calls,
            start,
            next,
            false,
            &mut HashSet::new(),
            &mut HashSet::new(),
        )
        || loop_blocks.contains(&end.block)
        || !dominates_in_body(body, reachable, next.block, switch)
    {
        return false;
    }

    if calls.iter().any(|call| {
        loop_blocks.contains(&call.block)
            && !matches!(
                call.kind,
                CallKind::Next | CallKind::SerializeEntry | CallKind::TryBranch
            )
    }) {
        return false;
    }
    let option_switch = Some(switch);
    if loop_blocks.iter().any(|block| {
        let TerminatorKind::SwitchInt { discr, .. } =
            &body.basic_blocks[*block].terminator().kind
        else {
            return false;
        };
        if Some(*block) == option_switch {
            return false;
        }
        !matches!(
            operand_origin(
                tcx,
                body,
                discr,
                reachable,
                calls,
                &mut HashSet::new(),
            ),
            Some(Origin::BranchTag(_))
        )
    }) {
        return false;
    }

    branches
        .iter()
        .filter(|branch| loop_blocks.contains(&branch.operation_block))
        .all(|branch| {
            loop_blocks.contains(&branch.continue_target)
                && !loop_blocks.contains(&branch.break_target)
        })
}

fn option_switch_targets<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    option_local: Local,
    targets: &mir::SwitchTargets,
) -> Option<(BasicBlock, BasicBlock)> {
    let ty::Adt(owner, _) = body.local_decls[option_local].ty.kind() else {
        return None;
    };
    if !types::physical_item_path(tcx, owner.did(), "core", &["option", "Option"])
        || owner.variants().len() != 2
    {
        return None;
    }
    let some = owner
        .variants()
        .iter_enumerated()
        .find(|(_, variant)| variant.name.as_str() == "Some")?
        .0;
    let none = owner
        .variants()
        .iter_enumerated()
        .find(|(_, variant)| variant.name.as_str() == "None")?
        .0;
    let some_value = owner.discriminant_for_variant(tcx, some).val;
    let none_value = owner.discriminant_for_variant(tcx, none).val;
    let some_target = targets.target_for_value(some_value);
    let none_target = targets.target_for_value(none_value);
    (some_target != none_target).then_some((some_target, none_target))
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
    if !types::physical_item_path(
        tcx,
        owner.did(),
        "core",
        &["ops", "control_flow", "ControlFlow"],
    ) || owner.variants().len() != 2
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

fn terminal_results_are_unchanged<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    reachable: &HashSet<BasicBlock>,
    end: &Call<'tcx>,
    calls: &[Call<'tcx>],
    start: &Call<'tcx>,
    next: &Call<'tcx>,
) -> bool {
    if end.destination != RETURN_PLACE
        || calls
            .iter()
            .filter(|call| call.kind == CallKind::End)
            .count()
            != 1
    {
        return false;
    }
    let terminals: Vec<_> = calls
        .iter()
        .filter(|call| call.kind == CallKind::End || call.kind == CallKind::FromResidual)
        .collect();
    if terminals.is_empty() || terminals.iter().any(|call| call.destination != RETURN_PLACE) {
        return false;
    }
    let mut returned = HashSet::new();
    {
        let mut traversal = TerminalPath {
            tcx,
            body,
            reachable,
            calls,
            start,
            next,
            active: HashSet::new(),
            complete: HashSet::new(),
            returned: &mut returned,
        };
        for call in terminals {
            let TerminatorKind::Call {
                target: Some(target),
                ..
            } = &body.basic_blocks[call.block].terminator().kind
            else {
                return false;
            };
            if !traversal.reaches_return(*target) {
                return false;
            }
        }
    }
    let actual: HashSet<_> = reachable
        .iter()
        .copied()
        .filter(|block| matches!(&body.basic_blocks[*block].terminator().kind, TerminatorKind::Return))
        .collect();
    returned == actual
}

struct TerminalPath<'a, 'tcx> {
    tcx: TyCtxt<'tcx>,
    body: &'a mir::Body<'tcx>,
    reachable: &'a HashSet<BasicBlock>,
    calls: &'a [Call<'tcx>],
    start: &'a Call<'tcx>,
    next: &'a Call<'tcx>,
    active: HashSet<BasicBlock>,
    complete: HashSet<BasicBlock>,
    returned: &'a mut HashSet<BasicBlock>,
}

impl TerminalPath<'_, '_> {
    fn reaches_return(&mut self, block: BasicBlock) -> bool {
        if !self.reachable.contains(&block) || self.active.contains(&block) {
            return false;
        }
        if self.complete.contains(&block) {
            return true;
        }
        self.active.insert(block);
        let data = &self.body.basic_blocks[block];
        let statements_are_bounded = data.statements.iter().all(|statement| {
            matches!(
                &statement.kind,
                StatementKind::StorageLive(_)
                    | StatementKind::StorageDead(_)
                    | StatementKind::Nop
                    | StatementKind::FakeRead(_)
            ) || bool_drop_flag_statement(self.tcx, self.body, statement)
        });
        let terminator = data.terminator().kind.clone();
        let bounded = if !statements_are_bounded {
            false
        } else {
            match &terminator {
                TerminatorKind::Return => {
                    self.returned.insert(block);
                    true
                }
                TerminatorKind::Goto { target } => self.reaches_return(*target),
                TerminatorKind::Drop {
                    place,
                    target,
                    unwind,
                    ..
                } => {
                    let drop = Drop {
                        block,
                        place: place.clone(),
                        unwind: *unwind,
                    };
                    place_drop_is_bounded(
                        self.tcx,
                        self.body,
                        &drop,
                        self.reachable,
                        self.calls,
                        self.start,
                        self.next,
                    ) && self.reaches_return(*target)
                }
                TerminatorKind::SwitchInt { discr, targets }
                    if bool_switch_discriminant_is_bounded(
                        self.tcx,
                        self.body,
                        self.reachable,
                        discr,
                    ) =>
                {
                    targets
                        .all_targets()
                        .iter()
                        .all(|target| self.reaches_return(*target))
                }
                _ => false,
            }
        };
        self.active.remove(&block);
        if bounded {
            self.complete.insert(block);
        }
        bounded
    }
}

fn bool_drop_flag_statement<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    statement: &mir::Statement<'tcx>,
) -> bool {
    let StatementKind::Assign(assignment) = &statement.kind else {
        return false;
    };
    let (target, value) = &**assignment;
    target.local.as_usize() > body.arg_count
        && bool_drop_flag_statement_shape(tcx, body, target, value)
        && bool_drop_flag_is_literal_and_unaliased(tcx, body, target.local)
}

fn bool_drop_flag_is_literal_and_unaliased<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    local: Local,
) -> bool {
    if local.as_usize() <= body.arg_count
        || !matches!(body.local_decls[local].ty.kind(), ty::Bool)
    {
        return false;
    }
    let mut assigned = false;
    let mut switched = false;
    for data in body.basic_blocks.iter() {
        match &data.terminator().kind {
            TerminatorKind::Call {
                args, destination, ..
            } if destination.local == local
                || args.iter().any(|argument| {
                    matches!(
                        &argument.node,
                        Operand::Copy(place) | Operand::Move(place) if place.local == local
                    )
                }) =>
            {
                return false;
            }
            TerminatorKind::InlineAsm { .. } => return false,
            TerminatorKind::SwitchInt {
                discr: Operand::Copy(place) | Operand::Move(place),
                ..
            } if place.local == local && place.projection.is_empty() => switched = true,
            _ => (),
        }
        for statement in &data.statements {
            if let StatementKind::Assign(assignment) = &statement.kind {
                let (target, value) = &**assignment;
                if target.local == local {
                    if !bool_drop_flag_statement_shape(tcx, body, target, value) {
                        return false;
                    }
                    assigned = true;
                }
                if matches!(
                    value,
                    Rvalue::Ref(_, BorrowKind::Mut { .. }, place)
                        | Rvalue::RawPtr(_, place)
                        if place.local == local
                ) {
                    return false;
                }
            } else if matches!(
                &statement.kind,
                StatementKind::SetDiscriminant { place, .. } if place.local == local
            ) {
                return false;
            }
        }
    }
    assigned && switched
}

fn bool_drop_flag_statement_shape<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    target: &Place<'tcx>,
    value: &Rvalue<'tcx>,
) -> bool {
    target.projection.is_empty()
        && matches!(body.local_decls[target.local].ty.kind(), ty::Bool)
        && matches!(
            value,
            Rvalue::Use(Operand::Constant(constant), _)
                if constant.const_.try_eval_bool(
                    tcx,
                    ty::TypingEnv::fully_monomorphized(),
                ).is_some()
        )
}

fn statements_are_bounded<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    reachable: &HashSet<BasicBlock>,
    calls: &[Call<'tcx>],
) -> bool {
    reachable.iter().all(|block| {
        body.basic_blocks[*block]
            .statements
            .iter()
            .all(|statement| match &statement.kind {
                StatementKind::StorageLive(_)
                | StatementKind::StorageDead(_)
                | StatementKind::Nop
                | StatementKind::FakeRead(_) => true,
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
                _ => false,
            })
    })
}

fn drops_are_bounded<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    reachable: &HashSet<BasicBlock>,
    drops: &[Drop<'tcx>],
    calls: &[Call<'tcx>],
    start: &Call<'tcx>,
    next: &Call<'tcx>,
) -> bool {
    drops.iter().all(|drop| {
        if !reachable.contains(&drop.block)
            || !place_drop_is_bounded(tcx, body, drop, reachable, calls, start, next)
        {
            return false;
        }
        if let UnwindAction::Cleanup(cleanup) = drop.unwind {
            if reachable.contains(&cleanup) {
                return false;
            }
        }
        true
    })
}

fn place_drop_is_bounded<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    drop: &Drop<'tcx>,
    reachable: &HashSet<BasicBlock>,
    calls: &[Call<'tcx>],
    start: &Call<'tcx>,
    next: &Call<'tcx>,
) -> bool {
    if !drop.place.projection.is_empty() {
        return false;
    }
    let dropped_ty = body.local_decls[drop.place.local].ty;
    let origin = place_origin(
        tcx,
        body,
        &drop.place,
        reachable,
        calls,
        &mut HashSet::new(),
    );
    let serializer = Local::from_usize(2);
    if !reachable.contains(&drop.block)
        && dropped_ty == body.local_decls[serializer].ty
        && origin == Some(Origin::Serializer)
    {
        return true;
    }
    if is_serializer_state_type(tcx, body, start, dropped_ty)
        && matches!(origin, Some(Origin::BranchOutput(_)))
    {
        return true;
    }
    let Some(into_iter) = calls.iter().find(|call| call.kind == CallKind::IntoIterator) else {
        return false;
    };
    if origin == Some(Origin::Iterator(into_iter.destination))
        && iterator_drop_type(tcx, body, dropped_ty, into_iter, next)
    {
        return true;
    }
    fixed_drop_type(tcx, dropped_ty, &mut HashSet::new())
}

fn is_serializer_state_type<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    start: &Call<'tcx>,
    dropped: Ty<'tcx>,
) -> bool {
    let result_ty = body.local_decls[start.destination].ty;
    let ty::Adt(result, arguments) = result_ty.kind() else {
        return false;
    };
    if !types::physical_item_path(tcx, result.did(), "core", &["result", "Result"]) {
        return false;
    }
    let Some(state) = arguments.types().next() else {
        return false;
    };
    let ty::Alias(_, alias) = state.kind() else {
        return false;
    };
    let ty::AliasTyKind::Projection { def_id } = alias.kind else {
        return false;
    };
    tcx.item_name(def_id).as_str() == "SerializeMap"
        && tcx.trait_of_assoc(def_id).is_some_and(|trait_id| {
            ["serde", "serde_core"].iter().any(|crate_name| {
                types::physical_item_path(tcx, trait_id, crate_name, &["ser", "Serializer"])
            })
        })
        && alias.args.types().next()
            == body.local_decls.get(Local::from_usize(2)).map(|local| local.ty)
        && dropped == state
}

fn iterator_drop_type<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    value: Ty<'tcx>,
    into_iter: &Call<'tcx>,
    next: &Call<'tcx>,
) -> bool {
    let iterator = body.local_decls[into_iter.destination].ty.peel_refs();
    let next_receiver = next
        .arguments
        .first()
        .map(|argument| argument.ty(body, tcx).peel_refs());
    next_receiver == Some(iterator)
        && iterator == value.peel_refs()
        && supported_iterator_type_for_any_map(tcx, iterator)
        && fixed_drop_type(tcx, value, &mut HashSet::new())
}

fn supported_iterator_type_for_any_map(tcx: TyCtxt<'_>, value: Ty<'_>) -> bool {
    supported_iterator_type(tcx, MapKind::BTree, value)
        || supported_iterator_type(tcx, MapKind::Json, value)
}

fn fixed_drop_type<'tcx>(
    tcx: TyCtxt<'tcx>,
    value: Ty<'tcx>,
    active: &mut HashSet<Ty<'tcx>>,
) -> bool {
    if !active.insert(value) {
        return false;
    }
    let bounded = match value.kind() {
        ty::Bool
        | ty::Char
        | ty::Int(_)
        | ty::Uint(_)
        | ty::Float(_)
        | ty::Ref(..)
        | ty::RawPtr(..)
        | ty::FnPtr(..)
        | ty::FnDef(..) => true,
        ty::Array(element, _) | ty::Slice(element) => fixed_drop_type(tcx, *element, active),
        ty::Tuple(fields) => fields.iter().all(|field| fixed_drop_type(tcx, field, active)),
        ty::Adt(owner, arguments) => {
            if owner.is_union() || owner.has_dtor(tcx) {
                false
            } else if types::physical_item_path(tcx, owner.did(), "core", &["option", "Option"])
                || types::physical_item_path(tcx, owner.did(), "core", &["result", "Result"])
                || types::physical_item_path(
                    tcx,
                    owner.did(),
                    "core",
                    &["ops", "control_flow", "ControlFlow"],
                )
            {
                arguments
                    .types()
                    .all(|field| fixed_drop_type(tcx, field, active))
            } else if supported_iterator_type_for_any_map(tcx, value) {
                true
            } else {
                false
            }
        }
        _ => false,
    };
    active.remove(&value);
    bounded
}

fn cleanup_paths_are_bounded<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    reachable: &HashSet<BasicBlock>,
    drops: &[Drop<'tcx>],
    start: &Call<'tcx>,
    next: &Call<'tcx>,
    calls: &[Call<'tcx>],
) -> bool {
    let mut pending: Vec<_> = calls
        .iter()
        .filter_map(|call| match call.unwind {
            UnwindAction::Cleanup(cleanup) => Some(cleanup),
            _ => None,
        })
        .chain(drops.iter().filter_map(|drop| match drop.unwind {
            UnwindAction::Cleanup(cleanup) => Some(cleanup),
            _ => None,
        }))
        .chain(reachable.iter().filter_map(|block| {
            match &body.basic_blocks[*block].terminator().kind {
                TerminatorKind::Assert {
                    unwind: UnwindAction::Cleanup(cleanup),
                    ..
                } => Some(*cleanup),
                _ => None,
            }
        }))
        .collect();
    let mut active = HashSet::new();
    let mut complete = HashSet::new();
    while let Some(block) = pending.pop() {
        if !cleanup_path_is_bounded(
            tcx,
            body,
            reachable,
            calls,
            start,
            next,
            block,
            &mut active,
            &mut complete,
        ) {
            return false;
        }
    }
    true
}

fn cleanup_path_is_bounded<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    reachable: &HashSet<BasicBlock>,
    calls: &[Call<'tcx>],
    start: &Call<'tcx>,
    next: &Call<'tcx>,
    block: BasicBlock,
    active: &mut HashSet<BasicBlock>,
    complete: &mut HashSet<BasicBlock>,
) -> bool {
    if reachable.contains(&block) || active.contains(&block) {
        return false;
    }
    if complete.contains(&block) {
        return true;
    }
    if !active.insert(block) {
        return false;
    }
    let data = &body.basic_blocks[block];
    let statements_are_empty = data.statements.iter().all(|statement| {
        matches!(
            &statement.kind,
            StatementKind::StorageLive(_)
                | StatementKind::StorageDead(_)
                | StatementKind::Nop
                | StatementKind::FakeRead(_)
        )
    });
    if !statements_are_empty {
        active.remove(&block);
        return false;
    }
    let bounded = match &data.terminator().kind {
        TerminatorKind::Goto { target } => cleanup_path_is_bounded(
            tcx, body, reachable, calls, start, next, *target, active, complete,
        ),
        TerminatorKind::SwitchInt { discr, targets }
            if bool_switch_discriminant_is_bounded(tcx, body, reachable, discr) =>
        {
            targets.all_targets().iter().all(|target| {
                cleanup_path_is_bounded(
                    tcx, body, reachable, calls, start, next, *target, active, complete,
                )
            })
        }
        TerminatorKind::Drop {
            place,
            target,
            unwind,
            ..
        } => {
            let drop = Drop {
                block,
                place: place.clone(),
                unwind: *unwind,
            };
            place_drop_is_bounded(tcx, body, &drop, reachable, calls, start, next)
                && cleanup_path_is_bounded(
                    tcx, body, reachable, calls, start, next, *target, active, complete,
                )
                && match unwind {
                    UnwindAction::Cleanup(cleanup) => cleanup_path_is_bounded(
                        tcx, body, reachable, calls, start, next, *cleanup, active, complete,
                    ),
                    UnwindAction::Continue
                    | UnwindAction::Unreachable
                    | UnwindAction::Terminate(_) => true,
                }
        }
        TerminatorKind::UnwindResume
        | TerminatorKind::UnwindTerminate(_)
        | TerminatorKind::Unreachable => true,
        _ => false,
    };
    active.remove(&block);
    if bounded {
        complete.insert(block);
    }
    bounded
}

fn bool_switch_discriminant_is_bounded<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    reachable: &HashSet<BasicBlock>,
    discriminant: &Operand<'tcx>,
) -> bool {
    if discriminant.ty(body, tcx) != tcx.types.bool {
        return false;
    }
    let (Operand::Copy(place) | Operand::Move(place)) = discriminant else {
        return matches!(
            discriminant,
            Operand::Constant(constant)
                if constant.const_.ty() == tcx.types.bool
                    && constant
                        .const_
                        .eval(tcx, ty::TypingEnv::fully_monomorphized(), DUMMY_SP)
                        .is_ok()
        );
    };
    if !place.projection.is_empty()
        || place.local.as_usize() <= body.arg_count
        || !bool_drop_flag_is_literal_and_unaliased(tcx, body, place.local)
    {
        return false;
    }
    let mut assigned = false;
    for block in reachable {
        for statement in &body.basic_blocks[*block].statements {
            let StatementKind::Assign(assignment) = &statement.kind else {
                continue;
            };
            let (target, value) = &**assignment;
            if target.local != place.local {
                continue;
            }
            if !bool_drop_flag_statement_shape(tcx, body, target, value) {
                return false;
            }
            assigned = true;
        }
    }
    assigned
}

fn reachable_blocks(body: &mir::Body<'_>) -> HashSet<BasicBlock> {
    let mut reachable = HashSet::new();
    let mut pending = vec![mir::START_BLOCK];
    while let Some(block) = pending.pop() {
        if reachable.insert(block) {
            pending.extend(normal_successors(&body.basic_blocks[block].terminator().kind));
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
        }
        | TerminatorKind::Drop { target, .. }
        | TerminatorKind::Assert { target, .. } => vec![*target],
        TerminatorKind::SwitchInt { targets, .. } => targets.all_targets().to_vec(),
        _ => Vec::new(),
    }
}

fn loop_component(
    body: &mir::Body<'_>,
    header: BasicBlock,
    reachable: &HashSet<BasicBlock>,
) -> HashSet<BasicBlock> {
    reachable
        .iter()
        .copied()
        .filter(|candidate| {
            can_reach(body, header, *candidate, reachable)
                && can_reach(body, *candidate, header, reachable)
        })
        .collect()
}

fn cycle_through(
    body: &mir::Body<'_>,
    header: BasicBlock,
    reachable: &HashSet<BasicBlock>,
) -> bool {
    normal_successors(&body.basic_blocks[header].terminator().kind)
        .into_iter()
        .any(|successor| can_reach(body, successor, header, reachable))
}

fn acyclic_without_block(
    body: &mir::Body<'_>,
    reachable: &HashSet<BasicBlock>,
    excluded: BasicBlock,
) -> bool {
    let nodes: HashSet<_> = reachable
        .iter()
        .copied()
        .filter(|block| *block != excluded)
        .collect();
    let mut indegree: HashMap<_, usize> = nodes.iter().copied().map(|block| (block, 0)).collect();
    for block in &nodes {
        for successor in normal_successors(&body.basic_blocks[*block].terminator().kind) {
            if successor == excluded || !nodes.contains(&successor) {
                continue;
            }
            let Some(count) = indegree.get_mut(&successor) else {
                return false;
            };
            let Some(updated) = count.checked_add(1) else {
                return false;
            };
            *count = updated;
        }
    }
    let mut pending: Vec<_> = indegree
        .iter()
        .filter_map(|(block, count)| (*count == 0).then_some(*block))
        .collect();
    let mut visited = 0_usize;
    while let Some(block) = pending.pop() {
        visited += 1;
        for successor in normal_successors(&body.basic_blocks[block].terminator().kind) {
            if successor == excluded || !nodes.contains(&successor) {
                continue;
            }
            let Some(count) = indegree.get_mut(&successor) else {
                return false;
            };
            let Some(updated) = count.checked_sub(1) else {
                return false;
            };
            *count = updated;
            if *count == 0 {
                pending.push(successor);
            }
        }
    }
    visited == nodes.len()
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

fn dominates_in_body(
    body: &mir::Body<'_>,
    reachable: &HashSet<BasicBlock>,
    first: BasicBlock,
    second: BasicBlock,
) -> bool {
    dominates(&dominators(body, reachable), first, second)
}

fn can_reach(
    body: &mir::Body<'_>,
    start: BasicBlock,
    target: BasicBlock,
    allowed: &HashSet<BasicBlock>,
) -> bool {
    if !allowed.contains(&start) || !allowed.contains(&target) {
        return false;
    }
    let mut seen = HashSet::new();
    let mut pending = vec![start];
    while let Some(block) = pending.pop() {
        if block == target {
            return true;
        }
        if allowed.contains(&block) && seen.insert(block) {
            pending.extend(normal_successors(&body.basic_blocks[block].terminator().kind));
        }
    }
    false
}

fn exactly_one<'call, 'tcx>(
    calls: &'call [Call<'tcx>],
    kind: CallKind,
) -> Option<&'call Call<'tcx>> {
    let mut matching = calls.iter().filter(|call| call.kind == kind);
    let call = matching.next()?;
    matching.next().is_none().then_some(call)
}
