// SPDX-License-Identifier: Apache-2.0
//! Source and output-shape proofs for ordinary Serde-derived serializers.

use crate::{conversion, types};
use rustc_middle::mir::{
    self, BasicBlock, BinOp, BorrowKind, Local, Operand, Place, Rvalue, StatementKind,
    TerminatorKind, UnwindAction, RETURN_PLACE,
};
use rustc_middle::ty::{self, Instance, Ty, TyCtxt, TypeVisitableExt};
use rustc_span::def_id::DefId;
use rustc_span::DUMMY_SP;
use std::collections::{HashMap, HashSet};

/// A concrete value that a derived serializer passes to one of Serde's
/// bounded child protocols. The projection owner must admit `ty` with the same
/// recursion and emitted-node state that it uses for the source root.
#[derive(Clone, Copy)]
pub(crate) struct SerializedChild<'tcx> {
    pub(crate) ty: Ty<'tcx>,
    pub(crate) charged_parent: bool,
    pub(crate) mode: SerializerMode,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SerializerMode {
    Plain,
    Flattened,
    Tagged,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Struct,
    StructVariant,
    Map,
    Seq,
    Tuple,
    TupleStruct,
    TupleVariant,
    StructField,
    StructVariantField,
    SkipField,
    MapEntry,
    MapKey,
    MapValue,
    Element,
    TupleStructField,
    TupleVariantField,
    End,
    Serialize,
    Some,
    NewtypeStruct,
    NewtypeVariant,
    Unit,
    UnitStruct,
    UnitVariant,
    TryBranch,
    FromResidual,
    Predicate,
    TaggedNewtype,
}

impl Kind {
    fn creates_state(self) -> bool {
        matches!(
            self,
            Self::Struct
                | Self::StructVariant
                | Self::Map
                | Self::Seq
                | Self::Tuple
                | Self::TupleStruct
                | Self::TupleVariant
        )
    }

    fn takes_state(self) -> bool {
        matches!(
            self,
            Self::StructField
                | Self::StructVariantField
                | Self::SkipField
                | Self::MapEntry
                | Self::MapKey
                | Self::MapValue
                | Self::Element
                | Self::TupleStructField
                | Self::TupleVariantField
                | Self::End
        )
    }

    fn fallible(self) -> bool {
        self != Self::TryBranch && self != Self::Predicate && self != Self::FromResidual
    }
}

struct Call<'tcx> {
    kind: Kind,
    definition: DefId,
    generic_args: ty::GenericArgsRef<'tcx>,
    block: BasicBlock,
    destination: Local,
    arguments: Vec<Operand<'tcx>>,
    has_cleanup: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Origin {
    Data { source: bool, fixed: bool },
    Serializer,
    OperationResult(Local),
    BranchValue(Local),
    BranchContinue(Local),
    BranchResidual(Local),
    BranchTag(Local),
    Predicate,
    FlatMap(Local),
    Result,
}

/// Build an instance of the exact `Serialize::serialize` method for `source`.
/// Parent impl parameters are fixed to the source type; method parameters such
/// as the serializer remain at identity for the caller to specialize.
pub(crate) fn serialize_instance<'tcx>(
    tcx: TyCtxt<'tcx>,
    source: Ty<'tcx>,
    method: DefId,
) -> Option<Instance<'tcx>> {
    let source = source.peel_refs();
    let ty::Adt(owner, source_args) = source.kind() else {
        return None;
    };
    let implementation = tcx.parent(method);
    if !matches!(
        tcx.def_kind(implementation),
        rustc_hir::def::DefKind::Impl { of_trait: true }
    ) {
        return None;
    }
    let implementation_trait = tcx.impl_opt_trait_ref(implementation)?;

    let implementation_type = tcx.type_of(implementation);
    let implementation_identity = implementation_type.instantiate_identity().skip_norm_wip();
    let ty::Adt(implementation_owner, _) = implementation_identity.kind() else {
        return None;
    };
    if implementation_owner.did() != owner.did()
        || tcx.generics_of(implementation).count() != source_args.len()
    {
        return None;
    }
    let concrete_trait = implementation_trait
        .instantiate(tcx, source_args)
        .skip_norm_wip();
    if !serialize_trait(tcx, concrete_trait.def_id) || concrete_trait.self_ty() != source {
        return None;
    }

    let method_generics = tcx.generics_of(method);
    let parent_count = method_generics.parent_count;
    if parent_count != source_args.len() || parent_count > method_generics.count() {
        return None;
    }
    let identity = ty::GenericArgs::identity_for_item(tcx, method);
    let method_args =
        tcx.mk_args_from_iter(source_args.iter().chain(identity.iter().skip(parent_count)));
    if method_args.len() != method_generics.count() || !tcx.is_mir_available(method) {
        return None;
    }
    let instance = Instance {
        def: ty::InstanceKind::Item(method),
        args: method_args,
    };
    let body = tcx.instance_mir(instance.def);
    if body.arg_count != 2 {
        return None;
    }
    let receiver = instantiate_ty(tcx, instance, body.local_decls[Local::from_usize(1)].ty);
    (receiver.peel_refs() == source).then_some(instance)
}

/// Instantiate a MIR body without normalizing its symbolic projections.
/// `Instance::instantiate_mir` has a `Copy` bound that excludes `Body`, so
/// apply the same `InstanceKind` argument rule directly to a cloned body.
pub(crate) fn instantiated_body<'tcx>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
) -> mir::Body<'tcx> {
    let body = ty::EarlyBinder::bind(tcx, (*tcx.instance_mir(instance.def)).clone());
    if instance.def.has_polymorphic_mir_body() {
        body.instantiate(tcx, instance.args).skip_norm_wip()
    } else {
        body.instantiate_identity().skip_norm_wip()
    }
}

/// Prove one concrete `Serialize::serialize` body and return the types of the
/// children that its actual Serde protocol calls expose.
///
/// This proof uses MIR for local and cross-crate derives alike. It does not
/// trust `automatically_derived`, the owner fields, or a serializer callback
/// name. Every child comes from a concrete protocol operand whose value traces
/// to an immutable source projection or a fixed constant.
pub(crate) fn derived_children<'tcx>(
    tcx: TyCtxt<'tcx>,
    source: Ty<'tcx>,
    method: DefId,
    mode: SerializerMode,
) -> Option<Vec<SerializedChild<'tcx>>> {
    let source = source.peel_refs();
    let instance = serialize_instance(tcx, source, method)?;
    let body = instantiated_body(tcx, instance);
    if conversion::cyclic(&body) {
        return None;
    }
    let serializer = instantiate_ty(tcx, instance, body.local_decls[Local::from_usize(2)].ty);

    let reachable = reachable_blocks(&body);
    if reachable.is_empty() {
        return None;
    }
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
                let function = instantiate_ty(tcx, instance, func.ty(&body, tcx));
                let ty::FnDef(definition, generic_args) = function.kind() else {
                    return None;
                };
                let generic_args = (*generic_args).no_bound_vars()?;
                let kind = classify_call(tcx, *definition).or_else(|| {
                    crate::structural_fixed::predicate_call_is_bounded(
                        tcx,
                        *definition,
                        generic_args,
                    )
                    .then_some(Kind::Predicate)
                })?;
                calls.push(Call {
                    kind,
                    definition: *definition,
                    generic_args,
                    block: *block,
                    destination: destination.local,
                    arguments: args.iter().map(|argument| argument.node.clone()).collect(),
                    has_cleanup: matches!(unwind, UnwindAction::Cleanup(_)),
                });
                if let UnwindAction::Cleanup(cleanup) = unwind {
                    cleanup_entries.push(*cleanup);
                }
            }
            TerminatorKind::SwitchInt { .. } => (),
            TerminatorKind::Assert {
                cond,
                expected,
                msg,
                unwind,
                ..
            } => {
                if !checked_add_assertion_is_bounded(
                    tcx, &body, *block, cond, *expected, msg, &reachable,
                ) {
                    return None;
                }
                if let UnwindAction::Cleanup(cleanup) = unwind {
                    cleanup_entries.push(*cleanup);
                }
            }
            TerminatorKind::Goto { .. } | TerminatorKind::Return | TerminatorKind::Unreachable => {
                ()
            }
            TerminatorKind::Drop { unwind, .. } => {
                if let UnwindAction::Cleanup(cleanup) = unwind {
                    cleanup_entries.push(*cleanup);
                }
            }
            _ => return None,
        }
    }

    // A drop can appear before its producing call in the unordered reachable
    // block set. Validate drops only after collecting the complete call graph.
    for block in &reachable {
        if let TerminatorKind::Drop { place, .. } = &body.basic_blocks[*block].terminator().kind {
            if !drop_is_protocol_temporary(tcx, instance, &body, serializer, place, &calls) {
                return None;
            }
        }
    }

    if calls.iter().any(|call| {
        !call_shape_is_valid(
            tcx,
            instance,
            &body,
            serializer,
            mode,
            call.destination,
            call.kind,
            call.definition,
            call.generic_args,
            &call.arguments,
            &reachable,
            &calls,
        )
    }) || !switches_are_bounded(tcx, instance, &body, &reachable, &calls)
        || !statements_are_bounded(tcx, instance, &body, &reachable, &calls)
        || !control_flow_is_bounded(tcx, instance, &body, serializer, &reachable, &calls)
        || !cleanup_is_typed(
            tcx,
            instance,
            &body,
            serializer,
            &reachable,
            &cleanup_entries,
            &calls,
        )
        || !all_returns_are_results(tcx, instance, &body, &reachable, &calls)
    {
        return None;
    }

    children_from_calls(tcx, instance, &body, &reachable, &calls, mode)
}

fn instantiate_ty<'tcx>(tcx: TyCtxt<'tcx>, instance: Instance<'tcx>, value: Ty<'tcx>) -> Ty<'tcx> {
    instance.instantiate_mir(tcx, ty::EarlyBinder::bind(tcx, value))
}

