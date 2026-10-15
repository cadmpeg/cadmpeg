// SPDX-License-Identifier: Apache-2.0

use super::super::{
    consumed_support_sequences, linear_nurbs_parameters, BoundaryEndpoint,
    BoundaryVertexDerivation, BoundaryVertexSourceEndpoint, WireProjectionOutcome,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, BTreeSet};

fn outcome<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    decoded: BTreeSet<u32>,
) -> WireProjectionOutcome<'ctx> {
    // Prebuilt unit inputs own real backing without decode traversal or creation.
    let node_bytes = 11 * std::mem::size_of::<u32>()
        + 16 * std::mem::size_of::<usize>()
        + 2 * std::mem::align_of::<u32>().max(std::mem::align_of::<usize>());
    let nodes = if decoded.is_empty() {
        0
    } else {
        (decoded.len() - 1) / 5 + 1
    };
    WireProjectionOutcome {
        decoded,
        decoded_storage: ctx
            .reserve_scoped(
                u64::try_from(nodes * node_bytes).unwrap(),
                "test wire decoded input backing",
            )
            .unwrap(),
        losses: Vec::new(),
        loss_slots_storage: ctx
            .reserve_scoped(0, "test wire loss input backing")
            .unwrap(),
        wire_edges: Vec::new(),
        wire_slots_storage: ctx
            .reserve_scoped(0, "test wire edge input backing")
            .unwrap(),
    }
}

fn boundary(work: u64, additional: u64, operation: &'static str) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut decoded_storage = ctx
        .reserve_scoped(0, "test merged decoded storage")
        .unwrap();
    let mut decoded = BTreeSet::new();
    let initial = outcome(&ctx, BTreeSet::from([1, 3, 5]));
    let replays = [
        outcome(&ctx, BTreeSet::from([1, 3, 5])),
        outcome(&ctx, BTreeSet::new()),
    ];
    let first = match initial.merge_into(
        &mut decoded,
        &mut decoded_storage,
        &mut Vec::new(),
        &mut Vec::new(),
        &ctx,
    ) {
        Err(CodecError::ResourceLimit(first)) => first,
        other => panic!("expected actual merged source/allocation refusal: {other:?}"),
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, operation);
    assert_eq!(
        (first.limit, first.used, first.additional),
        (work, work, additional)
    );
    assert!(decoded.is_empty());
    for replay in replays {
        assert!(matches!(replay.merge_into(
            &mut decoded, &mut decoded_storage, &mut Vec::new(), &mut Vec::new(), &ctx),
            Err(CodecError::ResourceLimit(last)) if last == first));
    }
    drop(decoded);
    drop(decoded_storage);
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn wire_merge_source_refuses_one_visit_before_any_decoded_insertion() {
    boundary(0, 1, "iges merged decoded traversal");
}

#[test]
fn wire_merge_node_work_refuses_after_one_visit_without_admitting_the_tail() {
    // An empty u32 set adds one node: three admitted node passes.
    let alignment = std::mem::align_of::<u32>().max(std::mem::align_of::<usize>());
    let node_bytes =
        11 * std::mem::size_of::<u32>() + 16 * std::mem::size_of::<usize>() + 2 * alignment;
    boundary(
        1,
        u64::try_from(3 * node_bytes).unwrap(),
        "iges merged decoded sequences",
    );
}

#[test]
fn empty_wire_merge_executes_no_source_steps() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut decoded_storage = ctx
        .reserve_scoped(0, "test merged decoded storage")
        .unwrap();
    outcome(&ctx, BTreeSet::new())
        .merge_into(
            &mut BTreeSet::new(),
            &mut decoded_storage,
            &mut Vec::new(),
            &mut Vec::new(),
            &ctx,
        )
        .unwrap();
    drop(decoded_storage);
    ctx.finish_session().unwrap();
}

fn linear_parameter_refusal(work: u64, range: [f64; 2], additional: u64, operation: &'static str) {
    let knots: Vec<_> = (0..1002).map(f64::from).collect();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(first)) =
        linear_nurbs_parameters(1, &knots, 1000, false, range, &ctx)
    else {
        panic!("expected linear parameter refusal")
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, operation);
    assert_eq!(
        (first.limit, first.used, first.additional),
        (work, work, additional)
    );
    assert!(
        matches!(linear_nurbs_parameters(1, &knots, 1000, false, range, &ctx),
        Err(CodecError::ResourceLimit(last)) if last == first)
    );
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn linear_parameter_source_refuses_one_visit_after_complete_validation() {
    // The accepted core validator currently admits1002 visits and one terminal probe.
    linear_parameter_refusal(
        1002 + 1,
        [500.25, 500.75],
        1,
        "iges linear NURBS parameter knots",
    );
}

#[test]
fn linear_parameter_source_refuses_its_last_outside_interval_knot() {
    // Validation1003 plus the first1001 source visits precede the final knot.
    linear_parameter_refusal(
        1002 + 1 + 1001,
        [500.25, 500.75],
        1,
        "iges linear NURBS parameter knots",
    );
}

