// SPDX-License-Identifier: Apache-2.0

use super::{
    transfer_datum_plane_surfaces, transfer_display_tessellations,
    transfer_placed_plane_surfaces_into_ir, transfer_reference_circles,
    transfer_reference_ellipses, transfer_reference_lines,
};
use crate::container::{scan_bytes_ok, ContainerScan};
use crate::decode::source_carriers::SourceUnitCarriers;
use crate::legacy::PrincipalUnitSystem;
use crate::primdata::PrimitiveTriangleStrip;
use crate::scalar::PlaneSupportFrameLayout;
use crate::surface::{LocalSystemClassification, PlaneLocalSystem};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::PositiveReal;
use cadmpeg_ir::units::FiniteVector;

fn inch_strip(positions: Vec<[f64; 3]>) -> ContainerScan<'static> {
    let mut scan = scan_bytes_ok(crate::test_support::build_prt("strip", &[]));
    scan.framing.principal_unit = Some(PrincipalUnitSystem::InchPoundMassSecond);
    scan.primitives
        .triangle_strips
        .push(PrimitiveTriangleStrip {
            offset: 0,
            positions: positions
                .into_iter()
                .map(|position| FiniteVector::new(position).expect("finite strip position"))
                .collect(),
            normals: None,
            strip_lengths: vec![3],
        });
    scan
}

#[test]
fn display_tessellation_vertices_are_in_millimeters_at_ir_admission() {
    let scan = inch_strip(vec![[1.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 4.0]]);
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
        &SourceUnitCarriers::default(),
    )
    .expect("remaining unit normalization");
    assert_eq!(
        ir.model.tessellations[0].vertices()[0].get(),
        Point3::new(25.4, 0.0, 0.0)
    );
}

#[test]
fn display_tessellation_vertex_overflow_refuses_unrepresentable_ir() {
    let scan = inch_strip(vec![[f64::MAX, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 4.0]]);
    let mut ir = CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::decode::with_test_decode_ctx(|ctx| {
        let error = transfer_display_tessellations(ctx, &scan, &mut ir, &mut annotations)
            .expect_err("scaled display vertex overflows");
        assert!(matches!(error, CodecError::NotImplemented(_)));
        assert!(ir.model.tessellations.is_empty());
    });
}

#[test]
fn positional_plane_cross_overflow_refuses_at_ir_transfer() {
    let a = f64::from_bits(0x5fed_817d_bb14_96d1);
    let b = f64::from_bits(0x5fd8_c57e_64a4_a42f);
    let mut scan = scan_bytes_ok(crate::test_support::build_prt("plane", &[]));
    scan.planes.local_systems.push(PlaneLocalSystem {
        surface_id: 17,
        body: Vec::new(),
        slots: [a, b, 0.0, -b, a, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0].map(Some),
        layout: Some(PlaneSupportFrameLayout::SupportTriples),
        classification: LocalSystemClassification::Simple,
        row_offset: 0,
        offset: 0,
    });
    let mut ir = CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::decode::with_test_decode_ctx(|ctx| {
        let error = transfer_placed_plane_surfaces_into_ir(
            ctx,
            &scan,
            &mut ir,
            &mut annotations,
            &mut SourceUnitCarriers::default(),
        )
        .expect_err("finite plane support cross overflows the frame");
        assert!(matches!(error, CodecError::NotImplemented(_)));
        assert!(ir.model.surfaces.is_empty());
    });
}

#[test]
fn positional_plane_missing_slots_remain_unplaced() {
    let mut scan = scan_bytes_ok(crate::test_support::build_prt("plane", &[]));
    let mut slots = [Some(0.0); 12];
    slots[0] = None;
    scan.planes.local_systems.push(PlaneLocalSystem {
        surface_id: 17,
        body: Vec::new(),
        slots,
        layout: Some(PlaneSupportFrameLayout::SupportTriples),
        classification: LocalSystemClassification::Simple,
        row_offset: 0,
        offset: 0,
    });
    let mut ir = CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::decode::with_test_decode_ctx(|ctx| {
        transfer_placed_plane_surfaces_into_ir(
            ctx,
            &scan,
            &mut ir,
            &mut annotations,
            &mut SourceUnitCarriers::default(),
        )
        .expect("a missing support slot does not define a frame");
        assert!(ir.model.surfaces.is_empty());
    });
}

