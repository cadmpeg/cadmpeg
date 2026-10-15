// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;
use std::mem::{align_of, size_of};

use cadmpeg_core::decode::{u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{Surface, SurfaceGeometry};
use cadmpeg_ir::geometry::nurbs::WeightedPole3;
use cadmpeg_ir::ids::{CurveId, PointId, SurfaceId, VertexId};
use cadmpeg_ir::CadIr;

use crate::entities::geometry::SourceSequences;
use crate::test_support::test_surface_fixtures::{ruled_surface_file, surface_of_revolution_file, tabulated_cylinder_file};
use crate::IgesCodec;

fn node<K, V>() -> usize {
    11 * (size_of::<K>() + size_of::<V>()) + 16 * size_of::<usize>()
        + 2 * align_of::<K>().max(align_of::<V>()).max(align_of::<usize>())
}

fn index_bytes(ir: &CadIr) -> usize {
    // Each fixture has at most five distinct keys in each index, so each
    // admitted tree has one node. Every curve has one indexed edge. Its
    // vector starts at four slots; each slot stores two IDs and an interval.
    assert!((1..=5).contains(&ir.model.curves.len()));
    assert_eq!(ir.model.edges.len(), ir.model.curves.len());
    assert!((1..=5).contains(&ir.model.points.len()));
    assert_eq!(ir.model.vertices.len(), ir.model.points.len());
    let strings = ir.model.curves.iter().map(|curve| curve.id.as_str().len()).sum::<usize>()
        + ir.model.edges.iter().map(|edge| edge.curve().unwrap().as_str().len()
            + edge.start.as_str().len() + edge.end.as_str().len()).sum::<usize>()
        + ir.model.points.iter().map(|point| point.id.as_str().len()).sum::<usize>()
        + ir.model.vertices.iter().map(|vertex| vertex.id.as_str().len()).sum::<usize>();
    node::<CurveId, usize>() + node::<CurveId, Vec<()>>()
        + node::<PointId, FinitePoint3>() + node::<VertexId, FinitePoint3>()
        + 4 * ir.model.edges.len() * (2 * size_of::<VertexId>() + size_of::<Option<[f64; 2]>>())
        + strings
}

fn boundary(bytes: &[u8], operation: &'static str, revolution: bool, directrix: bool) {
    let (directory, parameters, global) = crate::test_support::with_service_context(bytes, |ctx| {
        let scan = crate::card::scan_with_context(bytes, ctx).unwrap();
        let (global, _, _) = crate::global::parse(&scan, ctx).unwrap();
        let (directory, quarantined) = crate::directory::parse(&scan, global.global_table(), ctx).unwrap();
        assert!(quarantined.is_empty());
        let parameters = crate::parameter::assemble_with_context(&scan, &directory, &quarantined, &global, ctx).unwrap().records;
        (directory, parameters, global.length_context().unwrap())
    });
    let (mut fixture, _, _) = IgesCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default()).unwrap().into_parts();
    let mut prototype = fixture.model.surfaces.first().unwrap().clone();
    prototype.geometry = SurfaceGeometry::Solved(prototype.geometry.solved().unwrap().clone());
    fixture.model.surfaces.clear();
    fixture.model.procedural_surfaces.clear();
    // Existing output slots are fixture setup outside the measured session.
    // Growth is the next allocation after construction and sequence storage.
    fixture.model.surfaces.reserve_exact(64);
    assert_eq!(fixture.model.surfaces.capacity(), 64);
    for index in 0..64 {
        let mut surface = prototype.clone();
        surface.id = SurfaceId::mint(format!("test:model:surface#seed-{index}")).unwrap();
        fixture.model.surfaces.push(surface);
    }
    let sequence = directory.last().unwrap().sequence;
    let surface_id = format!("iges:model:surface#D{sequence}");
    let grid = if revolution {
        2 * size_of::<Vec<WeightedPole3<FinitePoint3>>>() + 6 * size_of::<WeightedPole3<FinitePoint3>>() + (4 + 6) * size_of::<f64>()
    } else {
        2 * size_of::<Vec<FinitePoint3>>() + 4 * size_of::<FinitePoint3>() + 8 * size_of::<f64>()
    };
    // Two borrowed u32 index trees, the composite index, surviving grid,
    // surface factory/key and source-sequence node. Tabulated/revolution
    // also keep their generated directrix identity for the construction.
    let prefix = u64_from_index(2 * node::<u32, &crate::parameter::ParameterRecord>()
        + index_bytes(&fixture) + grid + 2 * surface_id.len()
        + node::<SurfaceId, u32>() + usize::from(directrix) * "iges:model:curve#D1".len());
    let growth = u64_from_index(64 * size_of::<Surface>());
    for exact_growth in [false, true] {
        let cap = prefix + growth - u64::from(!exact_growth);
        let mut ir = fixture.clone();
        assert_eq!(ir.model.surfaces.capacity(), 64);
        let before = ir.clone();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = cap;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut sequences = SourceSequences::new(&ctx).unwrap();
        let mut output = ctx.reserve_scoped(0, "test surface output").unwrap();
        let error = output.with_storage(|| super::super::project(&mut ir, &directory, &parameters, &global, &ctx, &mut sequences).map(drop)).unwrap_err();
        let CodecError::ResourceLimit(first) = error else { panic!("expected phase refusal"); };
        assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!(first.operation, operation);
        // Exact delta admission reaches the old-buffer overlap reservation.
        let used = prefix + if exact_growth { growth } else { 0 };
        assert_eq!((first.limit, first.used, first.additional), (cap, used, growth));
        assert_eq!(ir, before);
        for _ in 0..64 {
            for entries in [directory.as_slice(), &[]] {
                assert!(matches!(super::super::project(&mut ir, entries, &parameters, &global, &ctx, &mut sequences), Err(CodecError::ResourceLimit(last)) if last == first));
                assert_eq!(ir, before);
            }
        }
        drop(ir);
        drop(sequences);
        drop(output);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
    }
}

#[test]
fn ruled_carriers_end_before_surface_slot_growth() {
    boundary(&ruled_surface_file(), "iges ruled neutral surface slots", false, false);
}

#[test]
fn tabulated_carrier_ends_before_surface_slot_growth() {
    boundary(&tabulated_cylinder_file(), "iges tabulated neutral surface slots", false, true);
}

#[test]
fn revolution_axis_and_carrier_end_before_surface_slot_growth() {
    boundary(&surface_of_revolution_file(), "iges revolution neutral surface slots", true, true);
}
