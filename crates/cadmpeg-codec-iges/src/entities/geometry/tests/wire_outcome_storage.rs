// SPDX-License-Identifier: Apache-2.0

use super::super::{SourceSequences, WireProjectionOutcome};
use crate::parameter::{ParameterRecord, Token, TokenValue};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::EdgeId;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};
use std::mem::{align_of, size_of};

// Core bounds materialization by min(policy ceiling, base + 1000 * input bytes).
const EMPTY_INPUT_MATERIALIZED_ALLOWANCE: u64 = 16 * 1024 * 1024;

fn node_bytes() -> u64 {
    u64::try_from(11 * size_of::<u32>() + 16 * size_of::<usize>()
        + 2 * align_of::<u32>().max(align_of::<usize>())).unwrap()
}

fn project_fixture<'ctx>(ctx: &'ctx DecodeContext<'_>, ir: &mut CadIr,
    entity_type: u32, valid_conic: bool) -> Result<WireProjectionOutcome<'ctx>, CodecError> {
    let bytes = crate::test_support::test_owned::owned_test_file(&[]);
    let global = crate::test_support::with_service_context(&bytes, |setup| {
        let scan = crate::card::scan_with_context(&bytes, setup).unwrap();
        let (global, _, _) = crate::global::parse(&scan, setup).unwrap();
        global.length_context().unwrap()
    });
    assert_eq!(global.length_factor_mm(), 1.0);
    let mut entry = crate::test_support::directory_target(1, i64::from(entity_type));
    if valid_conic { entry.form = 1; }
    let directory = [entry];
    let entries = BTreeMap::from([(1, &directory[0])]);
    // Type104 circle x^2+y^2=1, from (1,0) to (0,1), at z=0.
    let values = [104, 1, 0, 1, 0, 0, -1, 0, 1, 0, 0, 1];
    let parameters = if valid_conic {
        vec![ParameterRecord::from_test_tokens(1, 1..2, Vec::new(), values.len(),
            values.into_iter().map(|value| Token {
                value: TokenValue::Integer(value), span: 0..0,
            }).collect(), Vec::new())]
    } else { Vec::new() };
    let records = parameters.iter().map(|record| (record.directory_sequence, record)).collect();
    let mut sequences = SourceSequences::new(ctx)?;
    let result = match entity_type {
        104 => super::super::super::conics::project(ir, &directory, (&entries, &records),
            &global, ctx, &mut sequences),
        102 => super::super::super::composite::project(ir, &directory, (&entries, &records),
            &global, ctx, &mut sequences),
        112 => super::super::super::splines::project(ir, &directory, &parameters,
            &global, ctx, &mut sequences),
        130 => super::super::super::offsets::project(ir, &directory, &parameters,
            &global, ctx, &mut sequences),
        _ => unreachable!(),
    };
    // No later appearance lookup reads this actual source index.
    drop(sequences);
    result
}

fn assert_loss_backing(entity_type: u32) {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut ir = CadIr::empty();
    let outcome = project_fixture(&ctx, &mut ir, entity_type, false).unwrap();
    assert!(outcome.decoded.is_empty());
    assert!(outcome.wire_edges.is_empty());
    assert_eq!(outcome.losses.len(), 1);
    assert_eq!(outcome.losses[0].message, format!(
        "IGES entity type {entity_type} form 0 was not projected: Parameter Data record is missing"));
    let allowance = policy.limits.max_materialized_bytes.min(EMPTY_INPUT_MATERIALIZED_ALLOWANCE);
    let Err(CodecError::ResourceLimit(first)) = ctx.reserve_scoped(
        allowance, "test live wire loss backing") else {
        panic!("expected actual live Wire loss slots");
    };
    assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(first.operation, "test live wire loss backing");
    assert_eq!((first.limit, first.used, first.additional),
        (allowance, u64::try_from(4 * size_of::<LossNote>()).unwrap(), allowance));
    drop(outcome);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn conic_wire_outcome_holds_actual_loss_backing() { assert_loss_backing(104); }
#[test]
fn composite_wire_outcome_holds_actual_loss_backing() { assert_loss_backing(102); }
#[test]
fn spline_wire_outcome_holds_actual_loss_backing() { assert_loss_backing(112); }
#[test]
fn offset_wire_outcome_holds_actual_loss_backing() { assert_loss_backing(130); }

fn assert_merge_release(entity_type: u32, valid_conic: bool) {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut ir = CadIr::empty();
    let outcome = project_fixture(&ctx, &mut ir, entity_type, valid_conic).unwrap();
    let expected_losses = outcome.losses.clone();
    let loss_ptr = outcome.losses.first().map(|value| value.message.as_ptr());
    let expected_wire = outcome.wire_edges.clone();
    let wire_ptr = outcome.wire_edges.first().map(|value| value.as_str().as_ptr());
    let mut storage = ctx.reserve_scoped(0, "test wire merged decoded backing").unwrap();
    let mut decoded = BTreeSet::new();
    let mut losses = Vec::new();
    let mut wires = Vec::new();
    outcome.merge_into(&mut decoded, &mut storage, &mut losses, &mut wires, &ctx).unwrap();
    assert_eq!(losses, expected_losses);
    assert_eq!(losses.first().map(|value| value.message.as_ptr()), loss_ptr);
    assert_eq!(wires, expected_wire);
    assert_eq!(wires.first().map(|value| value.as_str().as_ptr()), wire_ptr);
    if valid_conic {
        assert_eq!(decoded, BTreeSet::from([1]));
        assert!(losses.is_empty());
        assert_eq!(ir.model.curves.len(), 1);
        assert_eq!(ir.model.edges.len(), 1);
        assert_eq!(wires, [ir.model.edges[0].id.clone()]);
        assert_eq!(ir.model.points[0].position().get(), cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0));
        assert_eq!(ir.model.points[1].position().get(), cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0));
    } else {
        assert!(decoded.is_empty());
        assert!(wires.is_empty());
        assert_eq!(losses.len(), 1);
        assert_eq!(losses[0].message, format!(
            "IGES entity type {entity_type} form 0 was not projected: Parameter Data record is missing"));
        assert_eq!(losses[0].provenance.as_ref().and_then(|value| value.tag.as_deref()),
            Some("directory_entry:D1"));
    }
    let allowance = policy.limits.max_materialized_bytes.min(EMPTY_INPUT_MATERIALIZED_ALLOWANCE);
    let source_released = ctx.reserve_scoped(allowance - if valid_conic { node_bytes() } else { 0 },
        "test wire source buffers destroyed").unwrap();
    drop(source_released);
    drop(decoded);
    drop(storage);
    let target_released = ctx.reserve_scoped(allowance, "test wire target nodes destroyed").unwrap();
    drop(target_released);
    ctx.finish_session().unwrap();
}