fn classify_call(tcx: TyCtxt<'_>, definition: DefId) -> Option<Kind> {
    if tagged_newtype_helper(tcx, definition) {
        return Some(Kind::TaggedNewtype);
    }
    if method_of(
        tcx,
        definition,
        &["serde", "serde_core"],
        &["ser", "Serialize"],
        "serialize",
    ) {
        return Some(Kind::Serialize);
    }
    if method_of(
        tcx,
        definition,
        &["serde", "serde_core"],
        &["ser", "Serializer"],
        "serialize_struct",
    ) {
        return Some(Kind::Struct);
    }
    if method_of(
        tcx,
        definition,
        &["serde", "serde_core"],
        &["ser", "Serializer"],
        "serialize_struct_variant",
    ) {
        return Some(Kind::StructVariant);
    }
    if method_of(
        tcx,
        definition,
        &["serde", "serde_core"],
        &["ser", "Serializer"],
        "serialize_map",
    ) {
        return Some(Kind::Map);
    }
    if method_of(
        tcx,
        definition,
        &["serde", "serde_core"],
        &["ser", "Serializer"],
        "serialize_seq",
    ) {
        return Some(Kind::Seq);
    }
    if method_of(
        tcx,
        definition,
        &["serde", "serde_core"],
        &["ser", "Serializer"],
        "serialize_tuple",
    ) {
        return Some(Kind::Tuple);
    }
    if method_of(
        tcx,
        definition,
        &["serde", "serde_core"],
        &["ser", "Serializer"],
        "serialize_tuple_struct",
    ) {
        return Some(Kind::TupleStruct);
    }
    if method_of(
        tcx,
        definition,
        &["serde", "serde_core"],
        &["ser", "Serializer"],
        "serialize_tuple_variant",
    ) {
        return Some(Kind::TupleVariant);
    }
    if method_of(
        tcx,
        definition,
        &["serde", "serde_core"],
        &["ser", "Serializer"],
        "serialize_some",
    ) {
        return Some(Kind::Some);
    }
    if method_of(
        tcx,
        definition,
        &["serde", "serde_core"],
        &["ser", "Serializer"],
        "serialize_newtype_struct",
    ) {
        return Some(Kind::NewtypeStruct);
    }
    if method_of(
        tcx,
        definition,
        &["serde", "serde_core"],
        &["ser", "Serializer"],
        "serialize_newtype_variant",
    ) {
        return Some(Kind::NewtypeVariant);
    }
    if method_of(
        tcx,
        definition,
        &["serde", "serde_core"],
        &["ser", "Serializer"],
        "serialize_unit",
    ) {
        return Some(Kind::Unit);
    }
    if method_of(
        tcx,
        definition,
        &["serde", "serde_core"],
        &["ser", "Serializer"],
        "serialize_unit_struct",
    ) {
        return Some(Kind::UnitStruct);
    }
    if method_of(
        tcx,
        definition,
        &["serde", "serde_core"],
        &["ser", "Serializer"],
        "serialize_unit_variant",
    ) {
        return Some(Kind::UnitVariant);
    }
    for (trait_path, kind, methods) in [
        (
            &["ser", "SerializeStruct"][..],
            Kind::StructField,
            &["serialize_field"][..],
        ),
        (
            &["ser", "SerializeStructVariant"][..],
            Kind::StructVariantField,
            &["serialize_field"][..],
        ),
        (
            &["ser", "SerializeStruct"][..],
            Kind::SkipField,
            &["skip_field"][..],
        ),
        (
            &["ser", "SerializeStructVariant"][..],
            Kind::SkipField,
            &["skip_field"][..],
        ),
        (
            &["ser", "SerializeMap"][..],
            Kind::MapEntry,
            &["serialize_entry"][..],
        ),
        (
            &["ser", "SerializeMap"][..],
            Kind::MapKey,
            &["serialize_key"][..],
        ),
        (
            &["ser", "SerializeMap"][..],
            Kind::MapValue,
            &["serialize_value"][..],
        ),
        (
            &["ser", "SerializeSeq"][..],
            Kind::Element,
            &["serialize_element"][..],
        ),
        (
            &["ser", "SerializeTuple"][..],
            Kind::Element,
            &["serialize_element"][..],
        ),
        (
            &["ser", "SerializeTupleStruct"][..],
            Kind::TupleStructField,
            &["serialize_field"][..],
        ),
        (
            &["ser", "SerializeTupleVariant"][..],
            Kind::TupleVariantField,
            &["serialize_field"][..],
        ),
    ] {
        if methods.iter().any(|method| {
            method_of(
                tcx,
                definition,
                &["serde", "serde_core"],
                trait_path,
                method,
            )
        }) {
            return Some(kind);
        }
    }
    for trait_name in [
        "SerializeStruct",
        "SerializeStructVariant",
        "SerializeMap",
        "SerializeSeq",
        "SerializeTuple",
        "SerializeTupleStruct",
        "SerializeTupleVariant",
    ] {
        if method_of(
            tcx,
            definition,
            &["serde", "serde_core"],
            &["ser", trait_name],
            "end",
        ) {
            return Some(Kind::End);
        }
    }
    if method_of(
        tcx,
        definition,
        &["core"],
        &["ops", "try_trait", "Try"],
        "branch",
    ) {
        return Some(Kind::TryBranch);
    }
    if method_of(
        tcx,
        definition,
        &["core"],
        &["ops", "try_trait", "FromResidual"],
        "from_residual",
    ) {
        return Some(Kind::FromResidual);
    }
    None
}

fn tagged_newtype_helper(tcx: TyCtxt<'_>, definition: DefId) -> bool {
    tcx.opt_item_name(definition)
        .is_some_and(|name| name.as_str() == "serialize_tagged_newtype")
        && types::physical_item_path(
            tcx,
            definition,
            "serde",
            &["private", "ser", "serialize_tagged_newtype"],
        )
}

fn method_of(
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

fn serialize_trait(tcx: TyCtxt<'_>, definition: DefId) -> bool {
    ["serde", "serde_core"].iter().any(|crate_name| {
        types::physical_item_path(tcx, definition, crate_name, &["ser", "Serialize"])
    })
}

fn call_shape_is_valid<'tcx>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
    body: &mir::Body<'tcx>,
    serializer: Ty<'tcx>,
    mode: SerializerMode,
    destination: Local,
    kind: Kind,
    definition: DefId,
    generic_args: ty::GenericArgsRef<'tcx>,
    arguments: &[Operand<'tcx>],
    reachable: &HashSet<BasicBlock>,
    previous: &[Call<'tcx>],
) -> bool {
    let origins: Option<Vec<_>> = arguments
        .iter()
        .map(|argument| {
            operand_origin(
                tcx,
                instance,
                body,
                argument,
                reachable,
                previous,
                &mut HashSet::new(),
            )
        })
        .collect();
    let Some(origins) = origins else {
        return false;
    };
    let call_result = instantiate_ty(tcx, instance, body.local_decls[destination].ty);
    if kind == Kind::TryBranch {
        if arguments.len() != 1 {
            return false;
        }
        let input = instantiate_ty(tcx, instance, arguments[0].ty(body, tcx));
        return matches!(origins[0], Origin::OperationResult(_))
            && try_branch_shape(tcx, input, call_result, serializer);
    }
    if kind == Kind::FromResidual {
        if arguments.len() != 1 {
            return false;
        }
        return arguments.len() == 1
            && matches!(origins[0], Origin::BranchResidual(_))
            && is_same_result(tcx, call_result, method_output(tcx, instance, body))
            && is_result_residual(
                tcx,
                instantiate_ty(tcx, instance, arguments[0].ty(body, tcx)),
                serializer,
            );
    }
    if kind == Kind::Predicate {
        return matches!(call_result.kind(), ty::Bool)
            && !origins.is_empty()
            && origins
                .iter()
                .all(|origin| *origin == Origin::Predicate || is_bounded_data(*origin));
    }
    if kind == Kind::TaggedNewtype {
        return mode != SerializerMode::Flattened
            && tagged_helper_shape(
                tcx,
                instance,
                body,
                serializer,
                definition,
                generic_args,
                arguments,
                &origins,
                call_result,
            );
    }
    if kind.fallible() && !is_result_for_serializer(tcx, call_result, serializer) {
        return false;
    }

    if mode == SerializerMode::Flattened
        && !matches!(
            kind,
            Kind::Struct
                | Kind::StructField
                | Kind::SkipField
                | Kind::End
                | Kind::Serialize
                | Kind::NewtypeStruct
                | Kind::TryBranch
                | Kind::FromResidual
                | Kind::Predicate
        )
    {
        return false;
    }
    if mode == SerializerMode::Tagged && matches!(kind, Kind::StructVariant | Kind::TupleVariant) {
        return false;
    }

    match kind {
        Kind::Struct
        | Kind::StructVariant
        | Kind::Map
        | Kind::Seq
        | Kind::Tuple
        | Kind::TupleStruct
        | Kind::TupleVariant => {
            if origins.first() != Some(&Origin::Serializer) {
                return false;
            }
            if mode == SerializerMode::Tagged
                && matches!(kind, Kind::Seq | Kind::Tuple | Kind::TupleStruct)
            {
                return false;
            }
            constructor_metadata_is_fixed(tcx, instance, body, kind, arguments, &origins, reachable)
        }
        Kind::StructField | Kind::StructVariantField => {
            arguments.len() == 3
                && matches!(origins[0], Origin::BranchContinue(_))
                && matches!(
                    origins[1],
                    Origin::Data {
                        source: false,
                        fixed: true
                    }
                )
                && is_bounded_data(origins[2])
        }
        Kind::TupleStructField | Kind::TupleVariantField => {
            arguments.len() == 2
                && matches!(origins[0], Origin::BranchContinue(_))
                && is_bounded_data(origins[1])
        }
        Kind::SkipField => {
            arguments.len() == 2
                && matches!(origins[0], Origin::BranchContinue(_))
                && matches!(
                    origins[1],
                    Origin::Data {
                        source: false,
                        fixed: true
                    }
                )
        }
        Kind::Element => {
            arguments.len() == 2
                && matches!(origins[0], Origin::BranchContinue(_))
                && is_bounded_data(origins[1])
        }
        Kind::MapEntry => {
            arguments.len() == 3
                && matches!(origins[0], Origin::BranchContinue(_))
                && matches!(
                    origins[1],
                    Origin::Data {
                        source: false,
                        fixed: true
                    }
                )
                && is_bounded_data(origins[2])
        }
        Kind::MapKey | Kind::MapValue => {
            arguments.len() == 2
                && matches!(origins[0], Origin::BranchContinue(_))
                && is_bounded_data(origins[1])
        }
        Kind::End => arguments.len() == 1 && matches!(origins[0], Origin::BranchContinue(_)),
        Kind::Serialize => {
            if arguments.len() != 2 || !is_bounded_data(origins[0]) {
                return false;
            }
            origins[1] == Origin::Serializer
                || mode != SerializerMode::Tagged
                    && matches!(origins[1], Origin::FlatMap(_))
                    && flat_map_type(
                        tcx,
                        instantiate_ty(tcx, instance, arguments[1].ty(body, tcx)),
                    )
        }
        Kind::Some => {
            arguments.len() == 2 && origins[0] == Origin::Serializer && is_bounded_data(origins[1])
        }
        Kind::NewtypeStruct => {
            arguments.len() == 3
                && origins[0] == Origin::Serializer
                && matches!(
                    origins[1],
                    Origin::Data {
                        source: false,
                        fixed: true
                    }
                )
                && is_bounded_data(origins[2])
        }
        Kind::NewtypeVariant => {
            arguments.len() == 5
                && origins[0] == Origin::Serializer
                && origins[1..4].iter().all(|origin| {
                    matches!(
                        origin,
                        Origin::Data {
                            source: false,
                            fixed: true
                        }
                    )
                })
                && is_bounded_data(origins[4])
        }
        Kind::Unit => arguments.len() == 1 && origins[0] == Origin::Serializer,
        Kind::UnitStruct => {
            arguments.len() == 2
                && origins[0] == Origin::Serializer
                && matches!(
                    origins[1],
                    Origin::Data {
                        source: false,
                        fixed: true
                    }
                )
        }
        Kind::UnitVariant => {
            arguments.len() == 4
                && origins[0] == Origin::Serializer
                && origins[1..4].iter().all(|origin| {
                    matches!(
                        origin,
                        Origin::Data {
                            source: false,
                            fixed: true
                        }
                    )
                })
        }
        Kind::TryBranch | Kind::FromResidual | Kind::Predicate | Kind::TaggedNewtype => false,
    }
}

fn tagged_helper_shape<'tcx>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
    body: &mir::Body<'tcx>,
    serializer: Ty<'tcx>,
    definition: DefId,
    generic_args: ty::GenericArgsRef<'tcx>,
    arguments: &[Operand<'tcx>],
    origins: &[Origin],
    result: Ty<'tcx>,
) -> bool {
    if arguments.len() != 6
        || origins.len() != 6
        || !is_result_for_serializer(tcx, result, serializer)
    {
        return false;
    }
    let type_arguments: Vec<_> = generic_args.types().collect();
    if type_arguments.len() != 2
        || type_arguments[0] != serializer
        || type_arguments[1]
            != instantiate_ty(tcx, instance, arguments[5].ty(body, tcx)).peel_refs()
        || origins[0] != Origin::Serializer
        || !origins[1..5].iter().all(|origin| {
            matches!(
                origin,
                Origin::Data {
                    source: false,
                    fixed: true
                }
            )
        })
        || !is_bounded_data(origins[5])
    {
        return false;
    }
    let signature = tcx
        .fn_sig(definition)
        .instantiate(tcx, generic_args)
        .skip_binder();
    if signature.inputs().len() != 6 || signature.output() != result {
        return false;
    }
    let static_str = |value: Ty<'tcx>| {
        matches!(value.kind(), ty::Ref(region, inner, ty::Mutability::Not)
            if matches!(region.kind(), ty::RegionKind::ReStatic)
                && matches!(inner.kind(), ty::Str))
    };
    let ty::Ref(_, value, ty::Mutability::Not) = signature.inputs()[5].kind() else {
        return false;
    };
    type_arguments[1] == *value
        && instantiate_ty(tcx, instance, arguments[0].ty(body, tcx)) == serializer
        && signature.inputs()[1..5]
            .iter()
            .enumerate()
            .all(|(offset, input)| {
                static_str(*input)
                    && instantiate_ty(tcx, instance, arguments[offset + 1].ty(body, tcx)) == *input
            })
}

fn checked_add_assertion_is_bounded<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    assertion_block: BasicBlock,
    condition: &Operand<'tcx>,
    expected: bool,
    message: &Box<mir::AssertMessage<'tcx>>,
    reachable: &HashSet<BasicBlock>,
) -> bool {
    if expected {
        return false;
    }
    let Some(local) = overflow_flag_local(condition) else {
        return false;
    };
    let Some((addition_block, left, right)) = checked_add_parts(body, local, reachable) else {
        return false;
    };
    if assertion_block != addition_block
        || !matches!(&**message, mir::AssertKind::Overflow(BinOp::Add, assert_left, assert_right)
            if same_count_operand(tcx, left, assert_left)
                && same_count_operand(tcx, right, assert_right))
    {
        return false;
    }
    checked_add_bound(tcx, body, local, reachable, &mut HashSet::new()).is_some()
}

