// SPDX-License-Identifier: Apache-2.0

use super::{
    transfer_datum_plane_surfaces, transfer_display_tessellations,
    transfer_placed_plane_surfaces_into_ir, transfer_reference_circles,
    transfer_reference_ellipses, transfer_reference_lines, CadIr, CurveGeometry,
    SolvedCurveGeometry, SolvedSurfaceGeometry,
};
use crate::container::scan_bytes_ok;
use crate::legacy::PrincipalUnitSystem;
use crate::reference::{ReferenceCircle, ReferenceEllipse, ReferenceLine, ReferenceLineKind};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::PositiveReal;
use cadmpeg_ir::units::UnitVector3;

fn inch_line(start: Point3, end: Point3) -> crate::container::ContainerScan<'static> {
    let mut scan = scan_bytes_ok(crate::test_support::build_prt("line", &[]));
    scan.framing.principal_unit = Some(PrincipalUnitSystem::InchPoundMassSecond);
    scan.references.lines.push(ReferenceLine {
        kind: ReferenceLineKind::Line,
        start: FinitePoint3::new(start).expect("finite source point"),
        end: FinitePoint3::new(end).expect("finite source endpoint"),
        offset: 0,
    });
    scan
}

#[test]
fn reference_line_origin_is_in_millimeters_at_ir_admission() {
    let scan = inch_line(Point3::new(1.0, 2.0, 0.0), Point3::new(2.0, 2.0, 0.0));
    let mut ir = CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::decode::with_test_decode_ctx(|ctx| {
        let converted =
            transfer_reference_lines(ctx, &scan, &mut ir, &mut annotations).expect("line transfer");
        let Some(SolvedCurveGeometry::Line(line)) = ir.model.curves[0].geometry.solved() else {
            panic!("expected a line curve");
        };
        assert_eq!(line.origin().get(), Point3::new(25.4, 50.8, 0.0));
        super::super::units::normalize_model_lengths(
            &mut ir,
            cadmpeg_ir::scalar::PositiveReal::new(25.4).expect("unit scale"),
            &super::super::units::ConvertedGeometry {
                curves: converted,
                surfaces: std::collections::BTreeSet::new(),
            },
        )
        .expect("remaining unit normalization");
        assert!(matches!(
            ir.model.curves[0].geometry,
            CurveGeometry::Solved(SolvedCurveGeometry::Line(line))
                if line.origin().get() == Point3::new(25.4, 50.8, 0.0)
        ));
    });
}

#[test]
fn reference_line_origin_overflow_refuses_unrepresentable_ir() {
    let scan = inch_line(Point3::new(f64::MAX, 0.0, 0.0), Point3::new(0.0, 0.0, 0.0));
    let mut ir = CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::decode::with_test_decode_ctx(|ctx| {
        let error = transfer_reference_lines(ctx, &scan, &mut ir, &mut annotations)
            .expect_err("millimeter origin overflows");
        assert!(matches!(error, CodecError::NotImplemented(_)));
        assert!(ir.model.curves.is_empty());
    });
}

#[test]
fn reference_circle_center_and_radius_are_in_millimeters_at_ir_admission() {
    let mut scan = scan_bytes_ok(crate::test_support::build_prt("circle", &[]));
    scan.framing.principal_unit = Some(PrincipalUnitSystem::InchPoundMassSecond);
    scan.references.circles.push(ReferenceCircle {
        entity_id: 7,
        center: [1.0, 0.0, 0.0],
        center_stored: true,
        radius: PositiveReal::new(1.0).expect("positive source radius"),
        axis: UnitVector3::Z_AXIS,
        start: FinitePoint3::new(Point3::new(2.0, 0.0, 0.0)).expect("finite start"),
        end: FinitePoint3::new(Point3::new(1.0, 1.0, 0.0)).expect("finite end"),
        offset: 0,
    });
    let mut ir = CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::decode::with_test_decode_ctx(|ctx| {
        transfer_reference_circles(ctx, &scan, &mut ir, &mut annotations).expect("circle transfer");
    });
    let Some(SolvedCurveGeometry::Circle(circle)) = ir.model.curves[0].geometry.solved() else {
        panic!("expected a circle curve");
    };
    assert_eq!(circle.center().get(), Point3::new(25.4, 0.0, 0.0));
    assert_eq!(circle.radius().get(), 25.4);
    assert_eq!(scan.references.circles[0].radius.get(), 1.0);
}

