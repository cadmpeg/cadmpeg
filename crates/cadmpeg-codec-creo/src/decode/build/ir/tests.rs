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

fn retained_boundary_sweep(
    expected: &[&str],
    mut run: impl for<'a> FnMut(&cadmpeg_core::decode::DecodeContext<'a>) -> Result<(), CodecError>,
) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let arena = DecodeArena::new();
    let mut observed = std::collections::BTreeSet::new();
    let mut exact_cap = None;
    for limit in 0..4096 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = limit;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
        match run(&ctx) {
            Err(CodecError::ResourceLimit(resource)) => {
                assert_eq!(resource.dimension, ResourceDimension::RetainedBytes);
                observed.insert(resource.operation);
            }
            Ok(()) => {
                exact_cap = Some(limit);
                break;
            }
            Err(error) => panic!("unexpected transfer error: {error:?}"),
        }
    }
    assert!(
        exact_cap.is_some(),
        "transfer did not fit within scanned cap"
    );
    assert!(
        expected
            .iter()
            .all(|operation| observed.contains(operation)),
        "missing admission boundary: {expected:?} vs {observed:?}"
    );
}

fn collection_boundary_sweep(
    expected: &[&str],
    mut run: impl for<'a> FnMut(&cadmpeg_core::decode::DecodeContext<'a>) -> Result<(), CodecError>,
) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let arena = DecodeArena::new();
    let mut observed = std::collections::BTreeSet::new();
    let mut exact_cap = None;
    for limit in 0..128 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
        match run(&ctx) {
            Err(CodecError::ResourceLimit(resource)) => {
                assert_eq!(resource.dimension, ResourceDimension::CollectionItems);
                observed.insert(resource.operation);
            }
            Ok(()) => {
                exact_cap = Some(limit);
                break;
            }
            Err(error) => panic!("unexpected transfer error: {error:?}"),
        }
    }
    assert!(
        exact_cap.is_some(),
        "transfer did not fit within scanned cap"
    );
    assert!(
        expected
            .iter()
            .all(|operation| observed.contains(operation)),
        "missing admission boundary: {expected:?} vs {observed:?}"
    );
}

#[test]
fn reference_line_identity_and_count_refuse_below_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let mut scan = scan_bytes_ok(crate::test_support::build_prt("reference", &[]));
    scan.references.lines.push(crate::reference::ReferenceLine {
        kind: crate::reference::ReferenceLineKind::Line3d {
            entity_id: 7,
            original_length: PositiveReal::new(1.0).expect("positive length"),
        },
        start: cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).expect("start"),
        end: cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 0.0, 0.0)).expect("end"),
        offset: 0,
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = transfer_reference_lines(
        &ctx,
        &scan,
        &mut CadIr::empty(),
        &mut cadmpeg_ir::AnnotationBuilder::new(),
        &mut SourceUnitCarriers::default(),
    )
    .expect_err("count node exceeds cap");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.operation == "creo reference line3d count nodes"));
    retained_boundary_sweep(
        &[
            "creo reference line3d identity",
            "creo reference line3d object identity",
        ],
        |ctx| {
            transfer_reference_lines(
                ctx,
                &scan,
                &mut CadIr::empty(),
                &mut cadmpeg_ir::AnnotationBuilder::new(),
                &mut SourceUnitCarriers::default(),
            )
        },
    );
    let mut second = scan.references.lines[0].clone();
    second.offset = 1;
    scan.references.lines.push(second);
    retained_boundary_sweep(
        &[
            "creo reference line3d identity",
            "creo reference line3d object identity",
        ],
        |ctx| {
            transfer_reference_lines(
                ctx,
                &scan,
                &mut CadIr::empty(),
                &mut cadmpeg_ir::AnnotationBuilder::new(),
                &mut SourceUnitCarriers::default(),
            )
        },
    );
    scan.references.lines.pop();
    scan.references.lines[0].kind = crate::reference::ReferenceLineKind::Line;
    retained_boundary_sweep(
        &[
            "creo reference line identity",
            "creo reference line object identity",
        ],
        |ctx| {
            transfer_reference_lines(
                ctx,
                &scan,
                &mut CadIr::empty(),
                &mut cadmpeg_ir::AnnotationBuilder::new(),
                &mut SourceUnitCarriers::default(),
            )
        },
    );
}

