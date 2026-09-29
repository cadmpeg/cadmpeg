// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::directory::UseFlag;
use std::collections::BTreeSet;
use std::io::Cursor;

use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::math::Vector3;

use super::{
    base_geometry_line_font_valid, base_geometry_use_flag_valid, consumed_support_sequences,
    declared_affine_progression, enforce_transform_depth, is_finite_nonzero_vector,
    normal_matches_plane, plane_coordinates, source_object, validate_declared_transform_frame,
    DeclaredInterval, DeclaredTransformFrameError, ProjectionOutcome, WireProjectionOutcome,
};

fn assert_geometry_collection_refusal(bytes: &[u8], operation: &str) {
    use cadmpeg_core::decode::DecodePolicy;
    use cadmpeg_ir::codec::DecodeFailure;
    let mut cap = 0_u64;
    for _ in 0..4096 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        match crate::IgesCodec.decode(
            &mut Cursor::new(bytes),
            &DecodeOptions {
                policy,
                ..DecodeOptions::default()
            },
        ) {
            Err(DecodeFailure::Codec(CodecError::ResourceLimit(limit))) => {
                assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                if limit.operation == operation {
                    return;
                }
                cap = limit.used.checked_add(limit.additional).unwrap();
            }
            other => panic!("expected geometry collection refusal at {operation}: {other:?}"),
        }
    }
    panic!("geometry collection refusal was not reached: {operation}");
}

#[test]
fn merged_trimming_vertex_derivations_refuse_before_accumulator_growth() {
    let bytes = crate::test_support::test_surface_fixtures::bounded_plane_file();
    assert_geometry_collection_refusal(&bytes, "iges merged boundary vertex derivations");
    assert!(crate::IgesCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .is_ok());
}

#[test]
fn primitive_identity_copy_refuses_retained_budget_before_model_insertion() {
    use cadmpeg_core::decode::DecodePolicy;
    use cadmpeg_ir::codec::DecodeFailure;

    let bytes = crate::test_support::test_curves_and_surfaces::point_file();
    let mut cap = 0_u64;
    for _ in 0..4096 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = cap;
        match crate::IgesCodec.decode(
            &mut Cursor::new(&bytes),
            &DecodeOptions {
                policy,
                ..DecodeOptions::default()
            },
        ) {
            Err(DecodeFailure::Codec(CodecError::ResourceLimit(limit))) => {
                assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
                if limit.operation == "iges geometry neutral identity copy" {
                    crate::IgesCodec
                        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                        .unwrap();
                    return;
                }
                cap = limit.used.checked_add(limit.additional).unwrap();
            }
            other => panic!("expected primitive identity refusal: {other:?}"),
        }
    }
    panic!("primitive identity copy was not reached");
}

#[test]
fn nurbs_projection_refuses_source_lanes_neutral_slots_and_decoded_node() {
    let bytes = crate::test_support::test_curves_and_surfaces::rational_nurbs_curve_file();
    for operation in [
        "iges NURBS source knots",
        "iges NURBS admitted knots",
        "iges NURBS source weights",
        "iges NURBS positive weights",
        "iges NURBS source poles",
        "iges NURBS source range",
        "iges NURBS placed controls",
        "iges NURBS plane controls",
        "iges NURBS neutral weights",
        "iges NURBS neutral point slots",
        "iges NURBS neutral vertex slots",
        "iges NURBS neutral curve slots",
        "iges NURBS neutral edge slots",
        "iges NURBS wire edge slots",
        "iges NURBS decoded sequences",
    ] {
        assert_geometry_collection_refusal(&bytes, operation);
    }
    crate::IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
}

#[test]
fn circular_arc_projection_refuses_neutral_slots_and_decoded_node() {
    let bytes = crate::test_support::test_curves_and_surfaces::circular_arc_file();
    for operation in [
        "iges circle neutral point slots",
        "iges circle neutral vertex slots",
        "iges circle neutral curve slots",
        "iges circle neutral edge slots",
        "iges circle wire edge slots",
        "iges circle decoded sequences",
    ] {
        assert_geometry_collection_refusal(&bytes, operation);
    }
    crate::IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
}

#[test]
fn point_flash_and_line_projection_refuse_neutral_slots() {
    let point = crate::test_support::test_curves_and_surfaces::point_file();
    for operation in [
        "iges point neutral point slots",
        "iges point neutral vertex slots",
        "iges point free vertex slots",
        "iges point decoded sequences",
    ] {
        assert_geometry_collection_refusal(&point, operation);
    }
    let flash = crate::test_support::test_owned::owned_test_file(&[
        crate::test_support::test_owned::OwnedTestEntity {
            entity_type: 125,
            form: 0,
            label: "FLASH".into(),
            status: "00000000",
            parameters: "125,1,2,0,0,0;".into(),
        },
    ]);
    for operation in [
        "iges entity loss slots",
        "iges flash neutral point slots",
        "iges flash neutral vertex slots",
        "iges flash free vertex slots",
        "iges flash decoded sequences",
    ] {
        assert_geometry_collection_refusal(&flash, operation);
    }
    let line = crate::test_support::test_curves_and_surfaces::line_file(0);
    for operation in [
        "iges line neutral curve slots",
        "iges line neutral point slots",
        "iges line neutral vertex slots",
        "iges line neutral edge slots",
        "iges line wire edge slots",
        "iges line decoded sequences",
    ] {
        assert_geometry_collection_refusal(&line, operation);
    }
    for bytes in [&point, &flash, &line] {
        crate::IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
    }
}

#[test]
fn free_wire_topology_refuses_nested_lists_and_model_slots() {
    let bytes = crate::test_support::test_curves_and_surfaces::line_file(0);
    for operation in [
        "iges free wire body regions",
        "iges free wire body slots",
        "iges free wire region shells",
        "iges free wire region slots",
        "iges free wire shell slots",
    ] {
        assert_geometry_collection_refusal(&bytes, operation);
    }
    let service = crate::IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(service.ir().model.bodies.len(), 1);
    assert_eq!(service.ir().model.regions.len(), 1);
    assert_eq!(service.ir().model.shells.len(), 1);
}