#[test]
fn reference_ellipse_center_and_radii_are_in_millimeters_at_ir_admission() {
    let mut scan = scan_bytes_ok(crate::test_support::build_prt("ellipse", &[]));
    scan.framing.principal_unit = Some(PrincipalUnitSystem::InchPoundMassSecond);
    scan.references.ellipses.push(ReferenceEllipse {
        source_entity_id: 8,
        center: FinitePoint3::new(Point3::new(1.0, 0.0, 0.0)).expect("finite center"),
        axis: UnitVector3::Z_AXIS,
        major_direction: UnitVector3::X_AXIS,
        major_radius: PositiveReal::new(2.0).expect("positive major radius"),
        minor_radius: PositiveReal::new(1.0).expect("positive minor radius"),
        offset: 0,
    });
    let mut ir = CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::decode::with_test_decode_ctx(|ctx| {
        transfer_reference_ellipses(ctx, &scan, &mut ir, &mut annotations)
            .expect("ellipse transfer");
    });
    let Some(SolvedCurveGeometry::Ellipse(ellipse)) = ir.model.curves[0].geometry.solved() else {
        panic!("expected an ellipse curve");
    };
    assert_eq!(ellipse.center().get(), Point3::new(25.4, 0.0, 0.0));
    assert_eq!(ellipse.major_radius().get(), 50.8);
    assert_eq!(ellipse.minor_radius().get(), 25.4);
    assert_eq!(scan.references.ellipses[0].major_radius.get(), 2.0);
}

#[test]
fn reference_circle_radius_overflow_refuses_unrepresentable_ir() {
    let mut scan = scan_bytes_ok(crate::test_support::build_prt("circle", &[]));
    scan.framing.principal_unit = Some(PrincipalUnitSystem::InchPoundMassSecond);
    scan.references.circles.push(ReferenceCircle {
        entity_id: 9,
        center: [0.0, 0.0, 0.0],
        center_stored: false,
        radius: PositiveReal::new(1e307).expect("positive source radius"),
        axis: UnitVector3::Z_AXIS,
        start: FinitePoint3::new(Point3::new(-1e307, 0.0, 0.0)).expect("finite start"),
        end: FinitePoint3::new(Point3::new(1e307, 0.0, 0.0)).expect("finite end"),
        offset: 0,
    });
    let mut ir = CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::decode::with_test_decode_ctx(|ctx| {
        let error = transfer_reference_circles(ctx, &scan, &mut ir, &mut annotations)
            .expect_err("scaled radius overflows");
        assert!(matches!(error, CodecError::NotImplemented(_)));
        assert!(ir.model.curves.is_empty());
    });
}

#[test]
fn reference_ellipse_minor_radius_collapse_refuses_unrepresentable_ir() {
    let mut scan = scan_bytes_ok(crate::test_support::build_prt("ellipse", &[]));
    scan.framing.principal_unit = Some(PrincipalUnitSystem::LegacyLengthScale(
        PositiveReal::new(1e-300).expect("positive source scale"),
    ));
    scan.references.ellipses.push(ReferenceEllipse {
        source_entity_id: 10,
        center: FinitePoint3::ZERO,
        axis: UnitVector3::Z_AXIS,
        major_direction: UnitVector3::X_AXIS,
        major_radius: PositiveReal::new(1.0).expect("positive major radius"),
        minor_radius: PositiveReal::new(f64::MIN_POSITIVE).expect("positive minor radius"),
        offset: 0,
    });
    let mut ir = CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::decode::with_test_decode_ctx(|ctx| {
        let error = transfer_reference_ellipses(ctx, &scan, &mut ir, &mut annotations)
            .expect_err("scaled minor radius collapses");
        assert!(matches!(error, CodecError::NotImplemented(_)));
        assert!(ir.model.curves.is_empty());
    });
}

#[test]
fn reference_circle_center_overflow_refuses_unrepresentable_ir() {
    let mut scan = scan_bytes_ok(crate::test_support::build_prt("circle", &[]));
    scan.framing.principal_unit = Some(PrincipalUnitSystem::InchPoundMassSecond);
    scan.references.circles.push(ReferenceCircle {
        entity_id: 11,
        center: [8e306, 0.0, 0.0],
        center_stored: false,
        radius: PositiveReal::new(1e306).expect("positive source radius"),
        axis: UnitVector3::Z_AXIS,
        start: FinitePoint3::new(Point3::new(7e306, 0.0, 0.0)).expect("finite start"),
        end: FinitePoint3::new(Point3::new(9e306, 0.0, 0.0)).expect("finite end"),
        offset: 0,
    });
    let mut ir = CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::decode::with_test_decode_ctx(|ctx| {
        let error = transfer_reference_circles(ctx, &scan, &mut ir, &mut annotations)
            .expect_err("scaled center overflows");
        assert!(matches!(error, CodecError::NotImplemented(_)));
        assert!(ir.model.curves.is_empty());
    });
}