fn checked_add_parts<'a, 'tcx>(
    body: &'a mir::Body<'tcx>,
    local: Local,
    reachable: &HashSet<BasicBlock>,
) -> Option<(BasicBlock, &'a Operand<'tcx>, &'a Operand<'tcx>)> {
    let mut found = None;
    for block in reachable {
        for statement in &body.basic_blocks[*block].statements {
            let StatementKind::Assign(assignment) = &statement.kind else {
                continue;
            };
            let (target, value) = &**assignment;
            if target.local != local {
                continue;
            }
            if !target.projection.is_empty() || found.is_some() {
                return None;
            }
            let Rvalue::BinaryOp(BinOp::AddWithOverflow, operands) = value else {
                return None;
            };
            let (left, right) = &**operands;
            found = Some((*block, left, right));
        }
    }
    found
}

fn checked_add_bound<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    local: Local,
    reachable: &HashSet<BasicBlock>,
    active: &mut HashSet<Local>,
) -> Option<u128> {
    let result_ty = body.local_decls[local].ty;
    let ty::Tuple(fields) = result_ty.kind() else {
        return None;
    };
    if fields.len() != 2
        || !matches!(fields[0].kind(), ty::Uint(ty::UintTy::Usize))
        || !matches!(fields[1].kind(), ty::Bool)
    {
        return None;
    }
    let (addition_block, left, right) = checked_add_parts(body, local, reachable)?;
    let assertion_matches = reachable.iter().any(|block| {
        if *block != addition_block {
            return false;
        }
        let TerminatorKind::Assert {
            cond,
            expected: false,
            msg,
            ..
        } = &body.basic_blocks[*block].terminator().kind
        else {
            return false;
        };
        overflow_flag_local(cond) == Some(local)
            && matches!(&**msg, mir::AssertKind::Overflow(BinOp::Add, assert_left, assert_right)
                if same_count_operand(tcx, left, assert_left)
                    && same_count_operand(tcx, right, assert_right))
    });
    if !assertion_matches {
        return None;
    }
    let left_bound = finite_count_operand(tcx, body, left, reachable, active)?;
    let right_bound = finite_count_operand(tcx, body, right, reachable, active)?;
    let result_bound = left_bound.checked_add(right_bound)?;
    (result_bound <= usize::MAX as u128).then_some(result_bound)
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
            left.const_
                .try_eval_target_usize(tcx, ty::TypingEnv::fully_monomorphized())
                == right
                    .const_
                    .try_eval_target_usize(tcx, ty::TypingEnv::fully_monomorphized())
        }
        _ => false,
    }
}

fn switches_are_bounded<'tcx>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
    body: &mir::Body<'tcx>,
    reachable: &HashSet<BasicBlock>,
    calls: &[Call<'tcx>],
) -> bool {
    reachable.iter().all(|block| {
        let TerminatorKind::SwitchInt { discr, .. } = &body.basic_blocks[*block].terminator().kind
        else {
            return true;
        };
        matches!(
            operand_origin(
                tcx,
                instance,
                body,
                discr,
                reachable,
                calls,
                &mut HashSet::new(),
            ),
            Some(Origin::Data { .. } | Origin::Predicate | Origin::BranchTag(_))
        )
    })
}

fn constructor_metadata_is_fixed<'tcx>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
    body: &mir::Body<'tcx>,
    kind: Kind,
    arguments: &[Operand<'tcx>],
    origins: &[Origin],
    reachable: &HashSet<BasicBlock>,
) -> bool {
    let (expected_len, count_index) = match kind {
        Kind::Struct => (3, 2),
        Kind::StructVariant => (5, 4),
        Kind::Map | Kind::Seq | Kind::Tuple => (2, 1),
        Kind::TupleStruct => (3, 2),
        Kind::TupleVariant => (5, 4),
        _ => return false,
    };
    if arguments.len() != expected_len {
        return false;
    }
    for index in 1..arguments.len() {
        if index == count_index {
            continue;
        }
        if !matches!(
            origins[index],
            Origin::Data {
                source: false,
                fixed: true
            }
        ) {
            return false;
        }
    }
    let count_ty = instantiate_ty(tcx, instance, arguments[count_index].ty(body, tcx));
    if kind == Kind::Map {
        let ty::Adt(owner, _) = count_ty.kind() else {
            return false;
        };
        return types::physical_item_path(tcx, owner.did(), "core", &["option", "Option"])
            && matches!(
                origins[count_index],
                Origin::Data {
                    source: false,
                    fixed: true
                }
            );
    }
    matches!(count_ty.kind(), ty::Uint(ty::UintTy::Usize))
        && finite_count_operand(
            tcx,
            body,
            &arguments[count_index],
            reachable,
            &mut HashSet::new(),
        )
        .is_some()
}