#[test]
fn conic_wire_merge_releases_loss_slots_and_preserves_payload() { assert_merge_release(104, false); }
#[test]
fn composite_wire_merge_releases_loss_slots_and_preserves_payload() { assert_merge_release(102, false); }
#[test]
fn spline_wire_merge_releases_loss_slots_and_preserves_payload() { assert_merge_release(112, false); }
#[test]
fn offset_wire_merge_releases_loss_slots_and_preserves_payload() { assert_merge_release(130, false); }
#[test]
fn valid_conic_wire_merge_preserves_geometry_and_identity_and_releases_source_backing() {
    assert_merge_release(104, true);
}

#[test]
fn valid_conic_wire_outcome_holds_actual_wire_and_decoded_backing() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut ir = CadIr::empty();
    let outcome = project_fixture(&ctx, &mut ir, 104, true).unwrap();
    assert_eq!(outcome.decoded, BTreeSet::from([1]));
    assert_eq!(outcome.wire_edges.len(), 1);
    assert!(outcome.losses.is_empty());
    let allowance = policy.limits.max_materialized_bytes.min(EMPTY_INPUT_MATERIALIZED_ALLOWANCE);
    let Err(CodecError::ResourceLimit(first)) = ctx.reserve_scoped(
        allowance, "test live conic wire backing") else { panic!("expected live Wire slots and nodes"); };
    assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!((first.limit, first.used, first.additional),
        (allowance, node_bytes() + u64::try_from(4 * size_of::<EdgeId>()).unwrap(), allowance));
    drop(outcome);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}

fn source_outcome<'ctx>(ctx: &'ctx DecodeContext<'_>) -> WireProjectionOutcome<'ctx> {
    WireProjectionOutcome {
        decoded: BTreeSet::from([1, 3, 5]),
        decoded_storage: ctx.reserve_scoped(node_bytes(), "test wire source nodes").unwrap(),
        losses: Vec::new(), loss_slots_storage: ctx.reserve_scoped(0, "test wire source losses").unwrap(),
        wire_edges: Vec::new(), wire_slots_storage: ctx.reserve_scoped(0, "test wire source edges").unwrap(),
    }
}

#[test]
fn wire_merge_target_nodes_refuse_while_source_backing_is_live() {
    let node = node_bytes();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 2 * node - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let source = source_outcome(&ctx);
    let mut storage = ctx.reserve_scoped(0, "test wire target nodes").unwrap();
    let mut decoded = BTreeSet::new();
    let Err(CodecError::ResourceLimit(first)) = source.merge_into(
        &mut decoded, &mut storage, &mut Vec::new(), &mut Vec::new(), &ctx) else {
        panic!("expected overlapping Wire source and target backing refusal");
    };
    assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(first.operation, "iges merged decoded sequences");
    assert_eq!((first.limit, first.used, first.additional), (2 * node - 1, node, node));
    assert!(decoded.is_empty());
    drop(decoded);
    drop(storage);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn wire_merge_accepts_exact_complete_node_work_and_releases_backing() {
    let node = node_bytes();
    // Three visits; node passes3+1+1, and two key lookups at target lengths1/2.
    let work = 3 + 5 * node + 2 * (1 + 2) * u64::try_from(size_of::<u32>()).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_collection_items = 3;
    policy.limits.max_materialized_bytes = 2 * node;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let source = source_outcome(&ctx);
    let mut storage = ctx.reserve_scoped(0, "test wire target nodes").unwrap();
    let mut decoded = BTreeSet::new();
    source.merge_into(&mut decoded, &mut storage, &mut Vec::new(), &mut Vec::new(), &ctx).unwrap();
    assert_eq!(decoded, BTreeSet::from([1, 3, 5]));
    let released = ctx.reserve_scoped(node, "test wire source nodes destroyed").unwrap();
    drop(released);
    drop(decoded);
    drop(storage);
    let released = ctx.reserve_scoped(2 * node, "test wire target nodes destroyed").unwrap();
    drop(released);
    let Err(CodecError::ResourceLimit(first)) = ctx.charge_work(1, "test wire exact work") else {
        panic!("expected exact complete merge work");
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!((first.limit, first.used, first.additional), (work, work, 1));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}