#[test]
fn reference_circle_radius_collapse_refuses_unrepresentable_ir() {
    let mut scan = scan_bytes_ok(crate::test_support::build_prt("circle", &[]));
    scan.framing.principal_unit = Some(PrincipalUnitSystem::LegacyLengthScale(
        PositiveReal::new(f64::from_bits(1)).expect("positive source scale"),
    ));
    scan.references.circles.push(ReferenceCircle {
        entity_id: 12,
        center: [0.0, 0.0, 0.0],
        center_stored: false,
        radius: PositiveReal::new(0.5).expect("positive source radius"),
        axis: UnitVector3::Z_AXIS,
        start: FinitePoint3::new(Point3::new(-0.5, 0.0, 0.0)).expect("finite start"),
        end: FinitePoint3::new(Point3::new(0.5, 0.0, 0.0)).expect("finite end"),
        offset: 0,
    });
    let mut ir = CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::decode::with_test_decode_ctx(|ctx| {
        let error = transfer_reference_circles(ctx, &scan, &mut ir, &mut annotations)
            .expect_err("scaled radius collapses");
        assert!(matches!(error, CodecError::NotImplemented(_)));
        assert!(ir.model.curves.is_empty());
    });
}

#[test]
fn reference_ellipse_center_overflow_refuses_unrepresentable_ir() {
    let mut scan = scan_bytes_ok(crate::test_support::build_prt("ellipse", &[]));
    scan.framing.principal_unit = Some(PrincipalUnitSystem::InchPoundMassSecond);
    scan.references.ellipses.push(ReferenceEllipse {
        source_entity_id: 13,
        center: FinitePoint3::new(Point3::new(8e306, 0.0, 0.0)).expect("finite center"),
        axis: UnitVector3::Z_AXIS,
        major_direction: UnitVector3::X_AXIS,
        major_radius: PositiveReal::new(1e306).expect("positive major radius"),
        minor_radius: PositiveReal::new(5e305).expect("positive minor radius"),
        offset: 0,
    });
    let mut ir = CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::decode::with_test_decode_ctx(|ctx| {
        let error = transfer_reference_ellipses(ctx, &scan, &mut ir, &mut annotations)
            .expect_err("scaled center overflows");
        assert!(matches!(error, CodecError::NotImplemented(_)));
        assert!(ir.model.curves.is_empty());
    });
}

#[test]
fn reference_ellipse_major_radius_overflow_refuses_unrepresentable_ir() {
    let mut scan = scan_bytes_ok(crate::test_support::build_prt("ellipse", &[]));
    scan.framing.principal_unit = Some(PrincipalUnitSystem::InchPoundMassSecond);
    scan.references.ellipses.push(ReferenceEllipse {
        source_entity_id: 14,
        center: FinitePoint3::ZERO,
        axis: UnitVector3::Z_AXIS,
        major_direction: UnitVector3::X_AXIS,
        major_radius: PositiveReal::new(f64::MAX).expect("positive major radius"),
        minor_radius: PositiveReal::new(1.0).expect("positive minor radius"),
        offset: 0,
    });
    let mut ir = CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::decode::with_test_decode_ctx(|ctx| {
        let error = transfer_reference_ellipses(ctx, &scan, &mut ir, &mut annotations)
            .expect_err("scaled major radius overflows");
        assert!(matches!(error, CodecError::NotImplemented(_)));
        assert!(ir.model.curves.is_empty());
    });
}

#[test]
fn datum_plane_origin_is_in_millimeters_at_ir_admission() {
    let mut scan = scan_bytes_ok(crate::test_support::build_prt("datum", &[]));
    scan.framing.principal_unit = Some(PrincipalUnitSystem::InchPoundMassSecond);
    scan.planes.datums.push(crate::datum::DatumPlaneRecord {
        id: 15,
        feature_id: 15,
        plane: crate::datum::DatumPlane {
            axis: crate::datum::Axis::X,
            offset: 2.0,
        },
        opposite_offset: 2.0,
        in_plane_corners: [[None; 2]; 2],
        offset_in_payload: 0,
    });
    let mut ir = CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::decode::with_test_decode_ctx(|ctx| {
        transfer_datum_plane_surfaces(ctx, &scan, &mut ir, &mut annotations)
            .expect("datum plane transfer");
    });
    let Some(SolvedSurfaceGeometry::Plane(plane)) = ir.model.surfaces[0].geometry.solved() else {
        panic!("expected a plane surface");
    };
    assert_eq!(plane.origin().get(), Point3::new(50.8, 0.0, 0.0));
    assert_eq!(scan.planes.datums[0].plane.offset, 2.0);
}

