// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_test_support::wire;
use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::build_prt;
use crate::CreoCodec;

#[test]
fn tabulated_cylinder_refusals_charge_text_and_loss_rows() {
    let records = ["missing chart".to_string()];
    for (retained_limit, item_limit, dimension, operation) in [
        (
            0,
            u64::MAX,
            ResourceDimension::RetainedBytes,
            "creo tabulated cylinder refusal text",
        ),
        (
            u64::MAX,
            0,
            ResourceDimension::CollectionItems,
            "creo tabulated cylinder losses",
        ),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = retained_limit;
        policy.limits.max_collection_items = item_limit;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
        let error = super::note_tabulated_cylinder_refusals(
            &ctx,
            7,
            42,
            "directrix",
            &records,
            &mut Vec::new(),
        )
        .expect_err("one refusal exceeds the limit");
        assert!(matches!(error, CodecError::ResourceLimit(resource)
            if resource.dimension == dimension && resource.operation == operation));
    }
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service root admitted");
    let mut losses = Vec::new();
    super::note_tabulated_cylinder_refusals(&ctx, 7, 42, "directrix", &records, &mut losses)
        .expect("service refusal admitted");
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].message,
        "VisibGeom surface row 7 states a tabulated-cylinder replay at offset 42 whose directrix lane forms no carrier: missing chart");
}

fn scan_with_tabulated_replay() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::test_support::empty_container_scan();
    scan.curves
        .tabulated_cylinder_replays
        .push(crate::surface::TabulatedCylinderCurveReplay {
            body: Vec::new(),
            surface_id: 7,
            curve_id: 9,
            curve_type: 0x13,
            flip: 1,
            tangent_condition: 0,
            degree: 3,
            parameter_body: Vec::new(),
            control_point_ids: [1, 2, 3, 4],
            successor_reference: 5,
            control_point_bodies: std::array::from_fn(|_| Vec::new()),
            control_points: [None; 4],
            terminal_reference: 6,
            offset: 0,
            surface_row_offset: 0,
        });
    scan
}

#[test]
fn line_extrusion_replay_set_refuses_before_node_insertion() {
    let scan = scan_with_tabulated_replay();
    let run = |limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("test decode context");
        super::transfer_positional_line_extrusion_planes(
            &ctx,
            &scan,
            &mut cadmpeg_ir::document::CadIr::empty(),
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
    };
    assert_eq!(run(1).expect("service admits replay set"), 0);
    assert!(matches!(
        run(0),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo line-extrusion replay surface ids"
    ));
}

#[test]
fn tabulated_replay_counts_refuse_before_node_insertion() {
    let scan = scan_with_tabulated_replay();
    let run = |limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("test decode context");
        super::transfer_tabulated_cylinder_spline_extrusions(
            &ctx,
            &scan,
            &mut cadmpeg_ir::document::CadIr::empty(),
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
            &mut Vec::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
    };
    assert_eq!(run(1).expect("service admits replay count"), 0);
    assert!(matches!(
        run(0),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo tabulated-cylinder replay counts"
    ));
}

fn positional_round_result(limit: u64) -> Result<usize, CodecError> {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.rows.push(crate::feature::rows::FeatureRow {
        feature_id: 913,
        root_schema_class: Some(crate::feature::schema::SchemaClass::Round),
        stream_offset: 0,
        body: vec![0; 2].try_into().expect("row body"),
        body_offset: 0,
        offset: 0,
    });
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 7,
        kind: crate::surface::SurfaceKind::TorusOrSphere,
        feature_id: 913,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 7,
    });
    scan.features
        .legacy_rounds
        .push(crate::legacy_feature::LegacyRoundFeature {
            feature_id: 913,
            radius: crate::legacy_feature::LegacyRoundRadius::Constant(
                cadmpeg_ir::scalar::PositiveReal::new(2.0).expect("positive radius"),
            ),
            edge_ids: None,
            offset: 0,
        });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("root input is admitted");
    super::transfer_positional_tori(
        &ctx,
        &scan,
        &mut cadmpeg_ir::document::CadIr::empty(),
        &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
        &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
    )
}

#[test]
fn positional_torus_round_feature_node_refuses_before_insertion() {
    assert_eq!(positional_round_result(2).expect("service admits round"), 0);
    let error = positional_round_result(0).expect_err("round feature needs a set node");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo positional torus round feature ids"
    ));
}

#[test]
fn positional_torus_constant_round_node_refuses_before_insertion() {
    let error = positional_round_result(1).expect_err("constant round follows feature node");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo positional torus constant round ids"
    ));
}