#[test]
fn reference_circle_identity_and_count_refuse_below_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let mut scan = scan_bytes_ok(crate::test_support::build_prt("reference", &[]));
    scan.references
        .circles
        .push(crate::reference::ReferenceCircle {
            entity_id: 7,
            center: cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
                .expect("center"),
            center_stored: true,
            radius: cadmpeg_ir::scalar::PositiveLength::new(1.0).expect("radius"),
            axis: cadmpeg_ir::units::UnitVector3::new([0.0, 0.0, 1.0].into()).expect("axis"),
            start: cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 0.0, 0.0))
                .expect("start"),
            end: cadmpeg_ir::features::FinitePoint3::ZERO,
            offset: 0,
        });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = transfer_reference_circles(
        &ctx,
        &scan,
        &mut CadIr::empty(),
        &mut cadmpeg_ir::AnnotationBuilder::new(),
        &mut SourceUnitCarriers::default(),
    )
    .expect_err("count node exceeds cap");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.operation == "creo reference circle count nodes"));
    retained_boundary_sweep(
        &[
            "creo reference circle identity",
            "creo reference circle object identity",
        ],
        |ctx| {
            transfer_reference_circles(
                ctx,
                &scan,
                &mut CadIr::empty(),
                &mut cadmpeg_ir::AnnotationBuilder::new(),
                &mut SourceUnitCarriers::default(),
            )
        },
    );
    let mut second = scan.references.circles[0].clone();
    second.offset = 1;
    scan.references.circles.push(second);
    retained_boundary_sweep(
        &[
            "creo reference circle identity",
            "creo reference circle object identity",
        ],
        |ctx| {
            transfer_reference_circles(
                ctx,
                &scan,
                &mut CadIr::empty(),
                &mut cadmpeg_ir::AnnotationBuilder::new(),
                &mut SourceUnitCarriers::default(),
            )
        },
    );
}

#[test]
fn reference_ellipse_identity_and_count_refuse_below_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let mut scan = scan_bytes_ok(crate::test_support::build_prt("reference", &[]));
    scan.references
        .ellipses
        .push(crate::reference::ReferenceEllipse {
            source_entity_id: 8,
            center: cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
                .expect("center"),
            axis: cadmpeg_ir::units::UnitVector3::new([0.0, 0.0, 1.0].into()).expect("axis"),
            major_direction: cadmpeg_ir::units::UnitVector3::new([1.0, 0.0, 0.0].into())
                .expect("direction"),
            major_radius: cadmpeg_ir::scalar::PositiveLength::new(2.0).expect("major radius"),
            minor_radius: cadmpeg_ir::scalar::PositiveLength::new(1.0).expect("minor radius"),
            offset: 0,
        });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = transfer_reference_ellipses(
        &ctx,
        &scan,
        &mut CadIr::empty(),
        &mut cadmpeg_ir::AnnotationBuilder::new(),
        &mut SourceUnitCarriers::default(),
    )
    .expect_err("count node exceeds cap");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.operation == "creo reference ellipse count nodes"));
    retained_boundary_sweep(
        &[
            "creo reference ellipse identity",
            "creo reference ellipse object identity",
        ],
        |ctx| {
            transfer_reference_ellipses(
                ctx,
                &scan,
                &mut CadIr::empty(),
                &mut cadmpeg_ir::AnnotationBuilder::new(),
                &mut SourceUnitCarriers::default(),
            )
        },
    );
    let mut second = scan.references.ellipses[0].clone();
    second.offset = 1;
    scan.references.ellipses.push(second);
    retained_boundary_sweep(
        &[
            "creo reference ellipse identity",
            "creo reference ellipse object identity",
        ],
        |ctx| {
            transfer_reference_ellipses(
                ctx,
                &scan,
                &mut CadIr::empty(),
                &mut cadmpeg_ir::AnnotationBuilder::new(),
                &mut SourceUnitCarriers::default(),
            )
        },
    );
}