#[test]
fn projector_merges_refuse_decoded_loss_and_wire_growth() {
    use crate::loss::IgesLossCode;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut decoded = BTreeSet::new();
    let mut losses = Vec::new();
    let result = ProjectionOutcome {
        decoded: BTreeSet::from([1]),
        losses: Vec::new(),
    }
    .merge_into(&mut decoded, &mut losses, &ctx);
    assert!(
        matches!(result, Err(CodecError::ResourceLimit(limit)) if limit.operation == "iges merged decoded sequences")
    );

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = ProjectionOutcome {
        decoded: BTreeSet::new(),
        losses: vec![IgesLossCode::EntityNotProjected.note("test")],
    }
    .merge_into(&mut decoded, &mut losses, &ctx);
    assert!(
        matches!(&result, Err(CodecError::ResourceLimit(limit)) if limit.operation == "iges merged loss slots"),
        "{result:?}"
    );

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut wire_edges = Vec::new();
    let edge = crate::ids::edge(&crate::ids::Stem::directory(1_u32));
    let result = WireProjectionOutcome {
        decoded: BTreeSet::new(),
        losses: Vec::new(),
        wire_edges: vec![edge],
    }
    .merge_into(&mut decoded, &mut losses, &mut wire_edges, &ctx);
    assert!(
        matches!(result, Err(CodecError::ResourceLimit(limit)) if limit.operation == "iges merged wire edge slots")
    );
}

#[test]
fn analytic_location_vertex_index_refuses_collection_limit() {
    use cadmpeg_core::decode::DecodePolicy;
    use cadmpeg_ir::codec::DecodeFailure;
    let bytes = crate::test_support::test_curves_and_surfaces::conic_arc_file(
        0,
        b"104,0.25,0,1,0,0,-1,0,2,0,0,1;",
    );
    let service = crate::IgesCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(service.ir().model.points.len(), 2);
    let mut cap = 0_u64;
    for _ in 0..4096 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        match crate::IgesCodec.decode(
            &mut Cursor::new(&bytes),
            &DecodeOptions {
                policy,
                ..DecodeOptions::default()
            },
        ) {
            Err(DecodeFailure::Codec(CodecError::ResourceLimit(limit))) => {
                assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                if limit.operation == "iges analytic-surface vertex point index" {
                    return;
                }
                let next = limit.used.checked_add(limit.additional).unwrap();
                assert!(next > cap, "limit did not advance from {cap}: {limit:?}");
                cap = next;
            }
            other => panic!("did not reach vertex point index at cap {cap}: {other:?}"),
        }
    }
    panic!("did not reach vertex point index within 4096 admission boundaries");
}

#[test]
fn source_sequence_maps_refuse_nodes_and_copied_keys() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let stem = crate::ids::Stem::directory(1_u32);
    for (kind, cap, operation) in [
        ("body", 0, "iges source body sequences"),
        ("body", 1, "iges source neutral body forms"),
        ("face", 0, "iges source face sequences"),
        ("curve", 0, "iges source curve sequences"),
        ("surface", 0, "iges source surface sequences"),
        ("point", 0, "iges source point sequences"),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut sequences = super::SourceSequences::default();
        let result = match kind {
            "body" => sequences.record_body(&crate::ids::body(&stem), 1, &stem, Some(&ctx)),
            "face" => sequences.record_face(&crate::ids::face(&stem), 1, Some(&ctx)),
            "curve" => sequences.record_curve(&crate::ids::curve(&stem), 1, Some(&ctx)),
            "surface" => sequences.record_surface(&crate::ids::surface(&stem), 1, Some(&ctx)),
            "point" => sequences.record_point(&crate::ids::point(&stem), &stem, Some(&ctx)),
            _ => panic!("unsupported test kind"),
        };
        assert!(
            matches!(result, Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == operation)
        );
    }

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut sequences = super::SourceSequences::default();
    let result = sequences.record_point(&crate::ids::point(&stem), &stem, Some(&ctx));
    assert!(
        matches!(result, Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == "iges source sequence key")
    );

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let mut sequences = super::SourceSequences::default();
    let body = crate::ids::body(&stem);
    let face = crate::ids::face(&stem);
    let curve = crate::ids::curve(&stem);
    let surface = crate::ids::surface(&stem);
    let point = crate::ids::point(&stem);
    sequences.record_body(&body, 1, &stem, Some(&ctx)).unwrap();
    sequences.record_face(&face, 1, Some(&ctx)).unwrap();
    sequences.record_curve(&curve, 1, Some(&ctx)).unwrap();
    sequences.record_surface(&surface, 1, Some(&ctx)).unwrap();
    sequences.record_point(&point, &stem, Some(&ctx)).unwrap();
    assert_eq!(
        (
            sequences.body(&body),
            sequences.body_neutral_form(&body),
            sequences.face(&face),
            sequences.curve(&curve),
            sequences.surface(&surface),
            sequences.point(&point)
        ),
        (Some(1), Some(1), Some(1), Some(1), Some(1), Some(1))
    );
}

#[test]
fn composite_coplanarity_refuses_segment_work_active_nodes_and_depth() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_ir::geometry::{
        CompositeCurveSegment, CompositeCurveSegments, CompositeCurveTransition, Curve,
        CurveGeometry,
    };
    use cadmpeg_ir::ids::CurveId;
    use cadmpeg_ir::index::ModelIndex;
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::transform::Transform;
    use cadmpeg_ir::CadIr;

    let child_id = CurveId::mint("iges:model:curve#D1").unwrap();
    let child = Curve {
        id: child_id.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
    };
    let segments = CompositeCurveSegments::try_from(vec![CompositeCurveSegment {
        curve: child_id,
        same_sense: true,
        transition: CompositeCurveTransition::Continuous,
    }])
    .unwrap();
    let composite = SolvedCurveGeometry::Composite {
        segments,
        self_intersect: None,
    };
    let mut ir = CadIr::empty();
    ir.model.curves.push(child);
    let index = ModelIndex::new(&ir);
    let plane = (Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0));

    for (dimension, cap, operation) in [
        (
            ResourceDimension::CollectionItems,
            0,
            "iges coplanar active curves",
        ),
        (
            ResourceDimension::RetainedBytes,
            0,
            "iges coplanar active curve id",
        ),
        (
            ResourceDimension::WorkUnits,
            0,
            "iges coplanar composite segments",
        ),
        (
            ResourceDimension::RecursionDepth,
            1,
            "iges coplanar curve recursion",
        ),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = cap,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
            ResourceDimension::RecursionDepth => policy.limits.max_recursion_depth = cap,
            _ => panic!("unsupported test dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = super::curve_geometry_coplanar(
            &composite,
            &index,
            Transform::identity(),
            plane,
            0.001,
            &mut BTreeSet::new(),
            Some(&ctx),
        );
        assert!(
            matches!(result, Err(CodecError::ResourceLimit(limit)) if limit.dimension == dimension && limit.operation == operation)
        );
    }
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert!(super::curve_geometry_coplanar(
        &composite,
        &index,
        Transform::identity(),
        plane,
        0.001,
        &mut BTreeSet::new(),
        Some(&ctx),
    )
    .unwrap());
}