#[test]
fn paired_sphere_association_copy_refuses_before_vec_growth() {
    let mut payload = b"srf_array\0\xf8\x01".to_vec();
    payload.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&[
        0x18, 0x0d, 0x41, 0xcf, 0xff, 0xff, 0xff, 0xe5, 0x79, 0x7b, 0x0e, 0x29, 0xdf, 0xff,
    ]);
    payload.push(0xe3);
    crate::test_support::push_named_analytic_prototype(
        &mut payload,
        "torus",
        &[("radius1", 1.0), ("radius2", 2.0)],
    );
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");
    let scan = crate::container::scan_bytes_ok(build_prt(
        "paired-sphere-association-limit",
        &[("ND:0:VisibGeom:0", payload)],
    ));
    let run = |limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("root input is admitted");
        super::transfer_paired_envelope_spheres(
            &ctx,
            &scan,
            &mut cadmpeg_ir::document::CadIr::empty(),
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
    };
    assert_eq!(run(37).expect("service limit admits the association"), 0);
    let error = run(36).expect_err("the copied association follows two discovery charges");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo paired sphere associations"
    ));
}

#[test]
fn unresolved_round_type26_frames_are_not_admitted_as_constant_tori() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.rows.push(crate::feature::rows::FeatureRow {
        feature_id: 913,
        root_schema_class: Some(crate::feature::schema::SchemaClass::Round),
        stream_offset: 0,
        body: vec![0; 2].try_into().expect("row body"),
        body_offset: 0,
        offset: 0,
    });
    scan.surfaces.rows.extend([
        crate::surface::SurfaceRow {
            id: 7,
            kind: crate::surface::SurfaceKind::TorusOrSphere,
            feature_id: 913,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: 7,
        },
        crate::surface::SurfaceRow {
            id: 8,
            kind: crate::surface::SurfaceKind::TorusOrSphere,
            feature_id: 913,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: 8,
        },
    ]);
    let parameter = |surface_id, minor_radius| crate::surface::SurfaceParameterRecord {
        surface_id,
        body: Vec::new(),
        scalar_tokens: Vec::new(),
        opaque_spans: Vec::new(),
        scalar_frames: Vec::new(),
        carrier: crate::surface::SurfaceParameterCarrier::Resolved(
            crate::surface::InlineSurfaceCarrier::Torus(
                crate::surface::PositionalTorusFrame::new(
                    [0.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0],
                    [1.0, 0.0, 0.0],
                    5.0,
                    minor_radius,
                )
                .expect("valid positional torus frame"),
            ),
        ),
        boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
        offset: usize::try_from(surface_id).expect("fixture index fits usize"),
        body_offset: usize::try_from(surface_id).expect("fixture index fits usize"),
    };
    scan.surfaces
        .parameters
        .extend([parameter(7, 1.0), parameter(8, 2.0)]);
    let mut ir = cadmpeg_ir::document::CadIr::empty();

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| super::transfer_positional_tori(
            ctx,
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        ))
        .expect("valid source object identity"),
        0
    );
    assert!(ir.model.surfaces.is_empty());
}

#[test]
fn transfers_an_exact_zero_major_inline_frame_as_a_sphere() {
    let mut payload = b"srf_array\0\xf8\x01".to_vec();
    payload.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0, 0xe3]);
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");
    let mut scan = crate::container::scan_bytes_ok(build_prt(
        "inline-sphere",
        &[("ND:0:VisibGeom:0", payload)],
    ));
    assert_eq!(scan.surfaces.rows.len(), 1);
    assert_eq!(scan.surfaces.parameters.len(), 1);
    scan.surfaces.parameters[0].carrier = crate::surface::SurfaceParameterCarrier::Resolved(
        crate::surface::InlineSurfaceCarrier::Torus(
            crate::surface::PositionalTorusFrame::new(
                [2.0, 2.0, 4.0],
                [0.0, 0.0, 1.0],
                [-1.0, 0.0, 0.0],
                0.0,
                2.0,
            )
            .expect("valid positional torus frame"),
        ),
    );
    let mut ir = cadmpeg_ir::document::CadIr::empty();

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| super::transfer_positional_tori(
            ctx,
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        ))
        .expect("valid source object identity"),
        1
    );
    let Some(SolvedSurfaceGeometry::Sphere(sphere_surface)) =
        ir.model.surfaces[0].geometry.solved()
    else {
        panic!("zero-major positional frame must transfer as a sphere");
    };
    let radius = sphere_surface.radius().get();
    assert_eq!(radius, 2.0);
}