#[test]
fn datum_surface_identity_refuses_below_retained_limits() {
    let scan = inch_datum_plane(1.0);
    retained_boundary_sweep(
        &[
            "creo datum plane surface identity",
            "creo datum plane object identity",
        ],
        |ctx| {
            transfer_datum_plane_surfaces(
                ctx,
                &scan,
                &mut CadIr::empty(),
                &mut cadmpeg_ir::AnnotationBuilder::new(),
                &mut SourceUnitCarriers::default(),
            )
        },
    );
}

#[test]
fn placed_plane_identity_and_overflow_text_refuse_below_retained_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

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
    let scan = scan_bytes_ok(crate::test_support::build_prt(
        "plane",
        &[("ND:0:VisibGeom:0", payload)],
    ));
    retained_boundary_sweep(
        &[
            "creo placed plane surface identity",
            "creo placed plane object identity",
        ],
        |ctx| {
            transfer_placed_plane_surfaces_into_ir(
                ctx,
                &scan,
                &mut CadIr::empty(),
                &mut cadmpeg_ir::AnnotationBuilder::new(),
                &mut SourceUnitCarriers::default(),
            )
        },
    );

    let a = f64::from_bits(0x5fed_817d_bb14_96d1);
    let b = f64::from_bits(0x5fd8_c57e_64a4_a42f);
    let mut overflow = scan_bytes_ok(crate::test_support::build_prt("plane", &[]));
    overflow.planes.local_systems.push(PlaneLocalSystem {
        surface_id: 17,
        body: Vec::new(),
        slots: [a, b, 0.0, -b, a, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0].map(Some),
        layout: Some(PlaneSupportFrameLayout::SupportTriples),
        classification: LocalSystemClassification::Simple,
        row_offset: 0,
        offset: 0,
    });

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = transfer_placed_plane_surfaces_into_ir(
        &ctx,
        &overflow,
        &mut CadIr::empty(),
        &mut cadmpeg_ir::AnnotationBuilder::new(),
        &mut SourceUnitCarriers::default(),
    )
    .expect_err("overflow refusal text exceeds cap");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.operation == "creo plane overflow refusal text"));
}

fn inch_strip(positions: Vec<[f64; 3]>) -> ContainerScan<'static> {
    let mut scan = scan_bytes_ok(crate::test_support::build_prt("strip", &[]));
    scan.framing.principal_unit = Some(PrincipalUnitSystem::InchPoundMassSecond);
    scan.primitives
        .triangle_strips
        .push(crate::decode::with_test_decode_ctx(|ctx| PrimitiveTriangleStrip::new(ctx, 0,
            positions.into_iter().map(|position| FiniteVector::new(position).expect("finite strip position")).collect(),
            None, vec![3])).expect("service strip").expect("valid strip"));
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
        scan.primitives.triangle_strips[0].positions().next().expect("first vertex").get(),
        [1.0, 0.0, 0.0]
    );
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
fn display_strip_allocations_refuse_at_each_collection_boundary() {
    let scan = inch_strip(vec![[1.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 4.0]]);
    retained_boundary_sweep(&["creo display tessellation identity"], |ctx| {
        transfer_display_tessellations(
            ctx,
            &scan,
            &mut CadIr::empty(),
            &mut cadmpeg_ir::AnnotationBuilder::new(),
        )
    });
    collection_boundary_sweep(
        &[
            "creo display tessellation positions",
            "creo display tessellation strip rows",
            "creo display tessellation strip vertices",
            "creo model tessellations",
        ],
        |ctx| {
            transfer_display_tessellations(
                ctx,
                &scan,
                &mut CadIr::empty(),
                &mut cadmpeg_ir::AnnotationBuilder::new(),
            )
        },
    );
    let mut shaded = inch_strip(vec![[1.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 4.0]]);
    shaded.primitives.triangle_strips[0] = crate::decode::with_test_decode_ctx(|ctx| PrimitiveTriangleStrip::new(ctx, 0,
        shaded.primitives.triangle_strips[0].positions().copied().collect(),
        Some(vec![FiniteVector::new([0.0,0.0,1.0]).expect("finite normal");3]), vec![3])).expect("service").expect("valid strip");
    collection_boundary_sweep(
        &[
            "creo display tessellation positions",
            "creo display tessellation shaded rows",
            "creo display tessellation strip rows",
            "creo display tessellation strip vertices",
            "creo model tessellations",
        ],
        |ctx| {
            transfer_display_tessellations(
                ctx,
                &shaded,
                &mut CadIr::empty(),
                &mut cadmpeg_ir::AnnotationBuilder::new(),
            )
        },
    );
    let mut ir = CadIr::empty();
    crate::decode::with_test_decode_ctx(|ctx| {
        transfer_display_tessellations(
            ctx,
            &shaded,
            &mut ir,
            &mut cadmpeg_ir::AnnotationBuilder::new(),
        )
        .expect("shaded strip transfer");
    });
    assert!(matches!(
        ir.model.tessellations[0].mesh(),
        cadmpeg_ir::tessellation::TessellationMesh::ShadedStrips { .. }
    ));
    assert_eq!(ir.model.tessellations[0].vertex_normals().len(), 3);
}

