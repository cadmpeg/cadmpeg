// SPDX-License-Identifier: Apache-2.0

use super::super::{project, CopiousProjectionOutcome};
use super::super::super::geometry::SourceSequences;
use crate::parameter::{ParameterRecord, Token, TokenValue};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::ids::{PointId, VertexId};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::{Point, Vertex};
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};
use std::mem::{align_of, size_of};

fn node_bytes() -> usize {
    let alignment = std::mem::align_of::<u32>()
        .max(std::mem::align_of::<usize>());
    11 * std::mem::size_of::<u32>()
        + 16 * std::mem::size_of::<usize>() + 2 * alignment
}

fn outcome<'ctx>(ctx: &'ctx DecodeContext<'_>, decoded: BTreeSet<u32>) -> CopiousProjectionOutcome<'ctx> {
    // These are prebuilt unit-test inputs. Hold their container backing in
    // the caller's session without charging decode traversal or item creation.
    let nodes = if decoded.is_empty() { 0 } else { (decoded.len() - 1) / 5 + 1 };
    let decoded_storage = ctx.reserve_scoped(u64::try_from(nodes * node_bytes()).unwrap(),
        "test copious decoded input backing").unwrap();
    CopiousProjectionOutcome {
        decoded, decoded_storage,
        losses: Vec::new(),
        loss_slots_storage: ctx.reserve_scoped(0, "test copious loss input backing").unwrap(),
        wire_edges: Vec::new(),
        wire_slots_storage: ctx.reserve_scoped(0, "test copious wire input backing").unwrap(),
        free_vertices: Vec::new(),
        free_vertex_slots_storage: ctx.reserve_scoped(0, "test copious free-vertex input backing").unwrap(),
    }
}

fn boundary(work: u64, additional: u64, operation: &'static str) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let initial = outcome(&ctx, BTreeSet::from([1, 3, 5]));
    let replays = [outcome(&ctx, BTreeSet::from([1, 3, 5])), outcome(&ctx, BTreeSet::new())];
    let mut decoded_storage = ctx.reserve_scoped(0, "test merged decoded storage").unwrap();
    let mut decoded = BTreeSet::new();
    let first = match initial.merge_into(
        &mut decoded, &mut decoded_storage, &mut Vec::new(), &mut Vec::new(), &mut Vec::new(), &ctx,
    ) {
        Err(CodecError::ResourceLimit(first)) => first,
        other => panic!("expected actual merged source/allocation refusal: {other:?}"),
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, operation);
    assert_eq!((first.limit, first.used, first.additional), (work, work, additional));
    assert!(decoded.is_empty());
    for replay in replays {
        assert!(matches!(replay.merge_into(
            &mut decoded, &mut decoded_storage, &mut Vec::new(), &mut Vec::new(), &mut Vec::new(), &ctx),
            Err(CodecError::ResourceLimit(last)) if last == first));
    }
    drop(decoded);
    drop(decoded_storage);
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn copious_merge_source_refuses_one_visit_before_any_decoded_insertion() {
    boundary(0, 1, "iges copious merged sequences");
}

#[test]
fn copious_merge_node_work_refuses_after_one_visit_without_admitting_the_tail() {
    // An empty u32 set adds one node: three admitted node passes.
    boundary(1, u64::try_from(3 * node_bytes()).unwrap(), "iges merged decoded sequences");
}

#[test]
fn empty_copious_merge_executes_no_source_steps() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut decoded_storage = ctx.reserve_scoped(0, "test merged decoded storage").unwrap();
    outcome(&ctx, BTreeSet::new()).merge_into(
        &mut BTreeSet::new(), &mut decoded_storage, &mut Vec::new(), &mut Vec::new(), &mut Vec::new(), &ctx,
    ).unwrap();
    drop(decoded_storage);
    ctx.finish_session().unwrap();
}

fn duplicate_lookup_work(count: usize) -> u64 {
    // One root key needs one comparison. A 64-key B-tree has at most two
    // levels; each admits at most eleven comparisons of a four-byte u32.
    let comparisons = match count { 1 => 1, 64 => 22, _ => panic!("unsupported fixture size") };
    comparisons * u64::try_from(std::mem::size_of::<u32>()).unwrap()
}

