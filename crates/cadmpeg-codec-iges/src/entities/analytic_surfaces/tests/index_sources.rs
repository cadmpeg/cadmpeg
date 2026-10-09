// SPDX-License-Identifier: Apache-2.0

use super::*;
use super::super::project;
use crate::directory::DirectoryEntry;
use crate::entities::geometry::SourceSequences;
use crate::global::ProjectedGlobal;
use crate::parameter::ParameterRecord;
use cadmpeg_core::decode::{u64_from_index, DecodeArena, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::CadIr;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::ids::PointId;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::Point;

fn inputs() -> (DirectoryEntry, ParameterRecord, ProjectedGlobal) {
    let bytes = pointer_defined_surface_file(196, 0);
    crate::test_support::with_service_context(&bytes, |ctx| {
        let scan = crate::card::scan_with_context(&bytes, ctx).unwrap();
        let (global, _, _storage) = crate::global::parse(&scan, ctx).unwrap();
        let (directory, quarantined) = crate::directory::parse(&scan, global.global_table(), ctx).unwrap();
        assert!(quarantined.is_empty());
        let parameters = crate::parameter::assemble_with_context(
            &scan, &directory, &quarantined, &global, ctx,
        ).unwrap().records;
        let entry = directory.into_iter().find(|entry| entry.entity_type == 196).unwrap();
        let record = parameters.into_iter().find(|record| record.directory_sequence == entry.sequence).unwrap();
        assert_eq!(entry.transform, 0);
        (entry, record, global.length_context().unwrap())
    })
}

fn node_bytes<K, V>() -> u64 {
    let alignment = std::mem::align_of::<K>()
        .max(std::mem::align_of::<V>()).max(std::mem::align_of::<usize>());
    u64_from_index(11 * (std::mem::size_of::<K>() + std::mem::size_of::<V>())
        + 16 * std::mem::size_of::<usize>() + 2 * alignment)
}

fn comparisons(count: usize) -> u64 {
    if count == 0 {
        0
    } else {
        let height = if count == 1 { 1 } else { ((count + 1) / 2).ilog(6) + 1 };
        u64_from_index(count).min(11 * u64::from(height))
    }
}

fn insertion_work(count: usize, node: u64) -> u64 {
    // The node bound increases at the first entry and every five later keys.
    let increase = u64::from(count % 5 == 0);
    node * (1 + 2 * increase)
}

fn sequence_index_work(count: usize) -> u64 {
    // Directory and Parameter indexes both store a u32 and one borrowed pointer.
    let node = node_bytes::<u32, &ParameterRecord>();
    (0..count).map(|index| 1 + 2 * u64_from_index(std::mem::size_of::<u32>())
        * comparisons(index) + insertion_work(index, node)).sum()
}

fn refusal(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    parameters: &[ParameterRecord],
    global: &ProjectedGlobal,
    work: u64,
    operation: &'static str,
    additional: u64,
) {
    let expected = ir.model.clone();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut sequences = SourceSequences::new(&ctx).unwrap();
    let first = match project(ir, directory, parameters, global, &ctx, &mut sequences) {
        Err(CodecError::ResourceLimit(first)) => first,
        _ => panic!("expected actual analytic index refusal"),
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, operation);
    assert_eq!((first.used, first.additional, first.limit), (work, additional, work));
    assert_eq!(ir.model, expected);
    assert!(matches!(project(ir, directory, parameters, global, &ctx, &mut sequences),
        Err(CodecError::ResourceLimit(last)) if last == first));
    assert!(matches!(project(&mut CadIr::empty(), &[], &[], global, &ctx, &mut sequences),
        Err(CodecError::ResourceLimit(last)) if last == first));
    drop(sequences);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn analytic_parameter_index_first_last_and_exact_completion_work() {
    let (_, record, global) = inputs();
    for count in [1, 64] {
        let parameters: Vec<_> = (0..count).map(|index| {
            let mut item = record.clone();
            item.directory_sequence = u32::try_from(2 * index + 1).unwrap();
            item
        }).collect();
        for visited in [0, count - 1] {
            refusal(&mut CadIr::empty(), &[], &parameters, &global,
                sequence_index_work(visited), "iges analytic-surface parameter index traversal", 1);
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = sequence_index_work(count);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ir = CadIr::empty();
        let mut sequences = SourceSequences::new(&ctx).unwrap();
        let result = project(&mut ir, &[], &parameters, &global, &ctx, &mut sequences).unwrap();
        assert!(result.decoded.is_empty());
        assert!(result.losses.is_empty());
        assert_eq!(ir.model, CadIr::empty().model);
        drop(result);
        drop(sequences);
        ctx.finish_session().unwrap();
    }
}

#[test]
fn analytic_directory_index_first_last_and_exact_ignored_pass_work() {
    let (_, _, global) = inputs();
    for count in [1, 64] {
        let directory: Vec<_> = (0..count).map(|index| {
            crate::test_support::directory_target(u32::try_from(2 * index + 1).unwrap(), 116)
        }).collect();
        for visited in [0, count - 1] {
            refusal(&mut CadIr::empty(), &directory, &[], &global,
                sequence_index_work(visited), "iges analytic-surface directory index traversal", 1);
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // The second pass visits every unsupported Directory entry once.
        policy.limits.max_work_units = sequence_index_work(count) + u64_from_index(count);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ir = CadIr::empty();
        let mut sequences = SourceSequences::new(&ctx).unwrap();
        let result = project(&mut ir, &directory, &[], &global, &ctx, &mut sequences).unwrap();
        assert!(result.decoded.is_empty());
        assert!(result.losses.is_empty());
        assert_eq!(ir.model, CadIr::empty().model);
        drop(result);
        drop(sequences);
        ctx.finish_session().unwrap();
    }
}

fn point_inputs(count: usize) -> CadIr {
    let mut ir = CadIr::empty();
    for index in 0..count {
        ir.model.points.push(Point::new(
            PointId::mint(format!("iges:model:point#D{}", 2 * index + 1)).unwrap(),
            FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).unwrap(), None,
        ));
    }
    ir
}

fn point_index_work(ir: &CadIr, count: usize) -> u64 {
    let node = node_bytes::<&str, Point3>();
    ir.model.points[..count].iter().enumerate().map(|(index, point)| {
        // One initial contains query, one insert admission query, one insert.
        1 + 3 * u64_from_index(point.id.as_str().len()) * comparisons(index)
            + insertion_work(index, node)
    }).sum()
}

fn point_prelude() -> u64 {
    // Two singleton indexes, one Directory visit, singleton binary search:
    // one partition visit and final compare, each reading two u32 keys.
    2 * sequence_index_work(1) + 1 + 2 * (1 + 2 * u64_from_index(std::mem::size_of::<u32>()))
}

#[test]
fn analytic_point_index_first_last_and_terminal_work_preserve_the_model() {
    let (entry, record, global) = inputs();
    for count in [1, 64] {
        let mut ir = point_inputs(count);
        for visited in [0, count - 1] {
            let work = point_prelude() + point_index_work(&ir, visited);
            refusal(&mut ir, std::slice::from_ref(&entry), std::slice::from_ref(&record),
                &global, work, "iges analytic point index traversal", 1);
        }
        let work = point_prelude() + point_index_work(&ir, count);
        // Exhaustion goes straight to the real location query, with no source probe.
        let additional = u64_from_index("iges:model:point#D1".len()) * comparisons(count);
        refusal(&mut ir, std::slice::from_ref(&entry), std::slice::from_ref(&record),
            &global, work, "iges analytic location lookup", additional);
    }
}

#[test]
fn analytic_point_index_keeps_the_first_location_identity_and_other_points() {
    let (entry, record, global) = inputs();
    for count in [1, 64] {
        let mut ir = point_inputs(count);
        ir.model.points.push(Point::new(
            PointId::mint("iges:model:point#D1").unwrap(),
            FinitePoint3::new(Point3::new(9.0, 8.0, 7.0)).unwrap(), None,
        ));
        let expected = ir.model.points.clone();
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut sequences = SourceSequences::new(&ctx).unwrap();
        let result = project(&mut ir, std::slice::from_ref(&entry),
            std::slice::from_ref(&record), &global, &ctx, &mut sequences).unwrap();
        assert_eq!(ir.model.points, expected);
        assert_eq!(ir.model.surfaces.len(), 1);
        assert_eq!(ir.model.surfaces[0].id.as_str(), "iges:model:surface#D3");
        let Some(SolvedSurfaceGeometry::Sphere(sphere)) = ir.model.surfaces[0].geometry.solved() else {
            panic!("expected a sphere at the first location identity");
        };
        assert_eq!(sphere.center().get(), Point3::new(1.0, 2.0, 3.0));
        assert_eq!(sphere.radius().get(), 2.0);
        assert_eq!(result.decoded, std::collections::BTreeSet::from([entry.sequence]));
        assert!(result.losses.is_empty());
        drop(result);
        drop(sequences);
        ctx.finish_session().unwrap();
    }
}

#[test]
fn empty_analytic_indexes_are_free_in_a_fresh_zero_budget_session() {
    let (_, _, global) = inputs();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_entities = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut ir = CadIr::empty();
    let mut sequences = SourceSequences::new(&ctx).unwrap();
    let result = project(&mut ir, &[], &[], &global, &ctx, &mut sequences).unwrap();
    assert!(result.decoded.is_empty());
    assert!(result.losses.is_empty());
    assert_eq!(ir.model, CadIr::empty().model);
    drop(result);
    drop(sequences);
    ctx.finish_session().unwrap();
}