#[test]
fn placed_plane_origin_is_in_millimeters_at_ir_admission() {
    let mut payload = b"srf_array\0\xf8\x01".to_vec();
    crate::test_support::push_generated_plane_row(
        &mut payload,
        18,
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
    let scale = PositiveReal::new(25.4).expect("inch scale");
    let mut source_carriers = SourceUnitCarriers::new(Some(scale));
    crate::decode::with_test_decode_ctx(|ctx| {
        transfer_placed_plane_surfaces_into_ir(
            ctx,
            &scan,
            &mut ir,
            &mut annotations,
            &mut source_carriers,
        )
        .expect("placed plane transfer");
    });
    let Some(cadmpeg_ir::geometry::SurfaceGeometry::Solved(
        cadmpeg_ir::geometry::SolvedSurfaceGeometry::Plane(plane),
    )) = ir.model.surfaces.first().map(|surface| &surface.geometry)
    else {
        panic!("placed plane was not admitted");
    };
    assert_eq!(plane.origin().get(), Point3::new(25.4, 0.0, 0.0));
    let carriers = crate::decode::analytic::carriers::placed_carriers(&scan, &ir, &source_carriers);
    let Some(crate::decode::analytic::equations::CarrierEquation::Plane(source_plane)) =
        carriers.get(&18)
    else {
        panic!("placed plane was lost before native topology transfer");
    };
    assert_eq!(source_plane.origin, [1.0, 0.0, 0.0]);
    super::super::units::normalize_model_lengths(&mut ir, scale, &source_carriers)
        .expect("remaining unit normalization");
    let Some(cadmpeg_ir::geometry::SurfaceGeometry::Solved(
        cadmpeg_ir::geometry::SolvedSurfaceGeometry::Plane(plane),
    )) = ir.model.surfaces.first().map(|surface| &surface.geometry)
    else {
        panic!("placed plane changed family");
    };
    assert_eq!(plane.origin().get(), Point3::new(25.4, 0.0, 0.0));
}

#[test]
fn placed_plane_stays_available_to_source_unit_carrier_analysis() {
    let mut payload = b"srf_array\0\xf8\x01".to_vec();
    crate::test_support::push_generated_plane_row(
        &mut payload,
        18,
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
    let mut source_carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
    crate::decode::with_test_decode_ctx(|ctx| {
        transfer_placed_plane_surfaces_into_ir(
            ctx,
            &scan,
            &mut ir,
            &mut annotations,
            &mut source_carriers,
        )
        .expect("placed plane transfer");
    });
    let carriers = crate::decode::analytic::carriers::placed_carriers(&scan, &ir, &source_carriers);
    let Some(crate::decode::analytic::equations::CarrierEquation::Plane(plane)) = carriers.get(&18)
    else {
        panic!("placed plane was lost before native topology transfer");
    };
    assert_eq!(plane.origin, [1.0, 0.0, 0.0]);
}

#[test]
fn placed_plane_scaled_origin_overflow_refuses_unrepresentable_ir() {
    let mut scan = scan_bytes_ok(crate::test_support::build_prt("plane", &[]));
    scan.planes.local_systems.push(PlaneLocalSystem {
        surface_id: 18,
        body: Vec::new(),
        slots: [
            1.0,
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
            0.0,
            0.0,
            0.0,
            f64::MAX,
            0.0,
            0.0,
        ]
        .map(Some),
        layout: Some(PlaneSupportFrameLayout::SupportTriples),
        classification: LocalSystemClassification::Simple,
        row_offset: 0,
        offset: 0,
    });
    scan.framing.principal_unit = Some(PrincipalUnitSystem::InchPoundMassSecond);
    let mut ir = CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    let mut source_carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
    crate::decode::with_test_decode_ctx(|ctx| {
        let error = transfer_placed_plane_surfaces_into_ir(
            ctx,
            &scan,
            &mut ir,
            &mut annotations,
            &mut source_carriers,
        )
        .expect_err("the millimeter plane origin is not representable");
        assert!(matches!(error, CodecError::NotImplemented(_)), "{error}");
    });
    assert!(ir.model.surfaces.is_empty());
}

fn inch_datum_plane(offset: f64) -> ContainerScan<'static> {
    let mut scan = scan_bytes_ok(crate::test_support::build_prt("datum", &[]));
    scan.framing.principal_unit = Some(PrincipalUnitSystem::InchPoundMassSecond);
    scan.planes.datums.push(crate::datum::DatumPlaneRecord {
        id: 5,
        feature_id: 1,
        plane: crate::datum::DatumPlane {
            axis: crate::datum::Axis::X,
            offset,
        },
        opposite_offset: offset,
        in_plane_corners: [[Some(0.0); 2]; 2],
        offset_in_payload: 0,
    });
    scan
}

#[test]
fn datum_plane_origin_is_in_millimeters_at_ir_admission() {
    let scan = inch_datum_plane(1.0);
    let mut ir = CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    let scale = PositiveReal::new(25.4).expect("inch scale");
    let mut source_carriers = SourceUnitCarriers::new(Some(scale));
    crate::decode::with_test_decode_ctx(|ctx| {
        transfer_datum_plane_surfaces(ctx, &scan, &mut ir, &mut annotations, &mut source_carriers)
            .expect("datum plane transfer");
    });
    let surface = ir.model.surfaces.first().expect("datum surface");
    let cadmpeg_ir::geometry::SurfaceGeometry::Solved(
        cadmpeg_ir::geometry::SolvedSurfaceGeometry::Plane(plane),
    ) = &surface.geometry
    else {
        panic!("datum plane changed family");
    };
    assert_eq!(plane.origin().get(), Point3::new(25.4, 0.0, 0.0));
    let cadmpeg_ir::geometry::SurfaceGeometry::Solved(
        cadmpeg_ir::geometry::SolvedSurfaceGeometry::Plane(source_plane),
    ) = source_carriers.surface_geometry(surface)
    else {
        panic!("source datum plane changed family");
    };
    assert_eq!(source_plane.origin().get(), Point3::new(1.0, 0.0, 0.0));
    super::super::units::normalize_model_lengths(&mut ir, scale, &source_carriers)
        .expect("remaining unit normalization");
    let cadmpeg_ir::geometry::SurfaceGeometry::Solved(
        cadmpeg_ir::geometry::SolvedSurfaceGeometry::Plane(plane),
    ) = &ir.model.surfaces[0].geometry
    else {
        panic!("datum plane changed family");
    };
    assert_eq!(plane.origin().get(), Point3::new(25.4, 0.0, 0.0));
}

#[test]
fn datum_plane_scaled_origin_overflow_refuses_unrepresentable_ir() {
    let scan = inch_datum_plane(f64::MAX);
    let mut ir = CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    let mut source_carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
    crate::decode::with_test_decode_ctx(|ctx| {
        let error = transfer_datum_plane_surfaces(
            ctx,
            &scan,
            &mut ir,
            &mut annotations,
            &mut source_carriers,
        )
        .expect_err("millimeter datum origin cannot be represented");
        assert!(matches!(error, CodecError::NotImplemented(_)), "{error}");
    });
    assert!(ir.model.surfaces.is_empty());
}

#[test]
fn reference_line_origin_is_in_millimeters_at_ir_admission() {
    let mut scan = scan_bytes_ok(crate::test_support::build_prt("reference", &[]));
    scan.references.lines.push(crate::reference::ReferenceLine {
        kind: crate::reference::ReferenceLineKind::Line,
        start: cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 0.0, 0.0))
            .expect("finite source point"),
        end: cadmpeg_ir::features::FinitePoint3::new(Point3::new(2.0, 0.0, 0.0))
            .expect("finite source point"),
        offset: 0,
    });
    let mut ir = CadIr::empty();
    let mut carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
    crate::decode::with_test_decode_ctx(|ctx| {
        transfer_reference_lines(
            ctx,
            &scan,
            &mut ir,
            &mut cadmpeg_ir::AnnotationBuilder::new(),
            &mut carriers,
        )
        .expect("reference line transfer");
    });
    let curve = ir.model.curves.first().expect("reference line");
    let CurveGeometry::Solved(SolvedCurveGeometry::Line(line)) = &curve.geometry else {
        panic!("reference line changed family");
    };
    assert_eq!(line.origin().get(), Point3::new(25.4, 0.0, 0.0));
    let CurveGeometry::Solved(SolvedCurveGeometry::Line(source_line)) =
        carriers.curve_geometry(curve)
    else {
        panic!("source reference line changed family");
    };
    assert_eq!(source_line.origin().get(), Point3::new(1.0, 0.0, 0.0));
    super::super::units::normalize_model_lengths(
        &mut ir,
        PositiveReal::new(25.4).expect("inch scale"),
        &carriers,
    )
    .expect("remaining unit normalization");
    let CurveGeometry::Solved(SolvedCurveGeometry::Line(line)) = &ir.model.curves[0].geometry
    else {
        panic!("reference line changed family");
    };
    assert_eq!(line.origin().get(), Point3::new(25.4, 0.0, 0.0));
}

