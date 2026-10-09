// SPDX-License-Identifier: Apache-2.0

use super::super::{project, CopiousProjectionOutcome};
use super::super::super::geometry::SourceSequences;
use crate::parameter::{ParameterRecord, Token, TokenValue};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::{EdgeId, VertexId};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};
use std::mem::{align_of, size_of};

// The core materialization allowance is min(policy ceiling, base +
// 1000 * physical input bytes). These caller sessions have no root bytes.
const EMPTY_INPUT_MATERIALIZED_ALLOWANCE: u64 = 16 * 1024 * 1024;

const LOSS_VALUES: &[i64] = &[106, 2, 0];
const POINT_VALUES: &[i64] = &[106, 1, 1, 0, 2, 3];
const WIRE_VALUES: &[i64] = &[106, 1, 2, 0, 0, 0, 1, 0];

fn decoded_node_bytes() -> usize {
    11 * size_of::<u32>() + 16 * size_of::<usize>()
        + 2 * align_of::<u32>().max(align_of::<usize>())
}

fn project_fixture<'ctx>(ctx: &'ctx DecodeContext<'_>, ir: &mut CadIr,
    form: i64, values: &[i64]) -> Result<CopiousProjectionOutcome<'ctx>, CodecError> {
    let bytes = crate::test_support::test_owned::owned_test_file(&[]);
    let global = crate::test_support::with_service_context(&bytes, |setup| {
        let scan = crate::card::scan_with_context(&bytes, setup).unwrap();
        let (global, _, _) = crate::global::parse(&scan, setup).unwrap();
        global.length_context().unwrap()
    });
    assert_eq!(global.length_factor_mm(), 1.0);
    let mut entry = crate::test_support::directory_target(1, 106);
    entry.form = form;
    let directory = [entry];
    let entries = BTreeMap::from([(1, &directory[0])]);
    let record = ParameterRecord::from_test_tokens(1, 1..2, Vec::new(), values.len(),
        values.iter().map(|value| Token {
            value: TokenValue::Integer(*value), span: 0..0,
        }).collect(), Vec::new());
    let records = BTreeMap::from([(1, &record)]);
    let mut sequences = SourceSequences::new(ctx)?;
    let result = project(ir, &directory, &entries, &records, &global, ctx, &mut sequences);
    // This test has no later appearance lookup. Destroy the actual index
    // before measuring the independently owned outcome buffers.
    drop(sequences);
    result
}