fn duplicate_source_boundary(count: usize, visited: usize, before_lookup: bool) {
    let keys: BTreeSet<_> = (1..=u32::try_from(count).unwrap()).collect();
    let before = keys.clone();
    let lookup = duplicate_lookup_work(count);
    let work = u64::try_from(visited).unwrap() * (1 + lookup) + u64::from(before_lookup);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_collection_items = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let initial = outcome(&ctx, keys.clone());
    // Construct owned replay inputs before fusing the session. Their backing
    // uses the existing fixture reservation; they perform no measured visits.
    let replays: Vec<_> = (0..64).flat_map(|_| [outcome(&ctx, keys.clone()),
        outcome(&ctx, BTreeSet::new())]).collect();
    let mut decoded = keys;
    let mut decoded_storage = ctx.reserve_scoped(0, "test trusted target set").unwrap();
    let mut losses = Vec::new();
    let mut edges = Vec::new();
    let mut vertices = Vec::new();
    let first = match initial.merge_into(&mut decoded, &mut decoded_storage,
        &mut losses, &mut edges, &mut vertices, &ctx) {
        Err(CodecError::ResourceLimit(first)) => first,
        other => panic!("expected duplicate source work refusal: {other:?}"),
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, if before_lookup { "iges merged decoded sequences" }
        else { "iges copious merged sequences" });
    assert_eq!((first.limit, first.used, first.additional),
        (work, work, if before_lookup { lookup } else { 1 }));
    assert_eq!(decoded, before);
    for replay in replays {
        assert!(matches!(replay.merge_into(&mut decoded, &mut decoded_storage,
            &mut losses, &mut edges, &mut vertices, &ctx),
            Err(CodecError::ResourceLimit(last)) if last == first));
        assert_eq!(decoded, before);
    }
    assert!(losses.is_empty() && edges.is_empty() && vertices.is_empty());
    drop(decoded);
    drop(decoded_storage);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn copious_duplicate_merge_refuses_first_and_last_actual_source_visits() {
    for count in [1, 64] {
        for visited in [0, count - 1] {
            duplicate_source_boundary(count, visited, false);
        }
    }
}

#[test]
fn copious_duplicate_merge_refuses_first_and_last_existing_key_lookups() {
    for count in [1, 64] {
        for visited in [0, count - 1] {
            duplicate_source_boundary(count, visited, true);
        }
    }
}

#[test]
fn copious_duplicate_merge_accepts_exact_whole_source_without_new_storage() {
    for count in [1, 64] {
        let mut decoded: BTreeSet<_> = (1..=u32::try_from(count).unwrap()).collect();
        let before = decoded.clone();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::try_from(count).unwrap() * (1 + duplicate_lookup_work(count));
        policy.limits.max_collection_items = 0;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let source = outcome(&ctx, decoded.clone());
        let mut decoded_storage = ctx.reserve_scoped(0, "test trusted target set").unwrap();
        source.merge_into(&mut decoded, &mut decoded_storage,
            &mut Vec::new(), &mut Vec::new(), &mut Vec::new(), &ctx).unwrap();
        assert_eq!(decoded, before);
        drop(decoded);
        drop(decoded_storage);
        // The source iterator and its reservation both ended. The target
        // was a trusted prebuilt input, so no measured backing remains.
        let allowance = 16 * 1024 * 1024;
        let free = ctx.reserve_scoped(allowance, "test duplicate source backing released").unwrap();
        drop(free);
        ctx.finish_session().unwrap();
    }
}

fn point_source_work(count: usize, completed: usize) -> u64 {
    if count == 0 {
        return 0;
    }
    let sequence_node = 11 * (size_of::<PointId>() + size_of::<u32>())
        + 16 * size_of::<usize>()
        + 2 * align_of::<PointId>().max(align_of::<u32>()).max(align_of::<usize>());
    // Directory visit1, parameter lookup4, tuple visits, and position Vec
    // moves at capacities4/8/16/32. There are no exhausted iterator reads.
    let mut work = 5 + count;
    for capacity in [4, 8, 16, 32] {
        if capacity < count {
            work += capacity * size_of::<FinitePoint3>();
        }
    }
    for existing in 0..completed {
        let point_bytes = format!("iges:model:point#D1-{}", existing + 1).len();
        let vertex_bytes = format!("iges:model:vertex#D1-{}", existing + 1).len();
        // The fixture has at most64keys. A second level can first exist at
        // eleven keys; lookup is bounded by min(keycount,11*height).
        let comparisons = if existing < 11 { existing } else { existing.min(22) };
        let node_added = existing == 0 || existing % 5 == 0;
        // One source visit, two formatting passes per ID, two point copies
        // and one vertex copy. Source index get_mut, contains and insert
        // each compare the new key. Node work is one shift plus two passes
        // when the insertion raises the node bound (n-1)/5+1.
        work += 1 + 4 * point_bytes + 3 * vertex_bytes
            + 3 * point_bytes * comparisons
            + sequence_node * (1 + 2 * usize::from(node_added));
        if existing >= 4 && existing.is_power_of_two() {
            work += existing * (size_of::<Point>() + size_of::<Vertex>() + size_of::<VertexId>());
        }
    }
    u64::try_from(work).unwrap()
}

fn point_projection_source_boundary(form: i64, count: usize, completed: usize, exact: bool, placed: bool) {
    let bytes = if form >= 11 {
        crate::test_support::test_owned::owned_test_file_with_global(&[],
            b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;")
    } else {
        crate::test_support::test_owned::owned_test_file(&[])
    };
    let global = crate::test_support::with_service_context(&bytes, |setup| {
        let scan = crate::card::scan_with_context(&bytes, setup).unwrap();
        let (global, _, _) = crate::global::parse(&scan, setup).unwrap();
        global.length_context().unwrap()
    });
    assert_eq!(global.length_factor_mm(), 1.0);
    if form >= 11 {
        assert_eq!(global.global_table(), crate::global::GlobalTable::V4_0);
        assert!(count <= 1);
    }
    let mut entry = crate::test_support::directory_target(1, 106);
    entry.form = form;
    if placed {
        entry.transform = 3;
    }
    let directory = [entry];
    let transform_entry = crate::test_support::directory_target(3, 124);
    let mut entries = BTreeMap::from([(1, &directory[0])]);
    if placed {
        entries.insert(3, &transform_entry);
    }
    let interpretation = if form >= 11 { form - 10 } else { form };
    let mut values = vec![106, interpretation, i64::try_from(count).unwrap()];
    if interpretation == 1 {
        values.push(0);
    }
    let mut expected = CadIr::empty();
    for index in 0..count {
        let x = i64::try_from(index).unwrap();
        match interpretation {
            1 => values.extend([x, 0]),
            2 => values.extend([x, 0, 0]),
            3 => values.extend([x, 0, 0, 0, 0, 1]),
            _ => panic!("unsupported point fixture form"),
        }
        let point = PointId::mint(format!("iges:model:point#D1-{}", index + 1)).unwrap();
        let x = f64::from(u32::try_from(index).unwrap());
        let position = if placed { Point3::new(x + 2.0, 3.0, 4.0) }
            else { Point3::new(x, 0.0, 0.0) };
        expected.model.points.push(Point::new(point.clone(),
            FinitePoint3::new(position).unwrap(), None));
        expected.model.vertices.push(Vertex {
            id: VertexId::mint(format!("iges:model:vertex#D1-{}", index + 1)).unwrap(),
            point, tolerance: None,
        });
    }
    expected.model.points.truncate(completed);
    expected.model.vertices.truncate(completed);
    let expected_free: Vec<_> = expected.model.vertices.iter().map(|vertex| vertex.id.clone()).collect();
    let record = ParameterRecord::from_test_tokens(1, 1..2, Vec::new(), values.len(),
        values.into_iter().map(|value| Token {
            value: TokenValue::Integer(value), span: 0..0,
        }).collect(), Vec::new());
    let transform_values = [124, 1, 0, 0, 2, 0, 1, 0, 3, 0, 0, 1, 4];
    let transform_record = ParameterRecord::from_test_tokens(3, 1..2, Vec::new(), transform_values.len(),
        transform_values.into_iter().map(|value| Token {
            value: TokenValue::Integer(value), span: 0..0,
        }).collect(), Vec::new());
    let mut records = BTreeMap::from([(1, &record)]);
    if placed {
        records.insert(3, &transform_record);
    }
    // Two-key maps add4 to the initial parameter lookup and8 per transform
    // lookup. The new path node needs3passes; removal needs1pass and4bytes
    // of key comparison. Matrix checks and composition have fixed size.
    let placement_work = if placed && count != 0 {
        u64::try_from(4 + 8 + 8 + 4 * node_bytes() + 4).unwrap()
    } else { 0 };
    let cap = point_source_work(count, completed)
        + placement_work
        + if exact && count != 0 { u64::try_from(3 * node_bytes()).unwrap() } else { 0 };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = cap;
    // One position, sequence entry, point, vertex and free-vertex slot per
    // tuple, followed by one decoded sequence. Two entities per tuple.
    policy.limits.max_collection_items = if count == 0 { 0 } else { u64::try_from(5 * count + 1).unwrap() };
    if placed && count != 0 {
        policy.limits.max_collection_items += 1;
    }
    policy.limits.max_entities = u64::try_from(2 * count).unwrap();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_recursion_depth = u64::from(placed && count != 0);
    if count == 0 {
        policy.limits.max_materialized_bytes = 0;
    }
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut output = ctx.reserve_scoped(0, "test copious point source output").unwrap();
    let mut sequences = SourceSequences::new(&ctx).unwrap();
    let mut ir = CadIr::empty();
    let source = if count == 0 { &[][..] } else { &directory[..] };
    let result = output.with_storage(||
        project(&mut ir, source, &entries, &records, &global, &ctx, &mut sequences));
    if exact {
        let outcome = result.unwrap();
        assert_eq!(ir, expected);
        assert_eq!(outcome.free_vertices, expected_free);
        assert_eq!(outcome.decoded, if count == 0 { BTreeSet::new() } else { BTreeSet::from([1]) });
        assert!(outcome.losses.is_empty() && outcome.wire_edges.is_empty());
        drop(outcome);
        drop(ir);
        drop(sequences);
        drop(output);
        let allowance = policy.limits.max_materialized_bytes.min(16 * 1024 * 1024);
        let released = ctx.reserve_scoped(allowance, "test copious point source released").unwrap();
        drop(released);
        let Err(CodecError::ResourceLimit(first)) = ctx.charge_work(1, "test copious exact point work") else {
            panic!("expected exact completed point work");
        };
        assert_eq!(first.dimension, ResourceDimension::WorkUnits);
        assert_eq!((first.limit, first.used, first.additional), (cap, cap, 1));
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
    } else {
        let first = match result.as_ref() {
            Err(CodecError::ResourceLimit(first)) => *first,
            _ => panic!("expected actual point source refusal"),
        };
        drop(result);
        assert_eq!(first.dimension, ResourceDimension::WorkUnits);
        assert_eq!(first.operation, "iges copious point projection");
        assert_eq!((first.limit, first.used, first.additional), (cap, cap, 1));
        assert_eq!(ir, expected);
        for _ in 0..64 {
            for source in [&directory[..], &[][..]] {
                assert!(matches!(project(&mut ir, source, &entries, &records, &global,
                    &ctx, &mut sequences), Err(CodecError::ResourceLimit(last)) if last == first));
                assert_eq!(ir, expected);
            }
        }
        drop(ir);
        drop(sequences);
        drop(output);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
    }
}

#[test]
fn copious_point_sources_refuse_first_actual_position_visit() {
    for form in [1, 2, 3] {
        for count in [1, 64] {
            point_projection_source_boundary(form, count, 0, false, false);
        }
    }
}

#[test]
fn copious_point_sources_refuse_last_actual_position_visit() {
    for form in [1, 2, 3] {
        for count in [1, 64] {
            point_projection_source_boundary(form, count, count - 1, false, false);
        }
    }
}

#[test]
fn copious_point_sources_accept_exact_whole_work_without_an_exhausted_read() {
    for form in [1, 2, 3] {
        for count in [0, 1, 64] {
            point_projection_source_boundary(form, count, count, true, false);
        }
    }
}

#[test]
fn copious_v4_path_sources_refuse_the_only_point_visit() {
    for form in [11, 12, 13] {
        point_projection_source_boundary(form, 1, 0, false, false);
    }
}

#[test]
fn copious_v4_path_sources_accept_exact_whole_point_work() {
    for form in [11, 12, 13] {
        point_projection_source_boundary(form, 1, 1, true, false);
    }
}

#[test]
fn copious_placed_point_sources_refuse_first_actual_position_visit() {
    for form in [1, 2, 3, 11, 12, 13] {
        for &count in if form >= 11 { &[1][..] } else { &[1, 64][..] } {
            point_projection_source_boundary(form, count, 0, false, true);
        }
    }
}

#[test]
fn copious_placed_point_sources_refuse_last_actual_position_visit() {
    for form in [1, 2, 3, 11, 12, 13] {
        for &count in if form >= 11 { &[1][..] } else { &[1, 64][..] } {
            point_projection_source_boundary(form, count, count - 1, false, true);
        }
    }
}

#[test]
fn copious_placed_point_sources_accept_exact_whole_work() {
    for form in [1, 2, 3, 11, 12, 13] {
        for &count in if form >= 11 { &[0, 1][..] } else { &[0, 1, 64][..] } {
            point_projection_source_boundary(form, count, count, true, true);
        }
    }
}