fn children_from_calls<'tcx>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
    body: &mir::Body<'tcx>,
    reachable: &HashSet<BasicBlock>,
    calls: &[Call<'tcx>],
    mode: SerializerMode,
) -> Option<Vec<SerializedChild<'tcx>>> {
    let branches = branch_operations(tcx, instance, body, reachable, calls)?;
    let mut children = Vec::new();
    let mut constructors = HashMap::<Local, Local>::new();
    for call in calls.iter().filter(|call| call.kind.creates_state()) {
        let branch = *branches.get(&call.destination)?;
        constructors.insert(branch, call.destination);
    }

    for call in calls {
        match call.kind {
            Kind::Struct
            | Kind::StructVariant
            | Kind::Map
            | Kind::Seq
            | Kind::Tuple
            | Kind::TupleStruct
            | Kind::TupleVariant => {
                if call.kind == Kind::Map && map_length_is_unknown(tcx, body, call, reachable)? {
                    let branch = *branches.get(&call.destination)?;
                    let flattened_for_this_state = calls.iter().any(|candidate| {
                        candidate.kind == Kind::Serialize
                            && candidate.arguments.get(1).is_some_and(|argument| {
                                operand_origin(
                                    tcx,
                                    instance,
                                    body,
                                    argument,
                                    reachable,
                                    calls,
                                    &mut HashSet::new(),
                                ) == Some(Origin::FlatMap(branch))
                            })
                    });
                    if !flattened_for_this_state {
                        return None;
                    }
                }
            }
            Kind::StructField
            | Kind::StructVariantField
            | Kind::TupleStructField
            | Kind::TupleVariantField
            | Kind::Element
            | Kind::MapEntry
            | Kind::MapKey
            | Kind::MapValue => {
                let parent_branch = match operand_origin(
                    tcx,
                    instance,
                    body,
                    &call.arguments[0],
                    reachable,
                    calls,
                    &mut HashSet::new(),
                )? {
                    Origin::BranchContinue(branch) => branch,
                    _ => return None,
                };
                if !constructors.contains_key(&parent_branch) {
                    return None;
                }
                let index = match call.kind {
                    Kind::StructField | Kind::StructVariantField => 2,
                    Kind::TupleStructField | Kind::TupleVariantField => 1,
                    Kind::Element | Kind::MapKey | Kind::MapValue => 1,
                    Kind::MapEntry => 2,
                    _ => return None,
                };
                let argument = call.arguments.get(index)?;
                let ty = instantiate_ty(tcx, instance, argument.ty(body, tcx));
                if ty.has_non_region_param() {
                    return None;
                }
                children.push(SerializedChild {
                    ty,
                    charged_parent: mode != SerializerMode::Flattened,
                    mode: SerializerMode::Plain,
                });
                if call.kind == Kind::MapEntry {
                    let key = call.arguments.get(1)?;
                    let key_ty = instantiate_ty(tcx, instance, key.ty(body, tcx));
                    if key_ty.has_non_region_param() {
                        return None;
                    }
                    children.push(SerializedChild {
                        ty: key_ty,
                        charged_parent: mode != SerializerMode::Flattened,
                        mode: SerializerMode::Plain,
                    });
                }
            }
            Kind::Serialize => {
                let value = call.arguments.first()?;
                let serializer_arg = call.arguments.get(1)?;
                let value_origin = operand_origin(
                    tcx,
                    instance,
                    body,
                    value,
                    reachable,
                    calls,
                    &mut HashSet::new(),
                )?;
                if !is_bounded_data(value_origin) {
                    return None;
                }
                let serializer_ty = instantiate_ty(tcx, instance, serializer_arg.ty(body, tcx));
                let serializer_origin = operand_origin(
                    tcx,
                    instance,
                    body,
                    serializer_arg,
                    reachable,
                    calls,
                    &mut HashSet::new(),
                )?;
                let (charged_parent, child_mode) = match serializer_origin {
                    Origin::Serializer => (false, mode),
                    Origin::FlatMap(_)
                        if mode != SerializerMode::Tagged && flat_map_type(tcx, serializer_ty) =>
                    {
                        (false, SerializerMode::Flattened)
                    }
                    _ => return None,
                };
                let ty = instantiate_ty(tcx, instance, value.ty(body, tcx));
                if ty.has_non_region_param() {
                    return None;
                }
                children.push(SerializedChild {
                    ty,
                    charged_parent,
                    mode: child_mode,
                });
            }
            Kind::Some => {
                // TaggedSerializer::serialize_some returns a fixed error and
                // does not call the payload's Serialize implementation.
                if mode == SerializerMode::Tagged {
                    continue;
                }
                let value = call.arguments.get(1)?;
                let ty = instantiate_ty(tcx, instance, value.ty(body, tcx));
                if ty.has_non_region_param() {
                    return None;
                }
                children.push(SerializedChild {
                    ty,
                    charged_parent: true,
                    mode: SerializerMode::Plain,
                });
            }
            Kind::NewtypeStruct => {
                let value = call.arguments.get(2)?;
                let ty = instantiate_ty(tcx, instance, value.ty(body, tcx));
                if ty.has_non_region_param() {
                    return None;
                }
                children.push(SerializedChild {
                    ty,
                    charged_parent: mode == SerializerMode::Plain,
                    mode,
                });
            }
            Kind::NewtypeVariant => {
                let value = call.arguments.get(4)?;
                let ty = instantiate_ty(tcx, instance, value.ty(body, tcx));
                if ty.has_non_region_param() {
                    return None;
                }
                children.push(SerializedChild {
                    ty,
                    charged_parent: true,
                    mode: SerializerMode::Plain,
                });
            }
            Kind::TaggedNewtype => {
                let value = call.arguments.get(5)?;
                let ty = instantiate_ty(tcx, instance, value.ty(body, tcx)).peel_refs();
                if ty.has_non_region_param() {
                    return None;
                }
                children.push(SerializedChild {
                    ty,
                    charged_parent: false,
                    mode: SerializerMode::Tagged,
                });
            }
            _ => (),
        }
    }
    Some(children)
}

fn map_length_is_unknown<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    call: &Call<'tcx>,
    reachable: &HashSet<BasicBlock>,
) -> Option<bool> {
    let count = call.arguments.get(1)?;
    let ty::Adt(option, _) = count.ty(body, tcx).kind() else {
        return None;
    };
    if !types::physical_item_path(tcx, option.did(), "core", &["option", "Option"]) {
        return None;
    }
    let (Operand::Copy(place) | Operand::Move(place)) = count else {
        return None;
    };
    if !place.projection.is_empty() {
        return None;
    }
    let mut saw_assignment = false;
    let mut may_be_unknown = false;
    for block in reachable {
        for statement in &body.basic_blocks[*block].statements {
            let StatementKind::Assign(assignment) = &statement.kind else {
                continue;
            };
            let (target, value) = &**assignment;
            if target.local != place.local || !target.projection.is_empty() {
                continue;
            }
            let Rvalue::Aggregate(kind, operands) = value else {
                return None;
            };
            let mir::AggregateKind::Adt(definition, variant, _, _, _) = &**kind else {
                return None;
            };
            if !types::physical_item_path(tcx, *definition, "core", &["option", "Option"]) {
                return None;
            }
            saw_assignment = true;
            match tcx.adt_def(*definition).variant(*variant).name.as_str() {
                "None" if operands.is_empty() => may_be_unknown = true,
                "Some"
                    if operands.len() == 1
                        && operands.iter().next().is_some_and(|operand| {
                            finite_count_operand(tcx, body, operand, reachable, &mut HashSet::new())
                                .is_some()
                        }) =>
                {
                    ()
                }
                _ => return None,
            }
        }
    }
    saw_assignment.then_some(may_be_unknown)
}