#[test]
fn linear_parameter_growth_refuses_after_six_visits_without_admitting_the_tail() {
    // The initial range endpoint reserves four f64 slots. Knots0/1 are skipped,
    // knots2/3/4 fill that backing, and knot5 requests growth that moves32 bytes.
    linear_parameter_refusal(
        1002 + 1 + 6,
        [1.0, 1000.0],
        u64::try_from(4 * std::mem::size_of::<f64>()).unwrap(),
        "iges linear NURBS parameters",
    );
}

#[test]
fn linear_parameter_source_accepts_exact_complete_work_for_two_output_values() {
    let knots: Vec<_> = (0..1002).map(f64::from).collect();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // The existing validator's end probe is separate from the exact1002 source visits.
    policy.limits.max_work_units = 1002 + 1 + 1002;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (parameters, storage) =
        linear_nurbs_parameters(1, &knots, 1000, false, [500.25, 500.75], &ctx)
            .unwrap()
            .unwrap();
    assert_eq!(parameters, [500.25, 500.75]);
    drop(parameters);
    drop(storage);
    ctx.finish_session().unwrap();
}

const DERIVATION_SOURCE: &str = "iges:entity:directory#1";

fn derivation_input() -> (cadmpeg_ir::ids::VertexId, Vec<BoundaryVertexSourceEndpoint>) {
    use cadmpeg_ir::features::FinitePoint3;
    use cadmpeg_ir::math::Point3;
    let vertex = cadmpeg_ir::ids::VertexId::mint("iges:model:vertex#fixture").unwrap();
    let endpoints = [
        ("abc", BoundaryEndpoint::Start, Point3::new(0.0, 0.0, 0.0)),
        ("def", BoundaryEndpoint::End, Point3::new(1.0, 0.0, 0.0)),
        ("ghi", BoundaryEndpoint::Start, Point3::new(2.0, 0.0, 0.0)),
    ]
    .into_iter()
    .map(|(edge, endpoint, position)| BoundaryVertexSourceEndpoint {
        edge: edge.into(),
        endpoint,
        position: FinitePoint3::new(position).unwrap(),
    })
    .collect();
    (vertex, endpoints)
}

