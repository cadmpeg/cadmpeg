// SPDX-License-Identifier: Apache-2.0
use super::{legacy_face_archive, ArchiveVersion, chunk_at};
use cadmpeg_ir::{document::CadIr, math::Point3};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_ir::{geometry::nurbs::NurbsCurve, topology::LoopBoundaryRole};

fn parsed_brep(data: &[u8]) -> super::super::LegacyBrep {
    let ctx = cadmpeg_test_support::service_decode_context();
    let comment = chunk_at(data, 32, data.len(), ArchiveVersion::V1, false).unwrap();
    let face = chunk_at(data, comment.next_offset(), data.len(), ArchiveVersion::V1, false).unwrap();
    let mut storage = ctx.reserve_scoped(0, "test parsing workspace").unwrap();
    super::super::legacy_brep(&ctx, &mut storage, data, &face, super::super::MillimeterScale::IDENTITY).unwrap()
}

#[test]
fn v1_brep_first_trim_index_refuses_before_topology_output() {
    let data = legacy_face_archive();
    for (dimension, operation) in [
        (ResourceDimension::MaterializedBytes, "Rhino V1 Brep first trim index"),
        (ResourceDimension::WorkUnits, "Rhino V1 Brep first trim lookup"),
    ] {
        cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
            let brep = parsed_brep(&data);
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
                _ => unreachable!("test dimensions"),
            }
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&data, &arena, &policy).unwrap();
            let mut ir = CadIr::empty();
            let result = super::super::append_legacy_brep(&ctx, &mut ir, brep, "indexed");
            assert_eq!(ir.model.entity_count(), 0);
            result
        });
    }
}

#[test]
fn v1_brep_first_trim_index_keeps_distinct_faces_and_loops() {
    let data = legacy_face_archive();
    let mut brep = parsed_brep(&data);
    let face = &mut brep.faces[0];
    face.loops.push(super::super::LegacyLoop {
        role: LoopBoundaryRole::Inner,
        trims: face.loops[0].trims.clone(),
    });
    let second = super::super::LegacyFace {
        reversed: face.reversed,
        seam_glue: Vec::new(),
        surface: face.surface.clone(),
        loops: face.loops.iter().map(|row| super::super::LegacyLoop {
            role: row.role, trims: row.trims.clone(),
        }).collect(),
    };
    brep.faces.push(second);
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut ir = CadIr::empty();
    super::super::append_legacy_brep(&ctx, &mut ir, brep, "indexed").unwrap();
    assert_eq!(ir.model.faces.len(), 2);
    assert_eq!(ir.model.loops.len(), 4);
    assert_eq!(ir.model.edges.len(), 16);
    assert_eq!(ir.model.coedges.len(), 16);
    assert_eq!(ir.model.vertices.len(), 16);
    for row in &ir.model.loops {
        assert_eq!(row.coedges().len(), 4);
        let positions: Vec<_> = row.coedges().iter().map(|id| {
            ir.model.coedges.iter().position(|coedge| &coedge.id == id).unwrap()
        }).collect();
        for (slot, index) in positions.iter().enumerate() {
            let coedge = &ir.model.coedges[*index];
            assert_eq!(row.next_coedge(&coedge.id), Some(&ir.model.coedges[positions[(slot + 1) % 4]].id));
            assert_eq!(row.previous_coedge(&coedge.id), Some(&ir.model.coedges[positions[(slot + 3) % 4]].id));
            assert_eq!(coedge.radial_next, coedge.id);
        }
    }
}

#[test]
fn v1_topology_parent_walk_refuses_before_compression() {
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits, "Rhino V1 topology parent walk", |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut parents = [1, 2, 3, 3];
            let result = super::super::find_root(&ctx, &mut parents, 0);
            assert_eq!(parents, [1, 2, 3, 3]);
            result
        },
    );
}

#[test]
fn v1_curve_evaluation_charges_actual_quadratic_degree_steps() {
    let parse_ctx = cadmpeg_test_support::service_decode_context();
    let curve = NurbsCurve::from_checked_lanes(
        &parse_ctx, 2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0),
             Point3::new(2.0, 0.0, 0.0)], None, false,
    ).unwrap().unwrap();
    // Three pole initializations, two degree levels, and three blends.
    // The endpoint selects its span directly, so no knot-span search runs.
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 8;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let point = super::super::evaluate_nurbs(&ctx, &curve, 1.0).unwrap();
    assert_eq!(point, Point3::new(2.0, 0.0, 0.0));
    ctx.finish_session().unwrap();
}