fn branch_operations<'tcx>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
    body: &mir::Body<'tcx>,
    reachable: &HashSet<BasicBlock>,
    calls: &[Call<'tcx>],
) -> Option<HashMap<Local, Local>> {
    let mut operation_branches = HashMap::new();
    let branches: Vec<_> = calls
        .iter()
        .filter(|call| call.kind == Kind::TryBranch)
        .collect();
    let residuals: Vec<_> = calls
        .iter()
        .filter(|call| call.kind == Kind::FromResidual)
        .collect();
    let mut branch_switches = HashMap::<Local, (BasicBlock, BasicBlock, BasicBlock)>::new();
    for block in reachable {
        let TerminatorKind::SwitchInt { discr, targets } =
            &body.basic_blocks[*block].terminator().kind
        else {
            continue;
        };
        let Some(Origin::BranchTag(branch)) = operand_origin(
            tcx,
            instance,
            body,
            discr,
            reachable,
            calls,
            &mut HashSet::new(),
        ) else {
            continue;
        };
        let (break_target, continue_target) =
            control_flow_targets(tcx, instance, body, branch, targets)?;
        if branch_switches
            .insert(branch, (*block, break_target, continue_target))
            .is_some()
        {
            return None;
        }
    }
    let mut residual_for_branch = HashMap::new();
    for residual in residuals {
        if residual.arguments.len() != 1 {
            return None;
        }
        let Origin::BranchResidual(branch) = operand_origin(
            tcx,
            instance,
            body,
            &residual.arguments[0],
            reachable,
            calls,
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
    let mut parent_for_branch = HashMap::new();
    for branch_call in branches {
        if branch_call.arguments.len() != 1 {
            return None;
        }
        let Origin::OperationResult(operation) = operand_origin(
            tcx,
            instance,
            body,
            &branch_call.arguments[0],
            reachable,
            calls,
            &mut HashSet::new(),
        )?
        else {
            return None;
        };
        let producer = calls.iter().find(|call| call.destination == operation)?;
        if !producer.kind.fallible()
            || operation_branches
                .insert(operation, branch_call.destination)
                .is_some()
        {
            return None;
        }
        parent_for_branch.insert(branch_call.destination, producer);
        let (switch, break_target, continue_target) =
            *branch_switches.get(&branch_call.destination)?;
        let residual = *residual_for_branch.get(&branch_call.destination)?;
        if !can_reach(body, branch_call.block, switch, reachable)
            || !dominates(body, reachable, branch_call.block, switch)
            || !can_reach(body, break_target, residual, reachable)
            || can_reach(body, continue_target, residual, reachable)
        {
            return None;
        }
    }

    let state_calls: Vec<_> = calls
        .iter()
        .filter(|call| call.kind.takes_state())
        .collect();
    for state_call in &state_calls {
        let Some(Origin::BranchContinue(branch)) =
            state_call.arguments.first().and_then(|argument| {
                operand_origin(
                    tcx,
                    instance,
                    body,
                    argument,
                    reachable,
                    calls,
                    &mut HashSet::new(),
                )
            })
        else {
            return None;
        };
        let state_constructor = parent_for_branch.get(&branch)?;
        if !state_constructor.kind.creates_state() {
            return None;
        }
        if !state_kind_matches(state_constructor.kind, state_call.kind)
            || !state_trait_matches(tcx, state_constructor.kind, state_call.definition)
            || branch_output_type(tcx, instance, body, branch)
                != Some(
                    instantiate_ty(tcx, instance, state_call.arguments[0].ty(body, tcx))
                        .peel_refs(),
                )
        {
            return None;
        }
        let (_, break_target, continue_target) = *branch_switches.get(&branch)?;
        if !can_reach(body, continue_target, state_call.block, reachable)
            || can_reach(body, break_target, state_call.block, reachable)
        {
            return None;
        }
    }
    for (branch, (_, break_target, continue_target)) in branch_switches {
        let producer = parent_for_branch.get(&branch)?;
        if producer.kind.creates_state() {
            let owns_end = end_calls_for_branch(tcx, instance, body, reachable, calls, branch)
                .is_some_and(|end| {
                    can_reach(body, continue_target, end, reachable)
                        && !can_reach(body, break_target, end, reachable)
                });
            if !owns_end {
                return None;
            }
        } else if producer.kind.takes_state() {
            for later in &state_calls {
                if later.block != producer.block
                    && can_reach(body, break_target, later.block, reachable)
                {
                    return None;
                }
            }
        }
    }
    Some(operation_branches)
}

fn end_calls_for_branch<'tcx>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
    body: &mir::Body<'tcx>,
    reachable: &HashSet<BasicBlock>,
    calls: &[Call<'tcx>],
    constructor_branch: Local,
) -> Option<BasicBlock> {
    calls
        .iter()
        .filter(|call| call.kind == Kind::End)
        .find(|call| {
            call.arguments.first().is_some_and(|argument| {
                operand_origin(
                    tcx,
                    instance,
                    body,
                    argument,
                    reachable,
                    calls,
                    &mut HashSet::new(),
                ) == Some(Origin::BranchContinue(constructor_branch))
            })
        })
        .map(|call| call.block)
}

fn control_flow_is_bounded<'tcx>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
    body: &mir::Body<'tcx>,
    serializer: Ty<'tcx>,
    reachable: &HashSet<BasicBlock>,
    calls: &[Call<'tcx>],
) -> bool {
    let mut branches = HashMap::new();
    for call in calls.iter().filter(|call| call.kind == Kind::TryBranch) {
        if call.arguments.len() != 1 {
            return false;
        }
        let Some(Origin::OperationResult(operation)) = operand_origin(
            tcx,
            instance,
            body,
            &call.arguments[0],
            reachable,
            calls,
            &mut HashSet::new(),
        ) else {
            return false;
        };
        let Some(producer) = calls
            .iter()
            .find(|candidate| candidate.destination == operation)
        else {
            return false;
        };
        if producer.has_cleanup
            && !cleanup_origin_is_typed(tcx, instance, body, serializer, producer)
        {
            return false;
        }
        if branches.insert(operation, call.destination).is_some() {
            return false;
        }
    }
    for call in calls {
        if call.kind == Kind::FromResidual
            && !result_call_returns_unchanged(tcx, instance, body, serializer, calls, call)
        {
            return false;
        }
        if call.destination == RETURN_PLACE
            && call.kind.fallible()
            && !result_call_returns_unchanged(tcx, instance, body, serializer, calls, call)
        {
            return false;
        }
        if call.kind.fallible() && call.kind != Kind::Predicate {
            if call.destination == RETURN_PLACE {
                continue;
            }
            if !branches.contains_key(&call.destination) {
                return false;
            }
        }
        if call.has_cleanup && !cleanup_origin_is_typed(tcx, instance, body, serializer, call) {
            return false;
        }
    }
    true
}

/// A Result written to the method return place must flow straight to Return.
/// In particular, this preserves a `FromResidual` refusal instead of letting
/// a later successful serializer call overwrite it. Only protocol-state drops
/// are allowed between the call and Return; those drops release already-built
/// output state and cannot inspect or consume the source.
fn result_call_returns_unchanged<'tcx>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
    body: &mir::Body<'tcx>,
    serializer: Ty<'tcx>,
    calls: &[Call<'tcx>],
    call: &Call<'tcx>,
) -> bool {
    if call.destination != RETURN_PLACE
        || !is_same_result(
            tcx,
            instantiate_ty(tcx, instance, body.local_decls[call.destination].ty),
            method_output(tcx, instance, body),
        )
    {
        return false;
    }
    let TerminatorKind::Call {
        destination,
        target: Some(first),
        ..
    } = &body.basic_blocks[call.block].terminator().kind
    else {
        return false;
    };
    if destination.local != RETURN_PLACE {
        return false;
    }

    let mut pending = vec![(*first, false)];
    let mut active = HashSet::new();
    let mut complete = HashSet::new();
    while let Some((block, leaving)) = pending.pop() {
        if leaving {
            active.remove(&block);
            complete.insert(block);
            continue;
        }
        if complete.contains(&block) {
            continue;
        }
        if !active.insert(block) {
            return false;
        }
        pending.push((block, true));
        let data = &body.basic_blocks[block];
        if data.statements.iter().any(|statement| {
            !(matches!(
                statement.kind,
                StatementKind::StorageLive(_)
                    | StatementKind::StorageDead(_)
                    | StatementKind::Nop
                    | StatementKind::FakeRead(_)
            ) || literal_bool_drop_flag_assignment(tcx, body, statement))
        }) {
            return false;
        }
        let mut successors = Vec::new();
        match &data.terminator().kind {
            TerminatorKind::Return => (),
            TerminatorKind::Goto { target } => successors.push(*target),
            TerminatorKind::Drop { place, target, .. }
                if drop_is_protocol_temporary(tcx, instance, body, serializer, place, calls) =>
            {
                successors.push(*target);
            }
            TerminatorKind::SwitchInt { discr, targets }
                if switch_is_literal_drop_flag(tcx, body, discr) =>
            {
                successors.extend(targets.all_targets().iter().copied());
            }
            _ => return false,
        }
        for successor in successors.into_iter().rev() {
            pending.push((successor, false));
        }
    }
    !complete.is_empty()
}

fn literal_bool_drop_flag_assignment<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    statement: &mir::Statement<'tcx>,
) -> bool {
    let StatementKind::Assign(assignment) = &statement.kind else {
        return false;
    };
    let (target, value) = &**assignment;
    target.local.as_usize() > body.arg_count
        && target.projection.is_empty()
        && matches!(body.local_decls[target.local].ty.kind(), ty::Bool)
        && matches!(value,
            Rvalue::Use(Operand::Constant(constant), _)
                if constant.const_.try_eval_bool(
                    tcx,
                    ty::TypingEnv::fully_monomorphized(),
                ).is_some())
}

fn fixed_bool_drop_flag<'tcx>(tcx: TyCtxt<'tcx>, body: &mir::Body<'tcx>, local: Local) -> bool {
    if local.as_usize() <= body.arg_count || !matches!(body.local_decls[local].ty.kind(), ty::Bool)
    {
        return false;
    }
    let mut assigned = false;
    for block in body.basic_blocks.iter() {
        if matches!(&block.terminator().kind,
            TerminatorKind::Call { destination, .. }
                if destination.local == local)
        {
            return false;
        }
        for statement in &block.statements {
            let StatementKind::Assign(assignment) = &statement.kind else {
                continue;
            };
            if assignment.0.local == local {
                if !literal_bool_drop_flag_assignment(tcx, body, statement) {
                    return false;
                }
                assigned = true;
            }
            if matches!(&**assignment,
                (_, Rvalue::Ref(_, BorrowKind::Mut { .. }, place))
                    if place.local == local)
            {
                return false;
            }
            if matches!(&statement.kind,
                StatementKind::SetDiscriminant { place, .. }
                    if place.local == local)
            {
                return false;
            }
        }
        if matches!(&block.terminator().kind,
        TerminatorKind::Call { args, .. }
            if args.iter().any(|argument| matches!(
                &argument.node,
                Operand::Copy(place) | Operand::Move(place) if place.local == local
            )))
        {
            return false;
        }
    }
    assigned
}

fn switch_is_literal_drop_flag<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    discriminant: &Operand<'tcx>,
) -> bool {
    let (Operand::Copy(place) | Operand::Move(place)) = discriminant else {
        return false;
    };
    place.projection.is_empty() && fixed_bool_drop_flag(tcx, body, place.local)
}

fn cleanup_origin_is_typed<'tcx>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
    body: &mir::Body<'tcx>,
    serializer: Ty<'tcx>,
    call: &Call<'tcx>,
) -> bool {
    let result = instantiate_ty(tcx, instance, body.local_decls[call.destination].ty);
    is_result_for_serializer(tcx, result, serializer)
        || is_control_flow_for_serializer(tcx, result, serializer)
        || matches!(call.kind, Kind::Predicate) && matches!(result.kind(), ty::Bool)
}

fn cleanup_is_typed<'tcx>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
    body: &mir::Body<'tcx>,
    serializer: Ty<'tcx>,
    normal: &HashSet<BasicBlock>,
    entries: &[BasicBlock],
    calls: &[Call<'tcx>],
) -> bool {
    for block in normal {
        let TerminatorKind::Drop { place, unwind, .. } =
            &body.basic_blocks[*block].terminator().kind
        else {
            continue;
        };
        if !drop_is_protocol_temporary(tcx, instance, body, serializer, place, calls) {
            return false;
        }
        if let UnwindAction::Cleanup(cleanup) = unwind {
            if !entries.contains(cleanup) {
                return false;
            }
        }
    }
    let mut pending: Vec<_> = entries.iter().rev().map(|block| (*block, false)).collect();
    let mut active = HashSet::new();
    let mut complete = HashSet::new();
    while let Some((block, leaving)) = pending.pop() {
        if normal.contains(&block) {
            return false;
        }
        if leaving {
            active.remove(&block);
            complete.insert(block);
            continue;
        }
        if complete.contains(&block) {
            continue;
        }
        if !active.insert(block) {
            return false;
        }
        pending.push((block, true));
        let data = &body.basic_blocks[block];
        if !data.is_cleanup {
            return false;
        }
        if data.statements.iter().any(|statement| {
            !(matches!(
                statement.kind,
                StatementKind::StorageLive(_)
                    | StatementKind::StorageDead(_)
                    | StatementKind::Nop
                    | StatementKind::FakeRead(_)
            ) || literal_bool_drop_flag_assignment(tcx, body, statement))
        }) {
            return false;
        }
        let mut successors = Vec::new();
        match &data.terminator().kind {
            TerminatorKind::Goto { target } => successors.push(*target),
            TerminatorKind::SwitchInt { discr, targets }
                if switch_is_literal_drop_flag(tcx, body, discr) =>
            {
                successors.extend(targets.all_targets().iter().copied());
            }
            TerminatorKind::Drop {
                place,
                target,
                unwind,
                ..
            } => {
                if !drop_is_protocol_temporary(tcx, instance, body, serializer, place, calls) {
                    return false;
                }
                successors.push(*target);
                if let UnwindAction::Cleanup(next) = unwind {
                    successors.push(*next);
                }
            }
            TerminatorKind::UnwindResume
            | TerminatorKind::UnwindTerminate(_)
            | TerminatorKind::Unreachable => (),
            _ => return false,
        }
        for successor in successors.into_iter().rev() {
            pending.push((successor, false));
        }
    }
    true
}