#[test]
fn source_object_fields_refuse_retained_limits_before_copy() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let mut entry = transform_entry(1, 0);
    entry.label = *b"HELLO   ";
    entry.level = 7;
    for (cap, operation) in [
        (0, "iges source object ID"),
        (2, "iges source object name"),
        (7, "iges source object layer"),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = source_object(&entry, Some(&ctx)).unwrap_err();
        assert!(
            matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == operation)
        );
    }
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let source = source_object(&entry, Some(&ctx)).unwrap();
    assert_eq!(source.object_id.as_str(), "D1");
    assert_eq!(source.name.as_deref(), Some("HELLO"));
    assert_eq!(source.layer.as_deref(), Some("7"));
}

#[test]
fn plane_coordinates_refuse_collection_limit_before_projection_array() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_ir::math::Point3;
    let points = [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)];
    let plane = (points[0], Vector3::new(0.0, 0.0, 1.0));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = plane_coordinates(&points, plane, &ctx).unwrap_err();
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "iges plane coordinates")
    );

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert_eq!(
        plane_coordinates(&points, plane, &ctx).unwrap(),
        Some(vec![[0.0, 0.0], [0.0, -1.0]])
    );
}

#[test]
fn consumed_support_indexes_refuse_collection_limits_before_insert() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let directory = [transform_entry(1, 0), transform_entry(3, 1)];
    let records = std::collections::BTreeMap::new();
    for (cap, operation) in [
        (0, "iges consumed-support directory index"),
        (2, "iges consumed-support transforms"),
        (3, "iges consumed-support closure"),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = consumed_support_sequences(&directory, &records, &ctx).unwrap_err();
        assert!(
            matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == operation)
        );
    }
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert_eq!(
        consumed_support_sequences(&directory, &records, &ctx).unwrap(),
        [1].into()
    );
}
use crate::global::GlobalTable;
use crate::loss::IgesLossCode;
use crate::test_support::test_curves_and_surfaces::{
    circular_arc_file, direction_file, equal_weight_rational_nurbs_curve_file, line_file,
    nurbs_curve_file, polynomial_nurbs_curve_file, rational_nurbs_curve_file,
    transformed_circular_arc_file, transformed_circular_arc_file_with_form,
    transformed_circular_arc_file_with_global,
};
use crate::test_support::test_drawing_and_trimming::test_surface_domains::nested_transformed_point_file;
use crate::test_support::test_owned::{
    owned_test_file, owned_test_file_with_global, owned_test_file_with_global_and_line_fonts,
    OwnedTestEntity,
};
use crate::IgesCodec;

const EPS_RADIUS_COMPARISON: f64 = 1.0e-12;

#[test]
fn plane_normal_match_uses_direction_on_both_sides() {
    let normal = Vector3::new(0.0, 0.0, 1.0);
    assert!(!normal_matches_plane(normal, Vector3::new(0.0, 0.0, 0.0)));
    assert!(normal_matches_plane(
        normal,
        Vector3::new(0.0, 0.0, 1.0e-300)
    ));
    assert!(normal_matches_plane(
        normal,
        Vector3::new(0.0, 0.0, 1.0e300)
    ));
    assert!(!normal_matches_plane(normal, Vector3::new(1.0, 0.0, 0.0)));
}

fn transform_entry(sequence: u32, transform: i64) -> crate::directory::DirectoryEntry {
    crate::directory::DirectoryEntry {
        source_offset: 0,
        sequence,
        entity_type: 124,
        parameter_start: 0,
        structure: 0,
        line_font: 0,
        level: 0,
        view: 0,
        transform,
        label_display: 0,
        status: crate::directory::SourceStatus::from_codes([0, 0, 0, 0]),
        line_weight: 0,
        color: 0,
        parameter_line_count: 0,
        form: 0,
        reserved: [[b' '; 8]; 2],
        label: [b' '; 8],
        subscript: 0,
    }
}

#[test]
fn point_display_symbol_targets_follow_the_declared_dialect() {
    assert!(super::point_display_symbol_type_allowed(
        408,
        GlobalTable::V4_0
    ));
    assert!(!super::point_display_symbol_type_allowed(
        308,
        GlobalTable::V4_0
    ));
    assert!(super::point_display_symbol_type_allowed(
        308,
        GlobalTable::V5_0
    ));
    assert!(!super::point_display_symbol_type_allowed(
        408,
        GlobalTable::V5_0
    ));
    assert!(super::point_display_symbol_type_allowed(
        308,
        GlobalTable::Legacy
    ));
    assert!(super::point_display_symbol_type_allowed(
        408,
        GlobalTable::Legacy
    ));
}

#[test]
fn entity_use_flag_six_is_admitted_only_by_the_later_profile() {
    let global_v4 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let global_v5 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";
    let file = |global: &[u8]| {
        owned_test_file_with_global(
            &[OwnedTestEntity {
                entity_type: 116,
                form: 0,
                label: "CONSTR".into(),
                status: "00000600",
                parameters: "116,1,2,3,0;".into(),
            }],
            global,
        )
    };

    let v4 = IgesCodec
        .decode(&mut Cursor::new(file(global_v4)), &DecodeOptions::default())
        .unwrap();
    assert!(v4.ir().model.points.is_empty());
    assert!(v4.report().losses.iter().any(|loss| {
        loss.code == IgesLossCode::EntityNotProjected.kind()
            && loss.message.contains("Entity Use Flag 06 is outside")
    }));

    let v5 = IgesCodec
        .decode(&mut Cursor::new(file(global_v5)), &DecodeOptions::default())
        .unwrap();
    assert_eq!(v5.ir().model.points.len(), 1);
    assert!(!v5.report().losses.iter().any(|loss| {
        loss.code == IgesLossCode::EntityNotProjected.kind()
            && loss.message.contains("Entity Use Flag 06 is outside")
    }));
}

#[test]
fn base_geometry_line_font_follows_the_declared_dialect() {
    for entity_type in [
        100, 104, 108, 110, 112, 114, 118, 120, 122, 126, 128, 130, 140, 142, 144,
    ] {
        assert!(!base_geometry_line_font_valid(
            entity_type,
            0,
            0,
            GlobalTable::V4_0
        ));
        assert!(base_geometry_line_font_valid(
            entity_type,
            0,
            1,
            GlobalTable::V4_0
        ));
    }
    assert!(base_geometry_line_font_valid(106, 1, 0, GlobalTable::V4_0));
    assert!(base_geometry_line_font_valid(106, 3, 0, GlobalTable::V4_0));
    for form in [11, 12, 13, 63] {
        assert!(!base_geometry_line_font_valid(
            106,
            form,
            0,
            GlobalTable::V4_0
        ));
    }
    assert!(base_geometry_line_font_valid(116, 0, 0, GlobalTable::V4_0));
    assert!(base_geometry_line_font_valid(110, 0, 0, GlobalTable::V5_0));
}