#[test]
fn display_strip_error_text_refuses_below_retained_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    crate::decode::with_test_decode_ctx(|ctx| {
        assert!(PrimitiveTriangleStrip::new(ctx, 0, vec![FiniteVector::new([1.0,0.0,0.0]).expect("finite");3], None, vec![2]).expect("service").is_none());
    });
    let overflow = inch_strip(vec![[f64::MAX, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 4.0]]);
    let arena = DecodeArena::new();
    for (scan, operation, expected) in [
        (&overflow, "creo display tessellation overflow text",
            "SolidPrimdata display triangle strip at byte 0 has a vertex that cannot be represented in millimeters"),
    ] {
        let mut observed = false;
        let mut service_error = None;
        for limit in 0..2048 {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
                .expect("empty root admitted");
            match transfer_display_tessellations(&ctx, scan, &mut CadIr::empty(),
                &mut cadmpeg_ir::AnnotationBuilder::new()) {
                Err(CodecError::ResourceLimit(resource)) => {
                    assert_eq!(resource.dimension, ResourceDimension::RetainedBytes);
                    observed |= resource.operation == operation;
                }
                Err(error) => {
                    service_error = Some(error);
                    break;
                }
                Ok(()) => panic!("invalid display strip was admitted"),
            }
        }
        assert!(observed, "missing below-limit refusal for {operation}");
        match service_error.expect("malformed strip fits within scanned cap") {
            CodecError::Malformed(message) | CodecError::NotImplemented(message) => {
                assert_eq!(message, expected);
            }
            error => panic!("unexpected display strip result: {error:?}"),
        }
    }
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
    let carriers = crate::decode::with_test_decode_ctx(|ctx| {
        crate::decode::analytic::carriers::placed_carriers(ctx, &scan, &ir, &source_carriers)
    })
    .expect("service placed carriers");
    let Some(crate::decode::analytic::equations::CarrierEquation::Plane(source_plane)) =
        carriers.get(&18)
    else {
        panic!("placed plane was lost before native topology transfer");
    };
    assert_eq!(source_plane.origin, [1.0, 0.0, 0.0]);
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
    let carriers = crate::decode::with_test_decode_ctx(|ctx| {
        crate::decode::analytic::carriers::placed_carriers(ctx, &scan, &ir, &source_carriers)
    })
    .expect("service placed carriers");
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
    scan.planes.datums.push(crate::datum::DatumPlaneRecord::new(5, 1, crate::datum::DatumPlane::new(crate::axis::Axis::X, offset).expect("valid datum fixture"), offset, [[Some(0.0); 2]; 2], 0).expect("valid datum fixture"));
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
            center: cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 0.0, 0.0))
                .expect("finite center"),
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