fn drop_is_protocol_temporary<'tcx>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
    body: &mir::Body<'tcx>,
    serializer: Ty<'tcx>,
    place: &Place<'tcx>,
    calls: &[Call<'tcx>],
) -> bool {
    if !place.projection.is_empty() {
        return false;
    }
    let ty = instantiate_ty(tcx, instance, place.ty(body, tcx).ty);
    if ty == serializer
        && original_serializer_alias_local(
            tcx,
            instance,
            body,
            serializer,
            place.local,
            &mut HashSet::new(),
        )
    {
        return true;
    }
    if matches!(
        ty.kind(),
        ty::Ref(..) | ty::Bool | ty::Char | ty::Int(_) | ty::Uint(_) | ty::Float(_)
    ) {
        return true;
    }
    let ty::Alias(_, projection) = ty.kind() else {
        return false;
    };
    let ty::AliasTyKind::Projection { def_id } = projection.kind else {
        return false;
    };
    let Some(trait_id) = tcx.trait_of_assoc(def_id) else {
        return false;
    };
    let Some(state_name) = tcx.opt_item_name(def_id) else {
        return false;
    };
    if !["serde", "serde_core"].iter().any(|crate_name| {
        types::physical_item_path(tcx, trait_id, crate_name, &["ser", "Serializer"])
    }) || projection.args.types().next() != Some(serializer)
    {
        return false;
    }
    let kind = match state_name.as_str() {
        "SerializeMap" => Kind::Map,
        "SerializeSeq" => Kind::Seq,
        "SerializeStruct" => Kind::Struct,
        "SerializeStructVariant" => Kind::StructVariant,
        "SerializeTuple" => Kind::Tuple,
        "SerializeTupleStruct" => Kind::TupleStruct,
        "SerializeTupleVariant" => Kind::TupleVariant,
        _ => return false,
    };
    calls.iter().any(|call| call.kind == kind)
}

fn original_serializer_alias_local<'tcx>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
    body: &mir::Body<'tcx>,
    serializer: Ty<'tcx>,
    local: Local,
    active: &mut HashSet<Local>,
) -> bool {
    if instantiate_ty(tcx, instance, body.local_decls[local].ty) != serializer {
        return false;
    }
    if local == Local::from_usize(2) {
        return !body.basic_blocks.iter().any(|block| {
            block
                .statements
                .iter()
                .any(|statement| match &statement.kind {
                    StatementKind::Assign(assignment) => assignment.0.local == local,
                    StatementKind::SetDiscriminant { place, .. } => place.local == local,
                    _ => false,
                })
                || matches!(&block.terminator().kind,
                TerminatorKind::Call { destination, .. }
                    if destination.local == local)
        });
    }
    if !active.insert(local) {
        return false;
    }
    let mut found = false;
    let mut valid = true;
    for block in body.basic_blocks.iter() {
        for statement in &block.statements {
            match &statement.kind {
                StatementKind::Assign(assignment) if assignment.0.local == local => {
                    let (target, value) = &**assignment;
                    if !target.projection.is_empty() {
                        valid = false;
                        break;
                    }
                    let Rvalue::Use(operand, _) = value else {
                        valid = false;
                        break;
                    };
                    let (Operand::Copy(source) | Operand::Move(source)) = operand else {
                        valid = false;
                        break;
                    };
                    if !source.projection.is_empty()
                        || !original_serializer_alias_local(
                            tcx,
                            instance,
                            body,
                            serializer,
                            source.local,
                            active,
                        )
                    {
                        valid = false;
                        break;
                    }
                    found = true;
                }
                StatementKind::SetDiscriminant { place, .. } if place.local == local => {
                    valid = false;
                    break;
                }
                _ => (),
            }
        }
        if !valid
            || matches!(&block.terminator().kind,
                TerminatorKind::Call { destination, .. }
                    if destination.local == local)
        {
            valid = false;
            break;
        }
    }
    active.remove(&local);
    valid && found
}

fn statements_are_bounded<'tcx>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
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
                        && (finite_count_local(
                            tcx,
                            body,
                            target.local,
                            reachable,
                            &mut HashSet::new(),
                        )
                        .is_some()
                            || rvalue_origin(
                                tcx,
                                instance,
                                body,
                                target.local,
                                value,
                                reachable,
                                calls,
                                &mut HashSet::new(),
                            )
                            .is_some())
                }
                _ => false,
            })
    })
}

fn all_returns_are_results<'tcx>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
    body: &mir::Body<'tcx>,
    reachable: &HashSet<BasicBlock>,
    calls: &[Call<'tcx>],
) -> bool {
    let output = method_output(tcx, instance, body);
    if !is_result(tcx, output) {
        return false;
    }
    for call in calls.iter().filter(|call| call.destination == RETURN_PLACE) {
        let call_output = instantiate_ty(tcx, instance, body.local_decls[call.destination].ty);
        if !is_same_result(tcx, call_output, output) {
            return false;
        }
    }
    matches!(
        local_origin(
            tcx,
            instance,
            body,
            RETURN_PLACE,
            reachable,
            calls,
            &mut HashSet::new(),
        ),
        Some(Origin::Result)
    )
}

fn method_output<'tcx>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
    body: &mir::Body<'tcx>,
) -> Ty<'tcx> {
    instantiate_ty(tcx, instance, body.local_decls[RETURN_PLACE].ty)
}

fn is_result<'tcx>(tcx: TyCtxt<'tcx>, value: Ty<'tcx>) -> bool {
    matches!(value.kind(), ty::Adt(owner, _) if types::physical_item_path(
        tcx,
        owner.did(),
        "core",
        &["result", "Result"],
    ))
}

fn is_same_result<'tcx>(tcx: TyCtxt<'tcx>, left: Ty<'tcx>, right: Ty<'tcx>) -> bool {
    if !is_result(tcx, left) || !is_result(tcx, right) {
        return false;
    }
    let (ty::Adt(_, left_args), ty::Adt(_, right_args)) = (left.kind(), right.kind()) else {
        return false;
    };
    left_args.types().eq(right_args.types())
}

fn is_result_for_serializer<'tcx>(
    tcx: TyCtxt<'tcx>,
    value: Ty<'tcx>,
    serializer: Ty<'tcx>,
) -> bool {
    let ty::Adt(owner, arguments) = value.kind() else {
        return false;
    };
    if !types::physical_item_path(tcx, owner.did(), "core", &["result", "Result"]) {
        return false;
    }
    let Some(error) = arguments.types().nth(1) else {
        return false;
    };
    let ty::Alias(_, alias) = error.kind() else {
        return false;
    };
    matches!(alias.kind, ty::AliasTyKind::Projection { def_id }
        if tcx.opt_item_name(def_id).is_some_and(|name| name.as_str() == "Error")
            && tcx.trait_of_assoc(def_id).is_some_and(|trait_id| {
            ["serde", "serde_core"].iter().any(|crate_name| {
                types::physical_item_path(tcx, trait_id, crate_name, &["ser", "Serializer"])
            })
        }) && alias.args.types().next() == Some(serializer))
}

fn is_result_residual<'tcx>(tcx: TyCtxt<'tcx>, value: Ty<'tcx>, serializer: Ty<'tcx>) -> bool {
    let ty::Adt(owner, arguments) = value.kind() else {
        return false;
    };
    types::physical_item_path(tcx, owner.did(), "core", &["result", "Result"])
        && arguments.types().next().is_some_and(|ok| {
            matches!(ok.kind(), ty::Adt(infallible, args)
            if args.types().next().is_none()
                && types::physical_item_path(
                    tcx,
                    infallible.did(),
                    "core",
                    &["convert", "Infallible"],
                ))
        })
        && is_result_for_serializer(tcx, value, serializer)
}

fn is_control_flow_for_serializer<'tcx>(
    tcx: TyCtxt<'tcx>,
    value: Ty<'tcx>,
    serializer: Ty<'tcx>,
) -> bool {
    let ty::Adt(owner, arguments) = value.kind() else {
        return false;
    };
    types::physical_item_path(
        tcx,
        owner.did(),
        "core",
        &["ops", "control_flow", "ControlFlow"],
    ) && arguments
        .types()
        .next()
        .is_some_and(|residual| is_result_residual(tcx, residual, serializer))
}

fn try_branch_shape<'tcx>(
    tcx: TyCtxt<'tcx>,
    input: Ty<'tcx>,
    output: Ty<'tcx>,
    serializer: Ty<'tcx>,
) -> bool {
    let (ty::Adt(result_owner, result_args), ty::Adt(flow_owner, flow_args)) =
        (input.kind(), output.kind())
    else {
        return false;
    };
    if !types::physical_item_path(tcx, result_owner.did(), "core", &["result", "Result"])
        || !types::physical_item_path(
            tcx,
            flow_owner.did(),
            "core",
            &["ops", "control_flow", "ControlFlow"],
        )
    {
        return false;
    }
    let mut result_args = result_args.types();
    let Some(ok) = result_args.next() else {
        return false;
    };
    let Some(input_error) = result_args.next() else {
        return false;
    };
    let mut flow_args = flow_args.types();
    let Some(residual) = flow_args.next() else {
        return false;
    };
    let Some(output_value) = flow_args.next() else {
        return false;
    };
    let ty::Adt(residual_owner, residual_args) = residual.kind() else {
        return false;
    };
    if !types::physical_item_path(tcx, residual_owner.did(), "core", &["result", "Result"]) {
        return false;
    }
    let Some(residual_error) = residual_args.types().nth(1) else {
        return false;
    };
    output_value == ok
        && residual_error == input_error
        && is_result_for_serializer(tcx, input, serializer)
        && is_result_residual(tcx, residual, serializer)
}