fn derivation_source_refusal(work: u64, operation: &'static str, additional: u64) {
    let (vertex, endpoints) = derivation_input();
    let members = [2, 0, 1];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut storage = ctx
        .reserve_scoped(0, "test derivation source storage")
        .unwrap();
    let Err(CodecError::ResourceLimit(first)) = BoundaryVertexDerivation::for_decode(
        (DERIVATION_SOURCE, &vertex),
        endpoints[0].position,
        0.25,
        &endpoints,
        &members,
        &ctx,
        &mut storage,
    ) else {
        panic!("expected boundary derivation source or endpoint copy refusal");
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, operation);
    assert_eq!(
        (first.limit, first.used, first.additional),
        (work, work, additional)
    );
    for replay in [members.as_slice(), &[]] {
        assert!(matches!(BoundaryVertexDerivation::for_decode(
            (DERIVATION_SOURCE, &vertex), endpoints[0].position, 0.25,
            &endpoints, replay, &ctx, &mut storage,
        ), Err(CodecError::ResourceLimit(last)) if last == first));
    }
    drop(storage);
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn derivation_member_source_refuses_one_visit_before_the_first_endpoint_copy() {
    derivation_source_refusal(0, "iges boundary derivation member traversal", 1);
}

#[test]
fn derivation_endpoint_copy_refuses_after_one_visit_without_admitting_the_tail() {
    derivation_source_refusal(1, "iges boundary derivation edge text", 3);
}

#[test]
fn derivation_endpoint_allocation_refuses_before_any_member_visit() {
    let (vertex, endpoints) = derivation_input();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut storage = ctx
        .reserve_scoped(0, "test derivation source storage")
        .unwrap();
    let Err(CodecError::ResourceLimit(first)) = BoundaryVertexDerivation::for_decode(
        (DERIVATION_SOURCE, &vertex),
        endpoints[0].position,
        0.25,
        &endpoints,
        &[2, 0, 1],
        &ctx,
        &mut storage,
    ) else {
        panic!("expected endpoint allocation before any member visit");
    };
    assert_eq!(first.dimension, ResourceDimension::CollectionItems);
    assert_eq!(first.operation, "iges boundary derivation endpoints");
    assert_eq!((first.limit, first.used, first.additional), (0, 0, 3));
    assert!(matches!(BoundaryVertexDerivation::for_decode(
        (DERIVATION_SOURCE, &vertex), endpoints[0].position, 0.25,
        &endpoints, &[], &ctx, &mut storage,
    ), Err(CodecError::ResourceLimit(last)) if last == first));
    drop(storage);
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn derivation_member_source_preserves_selected_order_and_owns_exact_copied_backing() {
    let (vertex, endpoints) = derivation_input();
    for members in [[2, 0, 1].as_slice(), &[]] {
        let copied_text = DERIVATION_SOURCE.len()
            + vertex.as_str().len()
            + members
                .iter()
                .map(|index| endpoints[*index].edge.len())
                .sum::<usize>();
        let materialized = u64::try_from(
            copied_text + members.len() * std::mem::size_of::<BoundaryVertexSourceEndpoint>(),
        )
        .unwrap();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::try_from(members.len() + copied_text).unwrap();
        policy.limits.max_materialized_bytes = materialized;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut storage = ctx
            .reserve_scoped(0, "test derivation source storage")
            .unwrap();
        let copy = BoundaryVertexDerivation::for_decode(
            (DERIVATION_SOURCE, &vertex),
            endpoints[0].position,
            0.25,
            &endpoints,
            members,
            &ctx,
            &mut storage,
        )
        .unwrap();
        assert_eq!(copy.source_entity, DERIVATION_SOURCE);
        assert_eq!(copy.vertex, vertex);
        assert_eq!(copy.representative, endpoints[0].position);
        assert_eq!(copy.tolerance, 0.25);
        assert_eq!(copy.source_endpoints.len(), members.len());
        for (copied, member) in copy.source_endpoints.iter().zip(members) {
            assert_eq!(copied.edge, endpoints[*member].edge);
            assert_eq!(copied.endpoint, endpoints[*member].endpoint);
            assert_eq!(copied.position, endpoints[*member].position);
        }
        drop(copy);
        drop(storage);
        let released = ctx
            .reserve_scoped(materialized, "test released derivation backing")
            .unwrap();
        drop(released);
        ctx.finish_session().unwrap();
    }
}

fn assembly_record(values: &[i64]) -> crate::parameter::ParameterRecord {
    use crate::parameter::{ParameterRecord, Token, TokenValue};
    ParameterRecord::from_test_tokens(
        1,
        1..2,
        Vec::new(),
        values.len(),
        values
            .iter()
            .map(|value| Token {
                value: TokenValue::Integer(*value),
                span: 0..0,
            })
            .collect(),
        Vec::new(),
    )
}

fn assembly_source_refusal(completed_members: u64) {
    let directory = [crate::test_support::directory_target(1, 184)];
    let record = assembly_record(&[184, 3, 0, 0, 0, 0, 0, 0]);
    let records = BTreeMap::from([(1, &record)]);
    // Two Directory visits and one comparison of the singleton u32 key precede
    // the members. Zero transform pointers execute no lookup or insertion.
    let prelude = 2 + u64::try_from(std::mem::size_of::<u32>()).unwrap();
    let work = prelude + completed_members;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(first)) =
        consumed_support_sequences(&directory, &records, &ctx)
    else {
        panic!("expected assembly member source refusal");
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, "iges consumed-support assembly members");
    assert_eq!((first.limit, first.used, first.additional), (work, work, 1));
    for _ in 0..64 {
        for replay in [directory.as_slice(), &[]] {
            assert!(matches!(consumed_support_sequences(replay, &records, &ctx),
                Err(CodecError::ResourceLimit(last)) if last == first));
        }
    }
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn consumed_assembly_source_refuses_before_first_member() {
    assembly_source_refusal(0);
}

#[test]
fn consumed_assembly_source_refuses_before_last_member() {
    assembly_source_refusal(2);
}

#[test]
fn consumed_assembly_source_accepts_exact_member_work_without_backing() {
    let directory = [crate::test_support::directory_target(1, 184)];
    let record = assembly_record(&[184, 3, 0, 0, 0, 0, 0, 0]);
    let records = BTreeMap::from([(1, &record)]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Three Directory visits, one singleton u32 lookup and three member visits.
    policy.limits.max_work_units = 3 + u64::try_from(std::mem::size_of::<u32>()).unwrap() + 3;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (consumed, storage) = consumed_support_sequences(&directory, &records, &ctx).unwrap();
    assert!(consumed.is_empty());
    drop(consumed);
    drop(storage);
    ctx.finish_session().unwrap();
}

#[test]
fn consumed_assembly_source_skips_invalid_and_zero_counts_without_member_work() {
    let directory = [crate::test_support::directory_target(1, 184)];
    for values in [
        &[184][..],
        &[184, -1, 0, 0, 0],
        &[184, 10, 0, 0, 0],
        &[184, 0, 0, 0, 0],
    ] {
        let record = assembly_record(values);
        let records = BTreeMap::from([(1, &record)]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Only three Directory visits and the singleton lookup execute.
        policy.limits.max_work_units = 3 + u64::try_from(std::mem::size_of::<u32>()).unwrap();
        policy.limits.max_collection_items = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let (consumed, storage) = consumed_support_sequences(&directory, &records, &ctx).unwrap();
        assert!(consumed.is_empty());
        drop(consumed);
        drop(storage);
        ctx.finish_session().unwrap();
    }
}

#[test]
fn consumed_assembly_source_empty_input_executes_no_work() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (consumed, storage) = consumed_support_sequences(&[], &BTreeMap::new(), &ctx).unwrap();
    assert!(consumed.is_empty());
    drop(consumed);
    drop(storage);
    ctx.finish_session().unwrap();
}
