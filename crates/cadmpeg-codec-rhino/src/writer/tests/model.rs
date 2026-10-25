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
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::UnsupportedContent(_)
    ));
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
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::UnsupportedContent(_)
    ));
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
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::UnsupportedContent(_)
    ));
}

#[test]
fn single_face_resolves_arena_permutations_in_traversal_order() {
    let mut ir = polygon_sheet(&[
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(0.0, 2.0, 0.0),
    ]);
    let expected = super::super::brep_payload(
        &cadmpeg_test_support::service_decode_context(),
        &WritableModel::try_new(&ir).expect("writable triangle"),
        crate::RhinoArchiveVersion::V8,
    )
    .expect("triangle payload");
    ir.model.vertices.reverse();
    ir.model.edges.reverse();
    ir.model.coedges.reverse();
    let model =
        WritableModel::try_new(&ir).expect("references resolve independently of arena order");
    let actual = super::super::brep_payload(
        &cadmpeg_test_support::service_decode_context(),
        &model,
        crate::RhinoArchiveVersion::V8,
    )
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

#[test]
fn planar_brep_serializes_trim_iso_without_changing_its_topology_or_curves() {
    use crate::brep::{BrepParse, RawTrimIso};
    use crate::chunks::ArchiveVersion;

    let ir = polygon_sheet(&[
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(0.5, 0.0, 0.0),
        Point3::new(0.5, 0.5, 0.0),
        Point3::new(0.0, 0.5, 0.0),
    ]);
    let model = WritableModel::try_new(&ir).expect("independent valid planar square");
    for (version, archive) in [
        (crate::RhinoArchiveVersion::V5, ArchiveVersion::V5),
        (crate::RhinoArchiveVersion::V8, ArchiveVersion::V8),
    ] {
        let ctx = cadmpeg_test_support::service_decode_context();
        let payload = super::super::brep_payload(&ctx, &model, version).unwrap();
        let BrepParse::Valid(brep) = crate::brep::parse(
            &ctx,
            &payload.body,
            0..payload.body.len(),
            archive,
            Some(super::super::OPENNURBS_WRITER_VERSION),
            &[],
        )
        .expect("native Brep grammar") else {
            panic!("square Brep topology must remain valid")
        };
        let raw = brep.raw();
        assert_eq!(
            raw.trims.iter().map(|trim| trim.iso).collect::<Vec<_>>(),
            [RawTrimIso::Y, RawTrimIso::X, RawTrimIso::Y, RawTrimIso::X]
        );
        assert_eq!(raw.trims.len(), 4);
        assert_eq!(raw.edges.len(), 4);
        assert_eq!(raw.vertices.len(), 4);
        assert_eq!(raw.loops.len(), 1);
        assert_eq!(raw.faces.len(), 1);
        for (index, trim) in raw.trims.iter().enumerate() {
            assert_eq!(trim.edge, Some(i32::try_from(index).unwrap()));
            assert!(!trim.reversed_3d);
            assert_eq!(trim.loop_index, 0);
            assert_eq!(
                trim.domain.0.get(),
                model.edges[index]
                    .domain
                    .map(cadmpeg_ir::scalar::FiniteReal::get)
            );
        }
    }
}

#[test]
fn bounded_free_line_uses_native_endpoints_and_its_declared_domain() {
    let curve = cadmpeg_ir::geometry::Curve::new(
        "test:geometry:curve#bounded-line"
            .try_into()
            .expect("curve identity"),
        CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::new(
                cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, -2.0, 3.0))
                    .expect("finite origin"),
                cadmpeg_ir::units::UnitVector3::Y_AXIS,
            ),
        )),
        Some(
            cadmpeg_ir::topology::IncreasingParameterInterval::new([-2.0, 6.0])
                .expect("finite increasing range"),
        ),
        None,
    );
    let (class, payload) = crate::writer::model::WritableObjectCurve::try_new(&curve)
        .expect("bounded finite line")
        .payload()
        .expect("native bounded line");
    assert_eq!(class, crate::test_support::test_archive::LINE_CLASS);
    assert_eq!(
        payload,
        crate::test_support::test_archive::line_payload(
            [1.0, -4.0, 3.0],
            [1.0, 4.0, 3.0],
            [-2.0, 6.0],
        )
    );
}

#[test]
fn unbounded_or_collapsed_free_line_does_not_invent_a_native_domain_or_geometry() {
    let mut curve = cadmpeg_ir::geometry::Curve::new(
        "test:geometry:curve#unbounded-line"
            .try_into()
            .expect("curve identity"),
        CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::new(
                cadmpeg_ir::features::FinitePoint3::ZERO,
                cadmpeg_ir::units::UnitVector3::X_AXIS,
            ),
        )),
        None,
        None,
    );
    assert!(matches!(
        crate::writer::model::WritableObjectCurve::try_new(&curve),
        Err(cadmpeg_core::CodecError::UnsupportedContent(_))
    ));
    curve.parameter_range = Some(
        cadmpeg_ir::topology::IncreasingParameterInterval::new([0.0, 1.0])
            .expect("finite increasing range"),
    );
    curve.geometry = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        cadmpeg_ir::geometry::analytic::LineCurve::new(
            cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0e30, 0.0, 0.0))
                .expect("finite large origin"),
            cadmpeg_ir::units::UnitVector3::X_AXIS,
        ),
    ));
    assert!(matches!(
        crate::writer::model::WritableObjectCurve::try_new(&curve),
        Err(cadmpeg_core::CodecError::UnsupportedContent(_))
    ));
}