fn branch_output_type<'tcx>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
    body: &mir::Body<'tcx>,
    branch: Local,
) -> Option<Ty<'tcx>> {
    let branch_ty = instantiate_ty(tcx, instance, body.local_decls[branch].ty);
    let ty::Adt(owner, arguments) = branch_ty.kind() else {
        return None;
    };
    types::physical_item_path(
        tcx,
        owner.did(),
        "core",
        &["ops", "control_flow", "ControlFlow"],
    )
    .then(|| arguments.types().nth(1))
    .flatten()
}

fn state_kind_matches(parent: Kind, child: Kind) -> bool {
    match parent {
        Kind::Struct => matches!(child, Kind::StructField | Kind::SkipField | Kind::End),
        Kind::StructVariant => matches!(
            child,
            Kind::StructVariantField | Kind::SkipField | Kind::End
        ),
        Kind::Map => matches!(
            child,
            Kind::MapEntry | Kind::MapKey | Kind::MapValue | Kind::End
        ),
        Kind::Seq => matches!(child, Kind::Element | Kind::End),
        Kind::Tuple => matches!(child, Kind::Element | Kind::End),
        Kind::TupleStruct => matches!(child, Kind::TupleStructField | Kind::End),
        Kind::TupleVariant => matches!(child, Kind::TupleVariantField | Kind::End),
        _ => false,
    }
}

fn state_trait_matches(tcx: TyCtxt<'_>, parent: Kind, method: DefId) -> bool {
    let expected = match parent {
        Kind::Struct => "SerializeStruct",
        Kind::StructVariant => "SerializeStructVariant",
        Kind::Map => "SerializeMap",
        Kind::Seq => "SerializeSeq",
        Kind::Tuple => "SerializeTuple",
        Kind::TupleStruct => "SerializeTupleStruct",
        Kind::TupleVariant => "SerializeTupleVariant",
        _ => return false,
    };
    tcx.trait_of_assoc(method).is_some_and(|trait_id| {
        ["serde", "serde_core"].iter().any(|crate_name| {
            types::physical_item_path(tcx, trait_id, crate_name, &["ser", expected])
        })
    })
}