#[test]
fn base_geometry_use_flag_follows_the_declared_dialect() {
    for use_flag in [0, 1, 2, 5] {
        assert!(base_geometry_use_flag_valid(
            110,
            0,
            UseFlag::parse(use_flag, crate::global::GlobalTable::V5Later).unwrap(),
            GlobalTable::V4_0
        ));
    }
    for use_flag in [3, 4] {
        assert!(!base_geometry_use_flag_valid(
            110,
            0,
            UseFlag::parse(use_flag, crate::global::GlobalTable::V5Later).unwrap(),
            GlobalTable::V4_0
        ));
    }
    assert!(base_geometry_use_flag_valid(
        110,
        0,
        UseFlag::parse(3, crate::global::GlobalTable::V5Later).unwrap(),
        GlobalTable::V5_0
    ));
    assert!(!base_geometry_use_flag_valid(
        116,
        0,
        UseFlag::parse(3, crate::global::GlobalTable::V5Later).unwrap(),
        GlobalTable::V4_0
    ));
    assert!(base_geometry_use_flag_valid(
        125,
        0,
        UseFlag::parse(3, crate::global::GlobalTable::V5Later).unwrap(),
        GlobalTable::V4_0
    ));
}

#[test]
fn decode_rejects_a_zero_v4_base_geometry_line_font() {
    const GLOBAL_V4: &[u8] = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file_with_global(
                &[OwnedTestEntity {
                    entity_type: 110,
                    form: 0,
                    label: "LINE".into(),
                    status: "00000000",
                    parameters: "110,0,0,0,1,0,0;".into(),
                }],
                GLOBAL_V4,
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result.ir().model.curves.is_empty());
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == IgesLossCode::EntityNotProjected.kind()
            && loss
                .message
                .contains("Line Font must be nonzero for this IGES 4.0 geometry entity")
    }));
}

#[test]
fn decode_applies_v4_base_geometry_use_flag_03_by_dialect() {
    let global_v4 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let global_v5 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file_with_global_and_line_fonts(
                &[OwnedTestEntity {
                    entity_type: 110,
                    form: 0,
                    label: "LINE".into(),
                    status: "00000300",
                    parameters: "110,0,0,0,1,0,0;".into(),
                }],
                global_v4,
                &[(1, 1)],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result.ir().model.curves.is_empty());
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == IgesLossCode::EntityNotProjected.kind()
            && loss.message.contains(
                "Entity Use Flag 03 is outside the IGES 4.0 base geometry values 00, 01, 02, and 05",
            )
    }));

    let later = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file_with_global_and_line_fonts(
                &[OwnedTestEntity {
                    entity_type: 110,
                    form: 0,
                    label: "LINE".into(),
                    status: "00000300",
                    parameters: "110,0,0,0,1,0,0;".into(),
                }],
                global_v5,
                &[(1, 1)],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(later.ir().model.curves.len(), 1);
    assert!(!later.report().losses.iter().any(|loss| {
        loss.code == IgesLossCode::EntityNotProjected.kind()
            && loss.message.contains("base geometry values")
    }));
}

#[test]
fn point_display_symbol_pointer_targets_follow_the_declared_dialect() {
    fn file(global: &[u8], pointer: u32) -> Vec<u8> {
        owned_test_file_with_global(
            &[
                OwnedTestEntity {
                    entity_type: 308,
                    form: 0,
                    label: "DEF".into(),
                    status: "00000200",
                    parameters: "308,0,3HDEF,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 408,
                    form: 0,
                    label: "INST".into(),
                    status: "00000000",
                    parameters: "408,1,0,0,0,1;".into(),
                },
                OwnedTestEntity {
                    entity_type: 116,
                    form: 0,
                    label: "POINT".into(),
                    status: "00000000",
                    parameters: format!("116,1,2,3,{pointer};"),
                },
            ],
            global,
        )
    }

    let global_v4 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let global_v5 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";

    let v4 = IgesCodec
        .decode(
            &mut Cursor::new(file(global_v4, 3)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(v4
        .ir()
        .model
        .points
        .iter()
        .any(|point| point.id.as_str() == "iges:model:point#D5"));
    assert!(!v4.report().losses.iter().any(|loss| {
        loss.message
            .contains("Type 116 display symbol pointer is invalid")
    }));

    let v5 = IgesCodec
        .decode(
            &mut Cursor::new(file(global_v5, 1)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(v5
        .ir()
        .model
        .points
        .iter()
        .any(|point| point.id.as_str() == "iges:model:point#D5"));
    assert!(!v5.report().losses.iter().any(|loss| {
        loss.message
            .contains("Type 116 display symbol pointer is invalid")
    }));

    for (global, pointer) in [(&global_v4[..], 1), (&global_v5[..], 3)] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(file(global, pointer)),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert!(result
            .ir()
            .model
            .points
            .iter()
            .any(|point| point.id.as_str() == "iges:model:point#D5"));
        assert!(result.report().losses.iter().any(|loss| {
            loss.code == IgesLossCode::DisplayDataNotProjected.kind()
                && loss
                    .message
                    .contains("Type 116 display symbol pointer is invalid")
        }));
    }
}

#[test]
fn type125_flash_forms_project_reference_points_and_retain_shape_parameters() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 125,
                    form: 0,
                    label: "FLASH0".into(),
                    status: "00000000",
                    parameters: "125,1,2,0,0,0,11;".into(),
                },
                OwnedTestEntity {
                    entity_type: 125,
                    form: 1,
                    label: "FLASH1".into(),
                    status: "00000000",
                    parameters: "125,3,4,10,0,0,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 125,
                    form: 2,
                    label: "FLASH2".into(),
                    status: "00000000",
                    parameters: "125,5,6,10,20,0.5,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 125,
                    form: 3,
                    label: "FLASH3".into(),
                    status: "00000000",
                    parameters: "125,7,8,30,10,0,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 125,
                    form: 4,
                    label: "FLASH4".into(),
                    status: "00000000",
                    parameters: "125,9,10,40,20,0.75,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 100,
                    form: 0,
                    label: "DEFINER".into(),
                    status: "00000000",
                    parameters: "100,0,0,0,1,0,0,1;".into(),
                },
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(
        result
            .ir()
            .model
            .points
            .iter()
            .filter(|point| {
                matches!(
                    point.id.as_str(),
                    "iges:model:point#D1"
                        | "iges:model:point#D3"
                        | "iges:model:point#D5"
                        | "iges:model:point#D7"
                        | "iges:model:point#D9"
                )
            })
            .count(),
        5
    );
    for (sequence, x, y) in [
        (1, 1.0, 2.0),
        (3, 3.0, 4.0),
        (5, 5.0, 6.0),
        (7, 7.0, 8.0),
        (9, 9.0, 10.0),
    ] {
        let point = result
            .ir()
            .model
            .points
            .iter()
            .find(|point| point.id.as_str() == format!("iges:model:point#D{sequence}"))
            .unwrap();
        assert_eq!(
            point.position().get(),
            cadmpeg_ir::math::Point3::new(x, y, 0.0)
        );
    }
    let flashes = &result.ir().native.namespace("iges").unwrap().arenas()["flashes"];
    assert_eq!(flashes.len(), 5);
    assert_eq!(flashes[0].fields()["form"], 0);
    assert_eq!(
        flashes[0].fields()["reference_entity"],
        "iges:entity:directory#11"
    );
    assert_eq!(flashes[2].fields()["dimension_1"], 10.0);
    assert_eq!(flashes[2].fields()["dimension_2"], 20.0);
    assert_eq!(flashes[2].fields()["rotation"], 0.5);
    assert!(
        result.report().losses.is_empty(),
        "{:?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn type125_flash_is_admitted_in_v4_and_v5() {
    let global_v4 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let global_v5 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";
    for global in [global_v4.as_slice(), global_v5.as_slice()] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(owned_test_file_with_global(
                    &[OwnedTestEntity {
                        entity_type: 125,
                        form: 2,
                        label: "FLASH".into(),
                        status: "00000000",
                        parameters: "125,3,4,10,20,0.5,0;".into(),
                    }],
                    global,
                )),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert_eq!(result.ir().model.points.len(), 1);
        assert_eq!(
            result.ir().native.namespace("iges").unwrap().arenas()["flashes"].len(),
            1
        );
        assert!(!result.report().losses.iter().any(|loss| {
            loss.code == IgesLossCode::EntityOutsideEnvelope.kind()
                || loss.code == IgesLossCode::EntityNotProjected.kind()
        }));
    }
}

#[test]
fn type125_form0_without_defining_entity_reports_display_loss() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[OwnedTestEntity {
                entity_type: 125,
                form: 0,
                label: "FLASH".into(),
                status: "00000000",
                parameters: "125,1,2,0,0,0;".into(),
            }])),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.points.len(), 1);
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == IgesLossCode::DisplayDataNotProjected.kind()
            && loss
                .message
                .contains("Type 125 Form 0 has no defining entity pointer")
    }));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn transform_depth_overflow_is_a_structured_resource_refusal() {
    let transform_count = 65_u32;
    let mut directory = (0..transform_count)
        .map(|index| {
            let sequence = 1 + index * 2;
            let transform = if index + 1 < transform_count {
                sequence + 2
            } else {
                0
            };
            transform_entry(sequence, i64::from(transform))
        })
        .collect::<Vec<_>>();
    directory.push(transform_entry(1 + transform_count * 2, 1));

    let error = enforce_transform_depth(&directory, None).unwrap_err();
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::Codec("iges_transform_depth")
                && limit.limit == 64
                && limit.used == 64
                && limit.additional == 1
    ));
}

