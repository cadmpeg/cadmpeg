// SPDX-License-Identifier: Apache-2.0
//! Unit tests for the writable brep model the encoder builds from CADIR.

use super::{adjacent_quad_sheet, polygon_sheet};
use crate::writer::model::{WritableFaceSurface, WritableModel};
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::EdgeCarrier;

#[test]
fn zero_length_edge_parameter_range_is_a_writer_limit() {
    let mut ir = polygon_sheet(&[
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(0.0, 2.0, 0.0),
    ]);
    let curve = ir.model.edges[0]
        .curve()
        .expect("polygon edge has a curve")
        .clone();
    ir.model.edges[0].carrier =
        EdgeCarrier::new(Some(curve), Some([0.0, 0.0])).expect("equal endpoints are admitted");

    let error = WritableModel::try_new(&ir)
        .err()
        .expect("Rhino cannot write a zero-length domain");
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));
}

#[test]
fn admitted_unit_direction_outside_rhino_bound_is_a_writer_limit() {
    let mut ir = polygon_sheet(&[
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(0.0, 2.0, 0.0),
    ]);
    ir.model.curves[0].geometry = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        cadmpeg_ir::geometry::analytic::LineCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0 + 5.0e-10, 0.0, 0.0),
        )
        .expect("the IR unit-direction bound admits this direction"),
    ));

    let error = WritableModel::try_new(&ir)
        .err()
        .expect("Rhino uses a tighter direction bound");
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));
}

#[test]
fn admitted_frame_outside_rhino_bound_is_a_writer_limit() {
    let mut ir = polygon_sheet(&[
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(0.0, 2.0, 0.0),
    ]);
    ir.model.surfaces[0].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0 + 5.0e-10),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("the IR orthonormal-frame bound admits this frame"),
    ));

    let error = WritableModel::try_new(&ir)
        .err()
        .expect("Rhino uses a tighter frame bound");
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));
}

#[test]
fn single_face_resolves_arena_permutations_in_traversal_order() {
    let mut ir = polygon_sheet(&[
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(0.0, 2.0, 0.0),
    ]);
    let expected = super::super::brep_payload(
        &WritableModel::try_new(&ir).expect("writable triangle"),
        crate::RhinoArchiveVersion::V8,
    )
    .expect("triangle payload");
    ir.model.vertices.reverse();
    ir.model.edges.reverse();
    ir.model.coedges.reverse();
    let model =
        WritableModel::try_new(&ir).expect("references resolve independently of arena order");
    let actual = super::super::brep_payload(&model, crate::RhinoArchiveVersion::V8)
        .expect("permuted triangle payload");
    assert_eq!(actual.body, expected.body);
    assert_eq!(actual.direct, expected.direct);
    assert_eq!(model.loops[0].coedges, vec![0, 1, 2]);
    for (position, edge) in model.edges.iter().enumerate() {
        assert_eq!(edge.start, position);
        assert_eq!(edge.end, (position + 1) % 3);
        assert_eq!(edge.uses, vec![position]);
    }
}

#[test]
fn multi_face_resolves_domains_and_incidence_in_arena_order() {
    let ir = adjacent_quad_sheet();
    let model = WritableModel::try_new(&ir).expect("writable adjacent faces");
    for (position, edge) in model.edges.iter().enumerate() {
        assert_eq!(edge.source.id, ir.model.edges[position].id);
        assert_eq!(
            Some(edge.domain),
            ir.model.edges[position]
                .param_range()
                .map(cadmpeg_ir::units::FiniteVector::finite_components)
        );
        for coedge in &edge.uses {
            assert_eq!(model.coedges[*coedge].edge, position);
        }
    }
    for face in &model.faces {
        assert!(matches!(
            model.surfaces[face.surface],
            WritableFaceSurface::Plane { .. }
        ));
    }
}