fn operand_origin<'tcx>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
    body: &mir::Body<'tcx>,
    operand: &Operand<'tcx>,
    reachable: &HashSet<BasicBlock>,
    calls: &[Call<'tcx>],
    active: &mut HashSet<Local>,
) -> Option<Origin> {
    match operand {
        Operand::Copy(place) | Operand::Move(place) => {
            place_origin(tcx, instance, body, place, reachable, calls, active)
        }
        Operand::Constant(constant) => {
            let value_ty = instantiate_ty(tcx, instance, operand.ty(body, tcx));
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
    instance: Instance<'tcx>,
    body: &mir::Body<'tcx>,
    place: &Place<'tcx>,
    reachable: &HashSet<BasicBlock>,
    calls: &[Call<'tcx>],
    active: &mut HashSet<Local>,
) -> Option<Origin> {
    if place.local == Local::from_usize(1) {
        return source_projection(body, place)
            .then_some(Origin::Data {
                source: true,
                fixed: false,
            })
            .or_else(|| {
                place.projection.is_empty().then_some(Origin::Data {
                    source: true,
                    fixed: false,
                })
            });
    }
    if place.local == Local::from_usize(2)
        && place
            .projection
            .iter()
            .all(|projection| matches!(projection, mir::ProjectionElem::Deref))
    {
        return Some(Origin::Serializer);
    }
    if calls
        .iter()
        .any(|call| call.destination == place.local && call.kind == Kind::TryBranch)
    {
        return branch_projection(tcx, instance, body, place);
    }
    if place.projection.is_empty() && calls.iter().any(|call| call.destination == place.local) {
        return local_origin(tcx, instance, body, place.local, reachable, calls, active);
    }
    if !place.projection.is_empty() {
        if place.projection.len() == 1 {
            if let Some(mir::ProjectionElem::Field(index, _)) = place.projection.first() {
                if checked_add_bound(tcx, body, place.local, reachable, active).is_some() {
                    return match index.as_usize() {
                        0 => Some(Origin::Data {
                            source: false,
                            fixed: true,
                        }),
                        1 => Some(Origin::Predicate),
                        _ => None,
                    };
                }
            }
        }
        // The only constructed wrapper admitted here is Serde's exact flatten
        // adapter around a successful state value.
        if flat_map_type(tcx, instantiate_ty(tcx, instance, place.ty(body, tcx).ty)) {
            return local_origin(tcx, instance, body, place.local, reachable, calls, active);
        }
        return None;
    }
    if finite_count_local(tcx, body, place.local, reachable, &mut HashSet::new()).is_some() {
        return Some(Origin::Data {
            source: false,
            fixed: true,
        });
    }
    local_origin(tcx, instance, body, place.local, reachable, calls, active)
}

fn local_origin<'tcx>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
    body: &mir::Body<'tcx>,
    local: Local,
    reachable: &HashSet<BasicBlock>,
    calls: &[Call<'tcx>],
    active: &mut HashSet<Local>,
) -> Option<Origin> {
    if local == Local::from_usize(1) {
        return Some(Origin::Data {
            source: true,
            fixed: false,
        });
    }
    if local == Local::from_usize(2) {
        return Some(Origin::Serializer);
    }
    if !active.insert(local) {
        return None;
    }
    let result = (|| {
        let mut origin = None;
        let mut found = false;
        for call in calls.iter().filter(|call| call.destination == local) {
            origin = Some(merge_origin(origin, call_origin(call))?);
            found = true;
        }
        for block in reachable {
            for statement in &body.basic_blocks[*block].statements {
                let StatementKind::Assign(assignment) = &statement.kind else {
                    continue;
                };
                let (target, value) = &**assignment;
                if target.local != local || !target.projection.is_empty() {
                    continue;
                }
                let next = rvalue_origin(
                    tcx,
                    instance,
                    body,
                    target.local,
                    value,
                    reachable,
                    calls,
                    active,
                )?;
                origin = Some(merge_origin(origin, next)?);
                found = true;
            }
        }
        found.then_some(origin?)
    })();
    active.remove(&local);
    result
}

fn rvalue_origin<'tcx>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
    body: &mir::Body<'tcx>,
    destination: Local,
    value: &Rvalue<'tcx>,
    reachable: &HashSet<BasicBlock>,
    calls: &[Call<'tcx>],
    active: &mut HashSet<Local>,
) -> Option<Origin> {
    match value {
        Rvalue::Use(operand, _) => {
            operand_origin(tcx, instance, body, operand, reachable, calls, active)
        }
        Rvalue::Ref(_, BorrowKind::Shared, place) | Rvalue::CopyForDeref(place) => {
            place_origin(tcx, instance, body, place, reachable, calls, active)
        }
        Rvalue::Ref(_, BorrowKind::Mut { .. }, place) => {
            match place_origin(tcx, instance, body, place, reachable, calls, active)? {
                origin @ Origin::BranchContinue(_) => Some(origin),
                _ => None,
            }
        }
        Rvalue::Cast(
            mir::CastKind::PointerCoercion(ty::adjustment::PointerCoercion::Unsize, _),
            operand,
            _,
        ) => operand_origin(tcx, instance, body, operand, reachable, calls, active),
        Rvalue::Cast(mir::CastKind::IntToInt, operand, _) => {
            let operand_ty = instantiate_ty(tcx, instance, operand.ty(body, tcx));
            if !matches!(operand_ty.kind(), ty::Bool) {
                return None;
            }
            matches!(
                operand_origin(tcx, instance, body, operand, reachable, calls, active)?,
                Origin::Predicate | Origin::Data { .. }
            )
            .then_some(Origin::Data {
                source: false,
                fixed: true,
            })
        }
        Rvalue::Discriminant(place) => {
            if source_enum_discriminant(tcx, instance, body, place) {
                Some(Origin::Data {
                    source: true,
                    fixed: false,
                })
            } else {
                match place_origin(tcx, instance, body, place, reachable, calls, active)? {
                    Origin::BranchValue(branch) => Some(Origin::BranchTag(branch)),
                    origin @ Origin::Data { .. } => Some(origin),
                    _ => None,
                }
            }
        }
        Rvalue::Aggregate(_, operands) => {
            let target_ty = instantiate_ty(tcx, instance, body.local_decls[destination].ty);
            if flat_map_type(tcx, target_ty) && operands.len() == 1 {
                return match operand_origin(
                    tcx,
                    instance,
                    body,
                    operands.iter().next()?,
                    reachable,
                    calls,
                    active,
                )? {
                    Origin::BranchContinue(branch) => Some(Origin::FlatMap(branch)),
                    _ => None,
                };
            }
            let ty::Adt(owner, _) = target_ty.kind() else {
                return None;
            };
            if !types::physical_item_path(tcx, owner.did(), "core", &["option", "Option"]) {
                return None;
            }
            let fixed = operands.iter().all(|operand| {
                matches!(
                    operand_origin(tcx, instance, body, operand, reachable, calls, active),
                    Some(Origin::Data {
                        source: false,
                        fixed: true
                    })
                )
            });
            fixed.then_some(Origin::Data {
                source: false,
                fixed: true,
            })
        }
        Rvalue::BinaryOp(BinOp::Add, operands) => {
            let (left, right) = &**operands;
            let left = operand_origin(tcx, instance, body, left, reachable, calls, active)?;
            let right = operand_origin(tcx, instance, body, right, reachable, calls, active)?;
            (finite_count_data(left) && finite_count_data(right)).then_some(Origin::Data {
                source: false,
                fixed: true,
            })
        }
        Rvalue::BinaryOp(BinOp::AddWithOverflow, _) => {
            checked_add_bound(tcx, body, destination, reachable, active).map(|_| Origin::Data {
                source: false,
                fixed: true,
            })
        }
        Rvalue::UnaryOp(_, operand)
            if matches!(
                instantiate_ty(tcx, instance, body.local_decls[destination].ty).kind(),
                ty::Bool
            ) =>
        {
            match operand_origin(tcx, instance, body, operand, reachable, calls, active)? {
                Origin::Predicate => Some(Origin::Predicate),
                origin @ Origin::Data { .. } => Some(origin),
                _ => None,
            }
        }
        _ => None,
    }
}

fn source_projection<'tcx>(body: &mir::Body<'tcx>, place: &Place<'tcx>) -> bool {
    if place.projection.is_empty() {
        return false;
    }
    let mut root_deref = false;
    let mut field = false;
    let mut current = body.local_decls[place.local].ty;
    for projection in place.projection.iter() {
        match projection {
            mir::ProjectionElem::Deref if !root_deref => {
                let ty::Ref(_, inner, ty::Mutability::Not) = current.kind() else {
                    return false;
                };
                current = *inner;
                root_deref = true;
            }
            mir::ProjectionElem::Field(_, field_ty) if root_deref => {
                current = field_ty;
                field = true;
            }
            mir::ProjectionElem::Downcast(_, _) if root_deref => (),
            _ => return false,
        }
    }
    root_deref && field
}

fn source_enum_discriminant<'tcx>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
    body: &mir::Body<'tcx>,
    place: &Place<'tcx>,
) -> bool {
    if place.local != Local::from_usize(1)
        || !place
            .projection
            .iter()
            .all(|projection| matches!(projection, mir::ProjectionElem::Deref))
    {
        return false;
    }
    let source =
        instantiate_ty(tcx, instance, body.local_decls[Local::from_usize(1)].ty).peel_refs();
    let projected = instantiate_ty(tcx, instance, place.ty(body, tcx).ty);
    projected == source && matches!(source.kind(), ty::Adt(owner, _) if owner.is_enum())
}

fn is_bounded_data(origin: Origin) -> bool {
    matches!(
        origin,
        Origin::Data { source: true, .. } | Origin::Data { fixed: true, .. }
    )
}

fn finite_count_data(origin: Origin) -> bool {
    matches!(
        origin,
        Origin::Predicate
            | Origin::Data {
                source: false,
                fixed: true
            }
    )
}

fn merge_origin(current: Option<Origin>, next: Origin) -> Option<Origin> {
    match (current, next) {
        (None, next) => Some(next),
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

fn fixed_constant_type(tcx: TyCtxt<'_>, value: Ty<'_>) -> bool {
    match value.kind() {
        ty::Bool | ty::Char | ty::Int(_) | ty::Uint(_) | ty::Float(_) => true,
        ty::Ref(_, inner, ty::Mutability::Not) => match inner.kind() {
            ty::Str => true,
            ty::Array(element, _) | ty::Slice(element) => fixed_constant_type(tcx, *element),
            _ => false,
        },
        ty::Array(element, _) => fixed_constant_type(tcx, *element),
        ty::Tuple(fields) => fields.iter().all(|field| fixed_constant_type(tcx, field)),
        ty::Adt(owner, args)
            if types::physical_item_path(tcx, owner.did(), "core", &["option", "Option"]) =>
        {
            args.types()
                .next()
                .is_some_and(|inner| fixed_constant_type(tcx, inner))
        }
        _ => false,
    }
}

fn flat_map_type(tcx: TyCtxt<'_>, value: Ty<'_>) -> bool {
    matches!(value.peel_refs().kind(), ty::Adt(owner, _)
        if types::physical_item_path(tcx, owner.did(), "serde", &["private", "ser", "FlatMapSerializer"]))
}

fn finite_count_operand<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    operand: &Operand<'tcx>,
    reachable: &HashSet<BasicBlock>,
    active: &mut HashSet<Local>,
) -> Option<u128> {
    match operand {
        Operand::Constant(constant) => constant
            .const_
            .try_eval_target_usize(tcx, ty::TypingEnv::fully_monomorphized())
            .map(u128::from),
        Operand::Copy(place) | Operand::Move(place) if place.projection.is_empty() => {
            finite_count_local(tcx, body, place.local, reachable, active)
        }
        Operand::Copy(place) | Operand::Move(place)
            if place.projection.len() == 1
                && matches!(
                    place.projection.first(),
                    Some(mir::ProjectionElem::Field(index, _)) if index.as_usize() == 0
                ) =>
        {
            if !active.insert(place.local) {
                return None;
            }
            let result = checked_add_bound(tcx, body, place.local, reachable, active);
            active.remove(&place.local);
            result
        }
        _ => None,
    }
}

fn finite_count_local<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    local: Local,
    reachable: &HashSet<BasicBlock>,
    active: &mut HashSet<Local>,
) -> Option<u128> {
    if matches!(body.local_decls[local].ty.kind(), ty::Bool) {
        return Some(1);
    }
    if local.as_usize() <= body.arg_count {
        return None;
    }
    if active.len() >= body.local_decls.len() || !active.insert(local) {
        return None;
    }
    let mut base_max = None::<u128>;
    let mut increment_total = 0u128;
    let mut found = false;
    for block in reachable {
        for statement in &body.basic_blocks[*block].statements {
            let StatementKind::Assign(assignment) = &statement.kind else {
                continue;
            };
            let (target, value) = &**assignment;
            if target.local != local || !target.projection.is_empty() {
                continue;
            }
            found = true;
            if let Rvalue::BinaryOp(BinOp::Add, operands) = value {
                let (left, right) = &**operands;
                let self_left = operand_is_local(left, local);
                let self_right = operand_is_local(right, local);
                if self_left || self_right {
                    if self_left == self_right {
                        active.remove(&local);
                        return None;
                    }
                    let increment = if self_left { right } else { left };
                    let Some(increment) =
                        finite_count_operand(tcx, body, increment, reachable, active)
                    else {
                        active.remove(&local);
                        return None;
                    };
                    let Some(total) = increment_total.checked_add(increment) else {
                        active.remove(&local);
                        return None;
                    };
                    increment_total = total;
                    continue;
                }
            }
            let Some(value) = finite_count_rvalue(tcx, body, value, reachable, active) else {
                active.remove(&local);
                return None;
            };
            base_max = Some(base_max.map_or(value, |current| current.max(value)));
        }
    }
    active.remove(&local);
    if !found {
        return None;
    }
    base_max?.checked_add(increment_total)
}

fn finite_count_rvalue<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mir::Body<'tcx>,
    value: &Rvalue<'tcx>,
    reachable: &HashSet<BasicBlock>,
    active: &mut HashSet<Local>,
) -> Option<u128> {
    match value {
        Rvalue::Use(operand, _) => finite_count_operand(tcx, body, operand, reachable, active),
        Rvalue::Cast(mir::CastKind::IntToInt, operand, _) => {
            matches!(operand.ty(body, tcx).kind(), ty::Bool)
                .then_some(1)
                .or_else(|| finite_count_operand(tcx, body, operand, reachable, active))
        }
        Rvalue::BinaryOp(BinOp::Add, operands) => {
            let (left, right) = &**operands;
            finite_count_operand(tcx, body, left, reachable, active)?
                .checked_add(finite_count_operand(tcx, body, right, reachable, active)?)
        }
        Rvalue::Aggregate(kind, operands) => {
            let mir::AggregateKind::Adt(definition, variant, _, _, _) = &**kind else {
                return None;
            };
            if !types::physical_item_path(tcx, *definition, "core", &["option", "Option"]) {
                return None;
            }
            match tcx.adt_def(*definition).variant(*variant).name.as_str() {
                "None" if operands.is_empty() => Some(0),
                "Some" if operands.len() == 1 => {
                    finite_count_operand(tcx, body, operands.iter().next()?, reachable, active)
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn operand_is_local(operand: &Operand<'_>, local: Local) -> bool {
    matches!(operand, Operand::Copy(place) | Operand::Move(place)
        if place.local == local && place.projection.is_empty())
}

fn control_flow_targets<'tcx>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
    body: &mir::Body<'tcx>,
    branch: Local,
    targets: &mir::SwitchTargets,
) -> Option<(BasicBlock, BasicBlock)> {
    let value = instantiate_ty(tcx, instance, body.local_decls[branch].ty);
    let ty::Adt(owner, _) = value.kind() else {
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

fn branch_projection<'tcx>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
    body: &mir::Body<'tcx>,
    place: &Place<'tcx>,
) -> Option<Origin> {
    if place.projection.is_empty() {
        return Some(Origin::BranchValue(place.local));
    }
    let value = instantiate_ty(tcx, instance, body.local_decls[place.local].ty);
    let ty::Adt(owner, _) = value.kind() else {
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
    let mut variant = None;
    let mut field = false;
    for projection in place.projection.iter() {
        match projection {
            mir::ProjectionElem::Downcast(_, index) if variant.is_none() => {
                variant = Some(owner.variant(index).name.as_str().to_owned());
            }
            mir::ProjectionElem::Field(index, _)
                if variant.is_some() && !field && index.as_usize() == 0 =>
            {
                field = true
            }
            _ => return None,
        }
    }
    if !field {
        return None;
    }
    match variant.as_deref()? {
        "Continue" => Some(Origin::BranchContinue(place.local)),
        "Break" => Some(Origin::BranchResidual(place.local)),
        _ => None,
    }
}

fn call_origin(call: &Call<'_>) -> Origin {
    match call.kind {
        Kind::TryBranch => Origin::BranchValue(call.destination),
        Kind::Predicate => Origin::Predicate,
        Kind::FromResidual
        | Kind::End
        | Kind::Some
        | Kind::NewtypeStruct
        | Kind::NewtypeVariant
        | Kind::Unit
        | Kind::UnitStruct
        | Kind::UnitVariant => Origin::Result,
        _ if call.destination == RETURN_PLACE => Origin::Result,
        _ => Origin::OperationResult(call.destination),
    }
}

fn normal_successors(terminator: &TerminatorKind<'_>) -> Vec<BasicBlock> {
    match terminator {
        TerminatorKind::Goto { target } => vec![*target],
        TerminatorKind::Call {
            target: Some(target),
            ..
        } => vec![*target],
        TerminatorKind::Drop { target, .. } => vec![*target],
        TerminatorKind::Assert { target, .. } => vec![*target],
        TerminatorKind::SwitchInt { targets, .. } => targets.all_targets().to_vec(),
        _ => Vec::new(),
    }
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

fn dominates(
    body: &mir::Body<'_>,
    reachable: &HashSet<BasicBlock>,
    dominator: BasicBlock,
    target: BasicBlock,
) -> bool {
    if dominator == target {
        return true;
    }
    let mut pending = vec![mir::START_BLOCK];
    let mut seen = HashSet::new();
    while let Some(block) = pending.pop() {
        if block == dominator || !seen.insert(block) || !reachable.contains(&block) {
            continue;
        }
        if block == target {
            return false;
        }
        pending.extend(normal_successors(
            &body.basic_blocks[block].terminator().kind,
        ));
    }
    true
}