#[test]
fn transform_preflight_admits_directory_index_and_walk_path() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let directory = [transform_entry(1, 3), transform_entry(3, 0)];
    for (cap, operation) in [
        (0, "iges transform preflight directory index"),
        (2, "iges transform preflight path"),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = enforce_transform_depth(&directory, Some(&ctx));
        assert!(matches!(
            result,
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.used == cap
                    && limit.additional == 1
                    && limit.operation == operation
        ));
    }

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert!(enforce_transform_depth(&directory, Some(&ctx)).is_ok());
}

#[test]
fn decode_preserves_rational_bspline_weights_and_multiplicities() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(rational_nurbs_curve_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let Some(SolvedCurveGeometry::Nurbs(nurbs)) = result.ir().model.curves[0].geometry.solved()
    else {
        panic!("expected a NURBS carrier");
    };
    assert_eq!(nurbs.degree(), 2);
    assert_eq!(nurbs.knots().as_slice(), [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    assert_eq!(nurbs.pole_rows().weights(), Some(vec![1.0, 0.5, 1.0]));
    assert_eq!(
        cadmpeg_ir::eval::nurbs_curve_point_at(nurbs, 0.5)
            .ok()
            .map(cadmpeg_ir::features::FinitePoint3::get),
        Some(cadmpeg_ir::math::Point3::new(1.0, 1.0 / 3.0, 0.0))
    );
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_rejects_a_rational_declaration_with_equal_weights() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(equal_weight_rational_nurbs_curve_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result.ir().model.curves.is_empty());
    assert_eq!(result.report().losses.len(), 1);
    assert_eq!(
        result.report().losses[0].code,
        IgesLossCode::EntityNotProjected.kind()
    );
}

#[test]
fn decode_rejects_inconsistent_type_126_planar_and_closed_flags() {
    for (flags, normal) in [
        ([1, 0, 0, 0], [1, 0, 0]),
        ([1, 1, 0, 0], [0, 0, 1]),
        ([0, 0, 0, 0], [0, 0, 0]),
    ] {
        let parameters = format!(
            "126,2,2,{},{},{},{},0,0,0,1,1,1,1,0.5,1,0,0,0,1,1,0,2,0,0,0,1,{},{},{};",
            flags[0], flags[1], flags[2], flags[3], normal[0], normal[1], normal[2]
        );
        let result = IgesCodec
            .decode(
                &mut Cursor::new(polynomial_nurbs_curve_file(parameters.as_bytes())),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert!(
            result.ir().model.curves.is_empty(),
            "flags={flags:?}: losses={:?}",
            result.report().losses
        );
        assert_eq!(result.report().losses.len(), 1, "flags={flags:?}");
        assert_eq!(
            result.report().losses[0].code,
            IgesLossCode::EntityNotProjected.kind(),
            "flags={flags:?}"
        );
    }
}

#[test]
fn decode_uses_strict_global_resolution_for_type_126_closed_flag() {
    for (endpoint, prop2, decoded) in [
        ("0.000999", 1, true),
        ("0.001", 0, true),
        ("0.001", 1, false),
        ("0.001001", 0, true),
    ] {
        let parameters =
            format!("126,1,1,1,{prop2},1,0,0,0,1,1,1,1,0,0,0,{endpoint},0,0,0,1,0,0,1;");
        let result = IgesCodec
            .decode(
                &mut Cursor::new(polynomial_nurbs_curve_file(parameters.as_bytes())),
                &DecodeOptions::default(),
            )
            .unwrap();

        assert_eq!(
            result.ir().model.curves.len(),
            usize::from(decoded),
            "endpoint={endpoint}, PROP2={prop2}"
        );
        assert_eq!(
            result.report().losses.is_empty(),
            decoded,
            "endpoint={endpoint}, PROP2={prop2}: {:?}",
            result.report().losses
        );
        if !decoded {
            assert_eq!(
                result.report().losses[0].code,
                IgesLossCode::EntityNotProjected.kind()
            );
        }
    }
}

#[test]
fn declared_transform_validation_separates_frame_and_handedness_invariants() {
    let intervals = |rows: [[f64; 3]; 3]| {
        std::array::from_fn::<_, 9, _>(|index| {
            DeclaredInterval::around(rows[index / 3][index % 3], 0.0)
        })
    };

    assert_eq!(
        validate_declared_transform_frame(
            intervals([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]),
            1.0,
        ),
        Ok(())
    );
    assert_eq!(
        validate_declared_transform_frame(
            intervals([[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]),
            -1.0,
        ),
        Ok(())
    );
    assert_eq!(
        validate_declared_transform_frame(
            intervals([[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]),
            1.0,
        ),
        Err(DeclaredTransformFrameError::WrongDeterminant)
    );
    assert_eq!(
        validate_declared_transform_frame(
            intervals([[1.1, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]),
            1.0,
        ),
        Err(DeclaredTransformFrameError::NotOrthonormal)
    );
}

#[test]
fn declared_intervals_prove_or_reject_an_affine_control_polygon() {
    assert!(declared_affine_progression(
        &[0.0, 1.0, 2.0, 3.0],
        &[0.0; 4]
    ));
    assert!(declared_affine_progression(
        &[0.0, 1.000_002, 2.000_004, 3.0],
        &[0.0, 5.0e-6, 5.0e-6, 0.0]
    ));
    assert!(!declared_affine_progression(
        &[0.0, 1.0, 2.2, 3.0],
        &[0.0; 4]
    ));
}

#[test]
fn type_123_accepts_a_finite_non_unit_direction() {
    assert!(is_finite_nonzero_vector(Vector3::new(2.0, -3.0, 4.0)));
    assert!(!is_finite_nonzero_vector(Vector3::new(0.0, 0.0, 0.0)));

    let result = IgesCodec
        .decode(
            &mut Cursor::new(direction_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let direction = &result.ir().native.namespace("iges").unwrap().arenas()["directions"][0];
    assert_eq!(
        direction.fields()["components"],
        serde_json::json!([2.0, -3.0, 4.0])
    );
    assert_eq!(result.report().losses.len(), 1);
    assert_eq!(
        result.report().losses[0].code,
        IgesLossCode::EntityRetainedUnprojected.kind()
    );
}

#[test]
fn decode_treats_type_126_periodic_flag_as_evaluation_metadata() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(polynomial_nurbs_curve_file(
                b"126,2,2,1,0,1,1,0,0,0,1,1,1,1,1,1,0,0,0,1,1,0,2,0,0,0,1,0,0,1;",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.curves.len(), 1);
    assert!(result.report().losses.is_empty());
    let Some(SolvedCurveGeometry::Nurbs(nurbs)) = result.ir().model.curves[0].geometry.solved()
    else {
        panic!("expected a NURBS carrier");
    };
    assert!(!nurbs.periodic());
}

#[test]
fn decode_rejects_type_126_without_required_normal_fields() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(polynomial_nurbs_curve_file(
                b"126,1,1,1,0,1,0,0,0,1,1,1,1,0,0,0,2,0,0,0,1;",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result.ir().model.curves.is_empty());
    assert_eq!(result.report().losses.len(), 1);
    assert_eq!(
        result.report().losses[0].code,
        IgesLossCode::EntityNotProjected.kind()
    );
}

#[test]
fn decode_accepts_omitted_type_126_normal_for_nonplanar_v4_and_v5() {
    let global_v4 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let global_v5 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";
    let parameters = "126,3,1,0,0,1,0,0,0,1,2,3,3,1,1,1,1,0,0,0,1,0,0,1,1,0,0,1,1,0,3,,,;";

    for global in [&global_v4[..], &global_v5[..]] {
        let file = owned_test_file_with_global_and_line_fonts(
            &[OwnedTestEntity {
                entity_type: 126,
                form: 0,
                label: "NURBS".into(),
                status: "00000000",
                parameters: parameters.into(),
            }],
            global,
            &[(1, 1)],
        );
        let result = IgesCodec
            .decode(&mut Cursor::new(file), &DecodeOptions::default())
            .unwrap();

        assert_eq!(result.ir().model.curves.len(), 1);
        assert!(!result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
    }
}

#[test]
fn decode_projects_a_bounded_polynomial_bspline_curve() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(nurbs_curve_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let Some(SolvedCurveGeometry::Nurbs(nurbs)) = result.ir().model.curves[0].geometry.solved()
    else {
        panic!("expected a NURBS carrier");
    };
    assert_eq!(nurbs.degree(), 1);
    assert_eq!(nurbs.knots().as_slice(), [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(nurbs.control_points().len(), 2);
    assert_eq!(nurbs.weights(), None);
    assert!(!nurbs.periodic());
    assert_eq!(
        cadmpeg_ir::eval::nurbs_curve_point_at(nurbs, 0.5)
            .ok()
            .map(cadmpeg_ir::features::FinitePoint3::get),
        Some(cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0))
    );
    assert_eq!(
        result.ir().model.edges[0]
            .param_range()
            .map(cadmpeg_ir::units::FiniteVector::get),
        Some([0.0, 1.0])
    );
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_projects_a_degree_zero_polynomial_bspline_curve() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(polynomial_nurbs_curve_file(
                b"126,0,0,0,1,1,0,0,1,1,1,2,3,0,1;",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    let Some(SolvedCurveGeometry::Nurbs(nurbs)) = result.ir().model.curves[0].geometry.solved()
    else {
        panic!("expected a NURBS carrier");
    };
    assert_eq!(nurbs.degree(), 0);
    assert_eq!(nurbs.knots().as_slice(), [0.0, 1.0]);
    assert_eq!(nurbs.control_points().len(), 1);
    assert_eq!(nurbs.weights(), None);
    assert_eq!(
        cadmpeg_ir::eval::nurbs_curve_point_at(nurbs, 0.5)
            .ok()
            .map(cadmpeg_ir::features::FinitePoint3::get),
        Some(cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0))
    );
    assert_eq!(
        result.ir().model.edges[0]
            .param_range()
            .map(cadmpeg_ir::units::FiniteVector::get),
        Some([0.0, 1.0])
    );
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_applies_declared_real_significance_to_polynomial_weights() {
    for (weights, decoded) in [
        ("1.,0.9999999", true),
        ("1.,0.99", false),
        ("1.D0,0.9999999D0", false),
    ] {
        let parameters = format!("126,1,1,1,0,1,0,0,0,1,1,{weights},0,0,0,2,0,0,0,1,0,0,1;");
        let result = IgesCodec
            .decode(
                &mut Cursor::new(polynomial_nurbs_curve_file(parameters.as_bytes())),
                &DecodeOptions::default(),
            )
            .unwrap();

        assert_eq!(
            result.ir().model.curves.len(),
            usize::from(decoded),
            "{weights}"
        );
        assert_eq!(result.report().losses.is_empty(), decoded, "{weights}");
        if decoded {
            let Some(SolvedCurveGeometry::Nurbs(nurbs)) =
                result.ir().model.curves[0].geometry.solved()
            else {
                panic!("expected a NURBS carrier");
            };
            assert_eq!(nurbs.weights(), None);
        } else {
            assert!(result.report().losses[0]
                .message
                .contains("polynomial spline has unequal weights"));
        }
    }
}

#[test]
fn decode_clamps_bspline_parameter_range_within_declared_real_significance() {
    for (range_start, decoded) in [("0.12345695", true), ("0.12", false)] {
        let parameters =
            format!("126,1,1,1,0,1,0,0.123457,0.123457,1,1,1,1,0,0,0,2,0,0,{range_start},1,0,0,1;");
        let result = IgesCodec
            .decode(
                &mut Cursor::new(polynomial_nurbs_curve_file(parameters.as_bytes())),
                &DecodeOptions::default(),
            )
            .unwrap();

        assert_eq!(
            result.ir().model.edges.len(),
            usize::from(decoded),
            "{range_start}"
        );
        if decoded {
            assert_eq!(
                result.ir().model.edges[0]
                    .param_range()
                    .map(cadmpeg_ir::units::FiniteVector::get),
                Some([0.123_457, 1.0])
            );
            assert!(result.report().losses.is_empty());
        } else {
            assert!(result.report().losses[0]
                .message
                .contains("parameter range lies outside the spline knot domain"));
        }
    }
}

#[test]
fn decode_projects_a_counterclockwise_circular_arc() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(circular_arc_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.curves.len(), 1);
    let Some(SolvedCurveGeometry::Circle(circle_curve)) =
        result.ir().model.curves[0].geometry.solved()
    else {
        panic!("expected a circle carrier");
    };
    let center = circle_curve.center().get();
    let axis = circle_curve.frame().axis().as_raw();
    let ref_direction = circle_curve.frame().reference().as_raw();
    let radius = circle_curve.radius().get();
    assert_eq!(center, cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0));
    assert_eq!(*axis, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(
        *ref_direction,
        cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)
    );
    assert_eq!(radius, 1.0);
    assert_eq!(
        result.ir().model.edges[0]
            .param_range()
            .map(cadmpeg_ir::units::FiniteVector::get),
        Some([0.0, std::f64::consts::FRAC_PI_2])
    );
    assert!(result
        .ir()
        .model
        .points
        .iter()
        .any(|point| point.position().get() == cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0)));
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_accepts_rounded_transformed_circular_arc_frame() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(transformed_circular_arc_file(
                b"124,1.0000049,0,0,0,0,1,0,0,0,0,1,0;",
                b"100,0,0,0,1,0,0,1;",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    let Some(SolvedCurveGeometry::Circle(circle_curve)) =
        result.ir().model.curves[0].geometry.solved()
    else {
        panic!("expected a circle carrier");
    };
    let radius = circle_curve.radius().get();
    assert!((radius - 1.0).abs() < EPS_RADIUS_COMPARISON);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_rejects_transform_roundoff_beyond_its_declared_precision() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(transformed_circular_arc_file(
                b"124,1.0000051,0,0,0,0,1,0,0,0,0,1,0;",
                b"100,0,0,0,1,0,0,1;",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result.ir().model.curves.is_empty());
    assert!(result.report().losses.iter().any(|loss| {
        loss.message
            .contains("not orthonormal within its declared numeric precision")
    }));
}

#[test]
fn decode_applies_declared_double_precision_to_transform_coefficients() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(transformed_circular_arc_file(
                b"124,.8D0,-.6000001D0,0,0,.6D0,.8D0,0,0,0,0,1,0;",
                b"100,0,0,0,1,0,0,1;",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result.ir().model.curves.is_empty());
    assert!(result.report().losses.iter().any(|loss| {
        loss.message
            .contains("not orthonormal within its declared numeric precision")
    }));
}

#[test]
fn decode_canonicalizes_a_rounded_left_handed_transform() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(transformed_circular_arc_file_with_form(
                1,
                b"124,.7071068,-.7071068,0,0,.7071068,.7071068,0,0,0,0,-1,0;",
                b"100,0,0,0,1,0,0,1;",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    let Some(SolvedCurveGeometry::Circle(circle_curve)) =
        result.ir().model.curves[0].geometry.solved()
    else {
        panic!("expected a circle carrier");
    };
    let axis = circle_curve.frame().axis().as_raw();
    let radius = circle_curve.radius().get();
    assert_eq!(*axis, cadmpeg_ir::math::Vector3::new(0.0, -0.0, 1.0));
    assert_eq!(radius, 1.0);
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_accepts_arc_endpoints_within_model_resolution() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(transformed_circular_arc_file(
                b"124,1,0,0,0,0,1,0,0,0,0,1,0;",
                b"100,0,0,0,16,0,0,16.000999;",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    let Some(SolvedCurveGeometry::Circle(circle_curve)) =
        result.ir().model.curves[0].geometry.solved()
    else {
        panic!("expected a circle carrier");
    };
    let radius = circle_curve.radius().get();
    assert!((radius - 16.0).abs() < EPS_RADIUS_COMPARISON);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_rejects_arc_endpoints_beyond_model_resolution() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(transformed_circular_arc_file(
                b"124,1,0,0,0,0,1,0,0,0,0,1,0;",
                b"100,0,0,0,16,0,0,16.001001;",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result.ir().model.curves.is_empty());
    assert!(result.report().losses.iter().any(|loss| {
        loss.message
            .contains("arc start and terminate points have different radii")
    }));
}

#[test]
fn decode_projects_a_line_as_a_normalized_bounded_wire_edge() {
    let result = IgesCodec
        .decode(&mut Cursor::new(line_file(0)), &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.curves.len(), 1);
    assert_eq!(result.ir().model.edges.len(), 1);
    assert_eq!(result.ir().model.points.len(), 2);
    let Some(SolvedCurveGeometry::Line(line_curve)) = result.ir().model.curves[0].geometry.solved()
    else {
        panic!("expected a line carrier");
    };
    let origin = line_curve.origin().get();
    let direction = *line_curve.direction().as_raw();
    assert_eq!(origin, cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0));
    assert_eq!(direction, cadmpeg_ir::math::Vector3::new(0.6, 0.8, 0.0));
    assert_eq!(
        result.ir().model.edges[0]
            .param_range()
            .map(cadmpeg_ir::units::FiniteVector::get),
        Some([0.0, 5.0])
    );
    assert_eq!(result.ir().model.shells[0].wire_edges().len(), 1);
    assert!(result.ir().model.shells[0].free_vertices().is_empty());
    assert_eq!(
        result.ir().model.curves[0]
            .source_object
            .as_ref()
            .unwrap()
            .object_id,
        "D1"
    );
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_preserves_semi_bounded_and_unbounded_line_domains_natively() {
    for form in [1, 2] {
        let result = IgesCodec
            .decode(&mut Cursor::new(line_file(form)), &DecodeOptions::default())
            .unwrap();

        assert_eq!(result.ir().model.curves.len(), 1);
        assert!(result.ir().model.edges.is_empty());
        assert!(result.ir().model.bodies.is_empty());
        assert_eq!(
            result.ir().model.curves[0]
                .source_object
                .as_ref()
                .unwrap()
                .object_id,
            "D1"
        );
        assert!(result.report().losses.is_empty());
        let native = result.ir().native.namespace("iges").unwrap();
        assert_eq!(native.arenas()["entities"][0].fields()["form"], form);
        let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
            .expect("resource allocation did not fail");
        assert!(validation.is_ok(), "{:#?}", validation.findings);
    }
}

#[test]
fn decode_applies_nested_transforms_reflection_units_and_model_scale_once() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(nested_transformed_point_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.points.len(), 1);
    assert_eq!(result.ir().model.points[0].position().get().x, 0.0);
    assert_eq!(result.ir().model.points[0].position().get().y, 80.0);
    assert_eq!(result.ir().model.points[0].position().get().z, 60.0);
    assert_eq!(
        result.ir().native.namespace("iges").unwrap().arenas()["transformations"].len(),
        2
    );
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn transform_translation_overflow_after_inch_scaling_is_rejected() {
    use crate::parameter::{ParameterRecord, Token, TokenValue};
    use std::collections::{BTreeMap, BTreeSet};
    let entry = transform_entry(1, 0);
    let values = [
        124.0,
        1.0,
        0.0,
        0.0,
        f64::MAX,
        0.0,
        1.0,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        0.0,
    ];
    let record = ParameterRecord::from_test_tokens(
        1,
        1..2,
        Vec::new(),
        values.len(),
        values
            .into_iter()
            .map(|value| Token {
                value: TokenValue::real(value),
                span: 0..0,
            })
            .collect(),
        Vec::new(),
    );
    let result = super::resolve_transform(
        1,
        &BTreeMap::from([(1, &entry)]),
        &BTreeMap::from([(1, &record)]),
        25.4,
        crate::global::RealPrecision {
            single_significance: 6,
            double_significance: 15,
        },
        &mut BTreeSet::new(),
        None,
    );
    assert!(result.is_err());
}

#[test]
fn transform_failures_render_original_diagnostics_without_allocating_early() {
    use super::TransformFailure;

    let cases = [
        (TransformFailure::Literal("transformation chain is cyclic"), "transformation chain is cyclic"),
        (TransformFailure::Depth, "transformation chain exceeds 64 entities"),
        (TransformFailure::MissingEntry(7), "transformation D7 is missing"),
        (TransformFailure::WrongTypeForm { sequence: 7, entity_type: 123, form: 2 }, "transformation D7 is type 123 form 2, expected defining type 124 form 0 or 1"),
        (TransformFailure::MissingParameters(7), "transformation D7 parameters are missing"),
        (TransformFailure::NonNumericCoefficient { sequence: 7, index: 12 }, "transformation D7 coefficient 12 is not numeric"),
        (TransformFailure::NonFiniteCoefficient(7), "transformation D7 has a non-finite coefficient"),
        (TransformFailure::NotOrthonormal(7), "transformation D7 linear part is not orthonormal within its declared numeric precision"),
        (TransformFailure::WrongDeterminant { sequence: 7, form: 1 }, "transformation D7 determinant disagrees with form 1 within its declared numeric precision"),
        (TransformFailure::FirstAxis(7), "transformation D7 first axis cannot be normalized"),
        (TransformFailure::SecondAxis(7), "transformation D7 second axis cannot be normalized"),
        (TransformFailure::NonFiniteScaled(7), "transformation D7 has non-finite coefficients after length scaling"),
        (TransformFailure::NonFiniteComposed(7), "transformation D7 has non-finite coefficients after composition"),
    ];
    for (reason, expected) in cases {
        assert_eq!(reason.to_string(), expected);
    }
}

#[test]
fn transform_chain_path_refuses_collection_limit_before_insertion() {
    use crate::parameter::{ParameterRecord, Token, TokenValue};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use std::collections::{BTreeMap, BTreeSet};

    let identity_record = |sequence| {
        let values = [
            124.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0,
        ];
        ParameterRecord::from_test_tokens(
            sequence,
            1..2,
            Vec::new(),
            values.len(),
            values
                .into_iter()
                .map(|value| Token {
                    value: TokenValue::real(value),
                    span: 0..0,
                })
                .collect(),
            Vec::new(),
        )
    };
    let parent = transform_entry(1, 0);
    let child = transform_entry(3, 1);
    let parent_record = identity_record(1);
    let child_record = identity_record(3);
    let entries = BTreeMap::from([(1, &parent), (3, &child)]);
    let records = BTreeMap::from([(1, &parent_record), (3, &child_record)]);
    let precision = crate::global::RealPrecision {
        single_significance: 6,
        double_significance: 15,
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = super::resolve_transform(
        3,
        &entries,
        &records,
        1.0,
        precision,
        &mut BTreeSet::new(),
        Some(&ctx),
    );
    assert!(matches!(
        result,
        Err(super::TransformResolutionError::Resource(
            cadmpeg_core::CodecError::ResourceLimit(limit)
        )) if limit.dimension == ResourceDimension::CollectionItems
            && limit.used == 1
            && limit.additional == 1
    ));

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert!(super::resolve_transform(
        3,
        &entries,
        &records,
        1.0,
        precision,
        &mut BTreeSet::new(),
        Some(&ctx),
    )
    .is_ok());
}

#[test]
fn affine_composition_rejects_translation_overflow() {
    let transform = cadmpeg_ir::transform::Transform::affine([
        [1.0, 0.0, 0.0, f64::MAX],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .unwrap();
    assert!(transform.compose(transform).is_err());
}

#[test]
fn decode_reports_transform_translation_overflow_after_inch_scaling() {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,1,2HIN,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let bytes = transformed_circular_arc_file_with_global(
        0,
        b"124,1,0,0,1.7D308,0,1,0,0,0,0,1,0;",
        b"100,0,0,0,1,0,0,1;",
        global,
    );
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(result.ir().model.curves.is_empty());
    assert!(result.report().losses.iter().any(|loss| loss
        .message
        .contains("non-finite coefficients after length scaling")));
}