#[test]
fn reference_line_scaled_origin_overflow_refuses_before_ir_admission() {
    let mut scan = scan_bytes_ok(crate::test_support::build_prt("reference", &[]));
    scan.references.lines.push(crate::reference::ReferenceLine {
        kind: crate::reference::ReferenceLineKind::Line,
        start: cadmpeg_ir::features::FinitePoint3::new(Point3::new(f64::MAX, 0.0, 0.0))
            .expect("finite source point"),
        end: cadmpeg_ir::features::FinitePoint3::new(Point3::new(f64::MAX, 1.0, 0.0))
            .expect("finite source point"),
        offset: 0,
    });
    let mut ir = CadIr::empty();
    let mut carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
    crate::decode::with_test_decode_ctx(|ctx| {
        let error = transfer_reference_lines(
            ctx,
            &scan,
            &mut ir,
            &mut cadmpeg_ir::AnnotationBuilder::new(),
            &mut carriers,
        )
        .expect_err("millimeter origin cannot be represented");
        assert!(matches!(error, CodecError::NotImplemented(_)), "{error}");
    });
    assert!(ir.model.curves.is_empty());
}

#[test]
fn reference_circle_radius_is_in_millimeters_at_ir_admission() {
    let mut scan = scan_bytes_ok(crate::test_support::build_prt("reference", &[]));
    scan.references
        .circles
        .push(crate::reference::ReferenceCircle {
            entity_id: 7,
            center: [1.0, 0.0, 0.0],
            center_stored: true,
            radius: cadmpeg_ir::scalar::PositiveLength::new(2.0).expect("positive radius"),
            axis: cadmpeg_ir::units::UnitVector3::new([0.0, 0.0, 1.0].into()).expect("unit axis"),
            start: cadmpeg_ir::features::FinitePoint3::new(Point3::new(3.0, 0.0, 0.0))
                .expect("finite start"),
            end: cadmpeg_ir::features::FinitePoint3::ZERO,
            offset: 0,
        });
    let mut ir = CadIr::empty();
    let mut carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
    crate::decode::with_test_decode_ctx(|ctx| {
        transfer_reference_circles(
            ctx,
            &scan,
            &mut ir,
            &mut cadmpeg_ir::AnnotationBuilder::new(),
            &mut carriers,
        )
        .expect("reference circle transfer");
    });
    let curve = ir.model.curves.first().expect("reference circle");
    let CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle)) = &curve.geometry else {
        panic!("reference circle changed family");
    };
    assert_eq!(circle.center().get(), Point3::new(25.4, 0.0, 0.0));
    assert_eq!(circle.radius().get(), 50.8);
    let CurveGeometry::Solved(SolvedCurveGeometry::Circle(source_circle)) =
        carriers.curve_geometry(curve)
    else {
        panic!("source reference circle changed family");
    };
    assert_eq!(source_circle.radius().get(), 2.0);
}