#[test]
fn positional_sphere_is_in_millimeters_at_ir_admission() {
    let mut payload = b"srf_array\0\xf8\x01".to_vec();
    payload.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0, 0xe3]);
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");
    let mut scan = crate::container::scan_bytes_ok(build_prt(
        "inline-sphere",
        &[("ND:0:VisibGeom:0", payload)],
    ));
    scan.surfaces.parameters[0].carrier = crate::surface::SurfaceParameterCarrier::Resolved(
        crate::surface::InlineSurfaceCarrier::Torus(
            crate::surface::PositionalTorusFrame::new(
                [1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
                [1.0, 0.0, 0.0],
                0.0,
                2.0,
            )
            .expect("valid positional sphere frame"),
        ),
    );
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let mut source_carriers = crate::decode::source_carriers::SourceUnitCarriers::new(
        cadmpeg_ir::scalar::PositiveReal::new(25.4),
    );
    crate::decode::with_test_decode_ctx(|ctx| {
        super::transfer_positional_tori(
            ctx,
            &scan,
            &mut ir,
            &mut cadmpeg_ir::AnnotationBuilder::new(),
            &mut source_carriers,
        )
        .expect("positional sphere transfer");
    });
    let surface = ir.model.surfaces.first().expect("positional sphere");
    let Some(SolvedSurfaceGeometry::Sphere(sphere)) = surface.geometry.solved() else {
        panic!("positional sphere changed family");
    };
    assert_eq!(sphere.center().get(), [25.4, 0.0, 0.0].into());
    assert_eq!(sphere.radius().get(), 50.8);
    let cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(source)) =
        source_carriers.surface_geometry(surface)
    else {
        panic!("source positional sphere changed family");
    };
    assert_eq!(source.radius().get(), 2.0);
}

#[test]
fn paired_envelope_spheres_do_not_join_rows_from_neighboring_surface_frames() {
    let lower = [
        0x18, 0x18, 0x01, 0x11, 0x2e, 0xb0, 0x12, 0x47, 0x05, 0x33, 0x2d, 0x2d, 0xff, 0xff, 0xff,
        0xff, 0xff, 0x29, 0x47, 0x05, 0x33, 0x2e, 0x05, 0x33, 0x2d, 0x31, 0xa6, 0x66, 0x66, 0x66,
        0x66, 0x66, 0x18,
    ];
    let upper = [
        0x18, 0x18, 0x01, 0x11, 0x2e, 0xb8, 0x12, 0x47, 0x05, 0x33, 0x2d, 0x28, 0xb3, 0x33, 0x33,
        0x33, 0x33, 0x33, 0x47, 0x05, 0x33, 0x2e, 0x05, 0x33, 0x2d, 0x2e, 0x00, 0x00, 0x00, 0x00,
        0x00, 0xd7, 0x18,
    ];
    let prototype = b"srf_prim_ptr(torus)\0\xe0\x01radius1\0\x18\xe0\x01radius2\0\x2e\x05\x33\xe3";
    let mut payload = Vec::new();
    payload.extend_from_slice(b"srf_array\0\xf8\x01");
    payload.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&lower);
    payload.push(0xe3);
    payload.extend_from_slice(prototype);
    payload.extend_from_slice(b"srf_array\0\xf8\x01");
    payload.extend_from_slice(&[8, 0x26, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&upper);
    payload.push(0xe3);
    payload.extend_from_slice(prototype);
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");

    let result = CreoCodec
        .decode(
            &mut Cursor::new(build_prt(
                "neighboring-frames",
                &[("ND:0:VisibGeom:0", payload)],
            )),
            &DecodeOptions::default(),
        )
        .expect("decode neighboring surface frames");

    assert_eq!(
        wire::coverage_count(
            result.report(),
            crate::coverage::TRANSFERRED_PAIRED_ENVELOPE_SPHERE_COUNT.as_str()
        ),
        0
    );
}