fn assert_live_backing(form: i64, values: &[i64], backing: usize) {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut ir = CadIr::empty();
    let outcome = project_fixture(&ctx, &mut ir, form, values).unwrap();
    let allowance = policy.limits.max_materialized_bytes.min(EMPTY_INPUT_MATERIALIZED_ALLOWANCE);
    let Err(CodecError::ResourceLimit(first)) = ctx.reserve_scoped(
        allowance, "test concurrent copious backing") else {
        panic!("expected live copious container backing");
    };
    assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(first.operation, "test concurrent copious backing");
    assert_eq!((first.limit, first.used, first.additional), (allowance,
        u64::try_from(backing).unwrap(), allowance));
    drop(outcome);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn copious_loss_slots_remain_scoped_until_the_outcome_is_destroyed() {
    assert_live_backing(12, LOSS_VALUES, 4 * size_of::<LossNote>());
}

#[test]
fn copious_free_vertex_slots_and_decoded_nodes_remain_scoped_until_destruction() {
    assert_live_backing(1, POINT_VALUES, 4 * size_of::<VertexId>() + decoded_node_bytes());
}

#[test]
fn copious_wire_slots_and_decoded_nodes_remain_scoped_until_destruction() {
    assert_live_backing(11, WIRE_VALUES, 4 * size_of::<EdgeId>() + decoded_node_bytes());
}

fn assert_merge_release(form: i64, values: &[i64]) {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut ir = CadIr::empty();
    let outcome = project_fixture(&ctx, &mut ir, form, values).unwrap();
    let expected_losses = outcome.losses.clone();
    let loss_ptr = outcome.losses.first().map(|loss| loss.message.as_ptr());
    let expected_wire = outcome.wire_edges.clone();
    let wire_ptr = outcome.wire_edges.first().map(|id| id.as_str().as_ptr());
    let expected_free = outcome.free_vertices.clone();
    let free_ptr = outcome.free_vertices.first().map(|id| id.as_str().as_ptr());
    let has_decoded = !outcome.decoded.is_empty();
    let mut decoded_storage = ctx.reserve_scoped(0, "test merged copious decoded nodes").unwrap();
    let mut decoded = BTreeSet::new();
    let mut losses = Vec::new();
    let mut wire_edges = Vec::new();
    let mut free_vertices = Vec::new();
    outcome.merge_into(&mut decoded, &mut decoded_storage, &mut losses,
        &mut wire_edges, &mut free_vertices, &ctx).unwrap();
    assert_eq!(decoded, if has_decoded { BTreeSet::from([1]) } else { BTreeSet::new() });
    assert_eq!(losses, expected_losses);
    assert_eq!(losses.first().map(|loss| loss.message.as_ptr()), loss_ptr);
    assert_eq!(wire_edges, expected_wire);
    assert_eq!(wire_edges.first().map(|id| id.as_str().as_ptr()), wire_ptr);
    assert_eq!(free_vertices, expected_free);
    assert_eq!(free_vertices.first().map(|id| id.as_str().as_ptr()), free_ptr);
    if form == 1 {
        assert_eq!(ir.model.points.len(), 1);
        assert_eq!(ir.model.points[0].position().get(), Point3::new(2.0, 3.0, 0.0));
        assert_eq!(free_vertices, [ir.model.vertices[0].id.clone()]);
    } else if form == 11 {
        assert_eq!(ir.model.points.len(), 2);
        assert_eq!(ir.model.points[0].position().get(), Point3::new(0.0, 0.0, 0.0));
        assert_eq!(ir.model.points[1].position().get(), Point3::new(1.0, 0.0, 0.0));
        assert_eq!(wire_edges, [ir.model.edges[0].id.clone()]);
    } else {
        assert!(ir.model.points.is_empty());
        assert_eq!(losses.len(), 1);
        assert_eq!(losses[0].message,
            "IGES entity type 106 form 12 was not projected: tuple count is outside 1..=1000000");
    }
    // Only the target's decoded node survives as materialized backing.
    let allowance = policy.limits.max_materialized_bytes.min(EMPTY_INPUT_MATERIALIZED_ALLOWANCE);
    let remaining = allowance
        - if has_decoded { u64::try_from(decoded_node_bytes()).unwrap() } else { 0 };
    let released = ctx.reserve_scoped(remaining, "test merged copious source backing released").unwrap();
    drop(released);
    drop(decoded);
    drop(decoded_storage);
    let released = ctx.reserve_scoped(allowance,
        "test merged copious target backing released").unwrap();
    drop(released);
    ctx.finish_session().unwrap();
}

#[test]
fn copious_loss_merge_releases_source_slots_and_preserves_retained_payload() {
    assert_merge_release(12, LOSS_VALUES);
}

#[test]
fn copious_free_vertex_merge_releases_source_slots_and_preserves_identities() {
    assert_merge_release(1, POINT_VALUES);
}

#[test]
fn copious_wire_merge_releases_source_slots_and_preserves_identities() {
    assert_merge_release(11, WIRE_VALUES);
}

#[test]
fn copious_loss_slot_materialization_refuses_before_payload_construction() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut ir = CadIr::empty();
    let result = project_fixture(&ctx, &mut ir, 12, LOSS_VALUES);
    let first = match result.as_ref() {
        Err(CodecError::ResourceLimit(first)) => *first,
        _ => panic!("expected copious loss-slot materialization refusal"),
    };
    drop(result);
    assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(first.operation, "iges entity loss slots");
    assert_eq!((first.limit, first.used, first.additional),
        (0, 0, u64::try_from(4 * size_of::<LossNote>()).unwrap()));
    assert!(ir.model.points.is_empty());
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn copious_decoded_target_node_refuses_materialization_before_insertion() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let node = u64::try_from(decoded_node_bytes()).unwrap();
    policy.limits.max_materialized_bytes = 2 * node - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut source_storage = ctx.reserve_scoped(0, "test copious decoded source nodes").unwrap();
    let mut source = BTreeSet::new();
    for sequence in [1, 3, 5] {
        ctx.insert_scoped_btree_set(&mut source_storage, &mut source, sequence,
            "test copious decoded source nodes", "test copious decoded source nodes").unwrap();
    }
    let outcome = CopiousProjectionOutcome {
        decoded: source, decoded_storage: source_storage,
        losses: Vec::new(), loss_slots_storage: ctx.reserve_scoped(0, "test copious loss slots").unwrap(),
        wire_edges: Vec::new(), wire_slots_storage: ctx.reserve_scoped(0, "test copious wire slots").unwrap(),
        free_vertices: Vec::new(), free_vertex_slots_storage: ctx.reserve_scoped(0, "test copious vertex slots").unwrap(),
    };
    let mut decoded_storage = ctx.reserve_scoped(0, "test merged copious decoded nodes").unwrap();
    let mut decoded = BTreeSet::new();
    let Err(CodecError::ResourceLimit(first)) = outcome.merge_into(&mut decoded, &mut decoded_storage,
        &mut Vec::new(), &mut Vec::new(), &mut Vec::new(), &ctx) else {
        panic!("expected overlapping decoded target-node refusal");
    };
    assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(first.operation, "iges merged decoded sequences");
    assert_eq!((first.limit, first.used, first.additional), (2 * node - 1, node, node));
    assert!(decoded.is_empty());
    drop(decoded);
    drop(decoded_storage);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}