#[test]
fn reference_ellipse_radii_are_in_millimeters_at_ir_admission() {
    let mut scan = scan_bytes_ok(crate::test_support::build_prt("reference", &[]));
    scan.references
        .ellipses
        .push(crate::reference::ReferenceEllipse {
            source_entity_id: 8,
            center: cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 0.0, 0.0))
                .expect("finite center"),
            axis: cadmpeg_ir::units::UnitVector3::new([0.0, 0.0, 1.0].into()).expect("unit axis"),
            major_direction: cadmpeg_ir::units::UnitVector3::new([1.0, 0.0, 0.0].into())
                .expect("unit direction"),
            major_radius: cadmpeg_ir::scalar::PositiveLength::new(2.0).expect("positive radius"),
            minor_radius: cadmpeg_ir::scalar::PositiveLength::new(1.0).expect("positive radius"),
            offset: 0,
        });
    let mut ir = CadIr::empty();
    let mut carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
    crate::decode::with_test_decode_ctx(|ctx| {
        transfer_reference_ellipses(
            ctx,
            &scan,
            &mut ir,
            &mut cadmpeg_ir::AnnotationBuilder::new(),
            &mut carriers,
        )
        .expect("reference ellipse transfer");
    });
    let curve = ir.model.curves.first().expect("reference ellipse");
    let CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse)) = &curve.geometry else {
        panic!("reference ellipse changed family");
    };
    assert_eq!(ellipse.center().get(), Point3::new(25.4, 0.0, 0.0));
    assert_eq!(ellipse.major_radius().get(), 50.8);
    assert_eq!(ellipse.minor_radius().get(), 25.4);
    let CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(source_ellipse)) =
        carriers.curve_geometry(curve)
    else {
        panic!("source reference ellipse changed family");
    };
    assert_eq!(source_ellipse.major_radius().get(), 2.0);
}