#[test]
fn paired_envelope_spheres_do_not_join_rows_from_two_prototypes_in_one_frame() {
    let lower = [
        0x18, 0x18, 0x01, 0x11, 0x2e, 0xb0, 0x12, 0x47, 0x05, 0x33, 0x2d, 0x2d, 0xff, 0xff, 0xff,
        0xff, 0xff, 0x29, 0x47, 0x05, 0x33, 0x2e, 0x05, 0x33, 0x2d, 0x31, 0xa6, 0x66, 0x66, 0x66,
        0x66, 0x66, 0x18,
    ];
    let upper = [
        0x18, 0x18, 0x01, 0x11, 0x2e, 0xb8, 0x12, 0x47, 0x05, 0x33, 0x2d, 0x28, 0xb3, 0x33, 0x33,
        0x33, 0x33, 0x33, 0x47, 0x05, 0x33, 0x2e, 0x05, 0x33, 0x2d, 0x2e, 0x00, 0x00, 0x00, 0x00,
        0x00, 0xd7, 0x18,
    ];
    let prototype = b"srf_prim_ptr(torus)\0\xe0\x01radius1\0\x18\xe0\x01radius2\0\x2e\x05\x33\xe3";
    let mut payload = Vec::new();
    payload.extend_from_slice(b"srf_array\0\xf8\x02");
    payload.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&lower);
    payload.push(0xe3);
    payload.extend_from_slice(prototype);
    payload.extend_from_slice(&[8, 0x26, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&upper);
    payload.push(0xe3);
    payload.extend_from_slice(prototype);
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");

    let data = build_prt("two-prototypes-one-frame", &[("ND:0:VisibGeom:0", payload)]);
    let scan = crate::container::scan_bytes_ok(data.clone());
    assert_eq!(scan.surfaces.rows.len(), 2);
    assert_eq!(scan.surfaces.parameters.len(), 2);
    assert_eq!(scan.surfaces.prototype_records.len(), 2);
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| {
            super::super::prototypes::unique_surface_prototype_associations(ctx, &scan)
        })
        .expect("prototype associations")
        .len(),
        2
    );

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode two prototypes in one surface frame");

    assert_eq!(
        wire::coverage_count(
            result.report(),
            crate::coverage::TRANSFERRED_PAIRED_ENVELOPE_SPHERE_COUNT.as_str()
        ),
        0
    );
}

fn construction_copy_scan(tabulated: bool) -> crate::container::ContainerScan<'static> {
    let mut scan = scan_with_tabulated_replay();
    if tabulated {
        scan.curves.tabulated_cylinder_replays[0].control_points = [
            Some([1.0, 2.0]),
            Some([2.0, 2.5]),
            Some([3.0, 3.5]),
            Some([4.0, 4.0]),
        ];
    } else {
        scan.curves.tabulated_cylinder_replays.clear();
    }
    scan.surfaces.rows.push(crate::decode::tests::surface_row(
        7,
        1,
        crate::surface::SurfaceKind::Extrusion(crate::surface::ExtrusionVariant::TabulatedCylinder),
    ));
    scan.surfaces.rows[0].offset = 0;
    let frame = |offset, values: &[f64]| crate::surface::SurfaceParameterScalarFrame {
        offset,
        slots: values
            .iter()
            .enumerate()
            .map(|(index, value)| crate::surface::SurfaceParameterScalar {
                value: Some(*value),
                raw: vec![0],
                offset: offset + index,
            })
            .collect(),
    };
    scan.surfaces
        .parameters
        .push(crate::surface::SurfaceParameterRecord {
            surface_id: 7,
            body: Vec::new(),
            scalar_tokens: Vec::new(),
            opaque_spans: vec![crate::surface::SurfaceParameterOpaqueSpan {
                raw: vec![0x00, 0x0c, 0x9a],
                offset: 3,
            }],
            scalar_frames: vec![
                frame(0, &[0.0, 0.0, 1.0]),
                frame(6, &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
            ],
            carrier: crate::surface::SurfaceParameterCarrier::Resolved(
                crate::surface::InlineSurfaceCarrier::Tabulated {
                    variant: crate::surface::ExtrusionVariant::TabulatedCylinder,
                    frame: crate::surface::TabulatedCylinderFrame::new(
                        [1.0, 2.0, 5.0, 4.0, 4.0, 10.0],
                        [0xa2, 0x42, 0x88, 0xa3, 0x18, 0x8a],
                    )
                    .expect("finite frame"),
                },
            ),
            boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
            offset: 0,
            body_offset: 0,
        });
    scan
}

#[test]
fn positional_line_extrusion_refuses_construction_identity_copies() {
    let scan = construction_copy_scan(false);
    let count = crate::test_support::assert_retained_boundaries(
        &[
            "creo construction curve identity copy",
            "creo construction surface identity copy",
        ],
        |ctx| {
            super::transfer_positional_line_extrusion_planes(
                ctx,
                &scan,
                &mut cadmpeg_ir::document::CadIr::empty(),
                &mut cadmpeg_ir::AnnotationBuilder::new(),
                &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
            )
        },
    );
    assert_eq!(count, 1);
}

#[test]
fn tabulated_extrusion_refuses_construction_identity_copies() {
    let scan = construction_copy_scan(true);
    let count = crate::test_support::assert_retained_boundaries(
        &[
            "creo construction curve identity copy",
            "creo construction surface identity copy",
        ],
        |ctx| {
            super::transfer_tabulated_cylinder_spline_extrusions(
                ctx,
                &scan,
                &mut cadmpeg_ir::document::CadIr::empty(),
                &mut cadmpeg_ir::AnnotationBuilder::new(),
                &mut Vec::new(),
                &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
            )
        },
    );
    assert_eq!(count, 1);
}