#[test]
fn datum_plane_origin_overflow_refuses_unrepresentable_ir() {
    let mut scan = scan_bytes_ok(crate::test_support::build_prt("datum", &[]));
    scan.framing.principal_unit = Some(PrincipalUnitSystem::InchPoundMassSecond);
    scan.planes.datums.push(crate::datum::DatumPlaneRecord {
        id: 16,
        feature_id: 16,
        plane: crate::datum::DatumPlane {
            axis: crate::datum::Axis::X,
            offset: f64::MAX,
        },
        opposite_offset: f64::MAX,
        in_plane_corners: [[None; 2]; 2],
        offset_in_payload: 0,
    });
    let mut ir = CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::decode::with_test_decode_ctx(|ctx| {
        let error = transfer_datum_plane_surfaces(ctx, &scan, &mut ir, &mut annotations)
            .expect_err("scaled datum plane origin overflows");
        assert!(matches!(error, CodecError::NotImplemented(_)));
        assert!(ir.model.surfaces.is_empty());
    });
}

#[test]
fn placed_plane_origin_is_in_millimeters_at_ir_admission() {
    let mut payload = b"srf_array\0\xf8\x01".to_vec();
    crate::test_support::push_generated_plane_row(
        &mut payload,
        17,
        false,
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    );
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");
    let mut scan = scan_bytes_ok(crate::test_support::build_prt(
        "plane",
        &[("ND:0:VisibGeom:0", payload)],
    ));
    scan.framing.principal_unit = Some(PrincipalUnitSystem::InchPoundMassSecond);
    let mut ir = CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::decode::with_test_decode_ctx(|ctx| {
        transfer_placed_plane_surfaces_into_ir(ctx, &scan, &mut ir, &mut annotations)
            .expect("placed plane transfer");
    });
    let Some(SolvedSurfaceGeometry::Plane(plane)) = ir.model.surfaces[0].geometry.solved() else {
        panic!("expected a plane surface");
    };
    assert_eq!(plane.origin().get(), Point3::new(25.4, 0.0, 0.0));
}

#[test]
fn display_tessellation_vertices_are_in_millimeters_at_ir_admission() {
    let mut scan = scan_bytes_ok(crate::test_support::build_prt("strip", &[]));
    scan.framing.principal_unit = Some(PrincipalUnitSystem::InchPoundMassSecond);
    scan.primitives
        .triangle_strips
        .push(crate::primdata::PrimitiveTriangleStrip {
            offset: 0,
            positions: vec![[1.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 4.0]],
            normals: None,
            strip_lengths: vec![3],
        });
    let mut ir = CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::decode::with_test_decode_ctx(|ctx| {
        transfer_display_tessellations(ctx, &scan, &mut ir, &mut annotations)
            .expect("display tessellation transfer");
    });
    assert_eq!(
        ir.model.tessellations[0].vertices()[0].get(),
        Point3::new(25.4, 0.0, 0.0)
    );
    assert_eq!(
        ir.model.tessellations[0].vertices()[1].get(),
        Point3::new(0.0, 50.8, 0.0)
    );
    assert_eq!(
        ir.model.tessellations[0].vertices()[2].get(),
        Point3::new(0.0, 0.0, 101.6)
    );
    assert_eq!(
        scan.primitives.triangle_strips[0].positions[0],
        [1.0, 0.0, 0.0]
    );
    super::super::units::normalize_model_lengths(
        &mut ir,
        PositiveReal::new(25.4).expect("unit scale"),
        &super::super::units::ConvertedGeometry::default(),
    )
    .expect("remaining unit normalization");
    assert_eq!(
        ir.model.tessellations[0].vertices()[0].get(),
        Point3::new(25.4, 0.0, 0.0)
    );
}

#[test]
fn display_tessellation_vertex_overflow_refuses_unrepresentable_ir() {
    let mut scan = scan_bytes_ok(crate::test_support::build_prt("strip", &[]));
    scan.framing.principal_unit = Some(PrincipalUnitSystem::InchPoundMassSecond);
    scan.primitives
        .triangle_strips
        .push(crate::primdata::PrimitiveTriangleStrip {
            offset: 0,
            positions: vec![[f64::MAX, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 3.0]],
            normals: None,
            strip_lengths: vec![3],
        });
    let mut ir = CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::decode::with_test_decode_ctx(|ctx| {
        let error = transfer_display_tessellations(ctx, &scan, &mut ir, &mut annotations)
            .expect_err("scaled display vertex overflows");
        assert!(matches!(error, CodecError::NotImplemented(_)));
        assert!(ir.model.tessellations.is_empty());
    });
}
