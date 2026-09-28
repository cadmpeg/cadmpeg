// SPDX-License-Identifier: Apache-2.0

#![allow(clippy::unwrap_used)]

mod nurbs;

use crate::directory::UseFlag;

use std::io::Cursor;

use cadmpeg_core::decode::DecodeArena;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::decode::DecodePolicy;
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::Codec;
use cadmpeg_ir::codec::DecodeFailure;
use cadmpeg_ir::codec::DecodeOptions;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::Curve;
use cadmpeg_ir::geometry::CurveGeometry;
use cadmpeg_ir::geometry::ProceduralCurveDefinition;
use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::ids::EdgeId;
use cadmpeg_ir::ids::PointId;
use cadmpeg_ir::ids::VertexId;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::topology::Edge;
use cadmpeg_ir::topology::Point;
use cadmpeg_ir::topology::Vertex;
use cadmpeg_ir::CadIr;

use crate::loss::IgesLossCode;
use crate::test_support::test_curves_and_surfaces::composite_curve_file;
use crate::test_support::test_curves_and_surfaces::composite_curve_with_join_gap;
use crate::test_support::test_owned::owned_test_file;
use crate::test_support::test_owned::owned_test_file_with_global;
use crate::test_support::test_owned::owned_test_file_with_global_and_directory_fields;
use crate::test_support::test_owned::OwnedTestEntity;
use crate::test_support::test_solids_and_structure::explicit_tetrahedron_solid_file;
use crate::IgesCodec;

#[test]
fn composite_degradation_reasons_render_at_the_admitted_loss_boundary() {
    use super::{CompositeCurveError, CompositeRefusal, DegreeElevationError};

    assert_eq!(
        CompositeRefusal::NoChildCarrier.to_string(),
        "a child has no bounded line or NURBS carrier"
    );
    assert_eq!(
        CompositeRefusal::Child(CompositeCurveError::EmptyChildList).to_string(),
        "a child states no curve carrier: the composite states no child curve"
    );
    assert_eq!(
        CompositeRefusal::JoinedCarrier(CompositeCurveError::EmptyChildList).to_string(),
        "the joined children state no curve carrier: the composite states no child curve"
    );
    assert_eq!(
        CompositeRefusal::Elevation(CompositeCurveError::Elevation(
            DegreeElevationError::SpansDoNotJoin
        ))
        .to_string(),
        "a child does not raise to the composite degree: the elevated Bezier spans do not join"
    );
    assert_eq!(
        CompositeRefusal::Other(CompositeCurveError::EmptyChildList).to_string(),
        "the composite states no child curve"
    );
}

#[test]
fn composite_index_refuses_each_collection_before_insertion() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(explicit_tetrahedron_solid_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let ir = decoded.ir();
    assert!(!ir.model.curves.is_empty());
    assert!(!ir.model.edges.is_empty());
    assert!(!ir.model.vertices.is_empty());
    for operation in [
        "iges composite curve index nodes",
        "iges composite edge index nodes",
        "iges composite indexed edges",
        "iges composite point index nodes",
        "iges composite vertex index nodes",
    ] {
        let mut cap = 0_u64;
        let mut found = false;
        for _ in 0..4096 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            match CompositeIndex::from_ir(ir, Some(&ctx)) {
                Err(CodecError::ResourceLimit(limit)) => {
                    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                    if limit.operation == operation {
                        found = true;
                        break;
                    }
                    cap = limit.used.checked_add(limit.additional).unwrap();
                }
                Ok(_) => panic!("composite index succeeded before {operation}"),
                Err(error) => panic!("unexpected composite index failure at {operation}: {error}"),
            }
        }
        assert!(
            found,
            "composite index collection boundary was not reached: {operation}"
        );
    }
}

#[test]
fn composite_index_refuses_each_retained_identity_copy() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(explicit_tetrahedron_solid_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let ir = decoded.ir();
    for operation in [
        "iges composite curve index keys",
        "iges composite edge index keys",
        "iges composite indexed start ids",
        "iges composite indexed end ids",
        "iges composite point index keys",
        "iges composite vertex index keys",
    ] {
        let mut cap = 0_u64;
        let mut found = false;
        for _ in 0..4096 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            match CompositeIndex::from_ir(ir, Some(&ctx)) {
                Err(CodecError::ResourceLimit(limit)) => {
                    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
                    if limit.operation == operation {
                        found = true;
                        break;
                    }
                    cap = limit.used.checked_add(limit.additional).unwrap();
                }
                Ok(_) => panic!("composite identity copy succeeded before {operation}"),
                Err(error) => {
                    panic!("unexpected composite identity copy failure at {operation}: {error}")
                }
            }
        }
        assert!(
            found,
            "composite retained boundary was not reached: {operation}"
        );
    }
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let index = CompositeIndex::from_ir(ir, Some(&ctx)).unwrap();
    assert_eq!(index.curve_positions.len(), ir.model.curves.len());
}

#[test]
fn composite_index_admits_added_entity_nodes_and_identity_keys() {
    let curve = CurveId::mint("test:model:curve#added").unwrap();
    let start = VertexId::mint("test:model:vertex#start").unwrap();
    let end = VertexId::mint("test:model:vertex#end").unwrap();
    let position = cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap();
    let add = |index: &mut CompositeIndex, ctx: &DecodeContext<'_>| {
        index.add_model_entity(
            &curve,
            0,
            super::CompositeEdge {
                start: start.clone(),
                end: end.clone(),
                param_range: None,
            },
            [(start.clone(), position), (end.clone(), position)],
            Some(ctx),
        )
    };
    for (dimension, operation) in [
        (
            ResourceDimension::RetainedBytes,
            "iges composite added curve index key",
        ),
        (
            ResourceDimension::CollectionItems,
            "iges composite added curve index node",
        ),
        (
            ResourceDimension::RetainedBytes,
            "iges composite added edge index key",
        ),
        (
            ResourceDimension::CollectionItems,
            "iges composite added edge index node",
        ),
        (
            ResourceDimension::CollectionItems,
            "iges composite added edge slots",
        ),
        (
            ResourceDimension::CollectionItems,
            "iges composite added vertex index nodes",
        ),
    ] {
        let mut cap = 0_u64;
        let mut found = false;
        for _ in 0..32 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = cap,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
                _ => panic!("unexpected resource dimension"),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut index = CompositeIndex::default();
            match add(&mut index, &ctx) {
                Err(CodecError::ResourceLimit(limit)) => {
                    assert_eq!(limit.dimension, dimension);
                    if limit.operation == operation {
                        found = true;
                        break;
                    }
                    cap = limit.used.checked_add(limit.additional).unwrap();
                }
                Ok(()) => panic!("composite index addition succeeded before {operation}"),
                Err(error) => panic!("unexpected composite index addition failure: {error}"),
            }
        }
        assert!(
            found,
            "composite index addition boundary was not reached: {operation}"
        );
    }
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let mut index = CompositeIndex::default();
    add(&mut index, &ctx).unwrap();
    assert_eq!(index.curve_positions.len(), 1);
    assert_eq!(index.edges[&curve].len(), 1);
    assert_eq!(index.vertex_points.len(), 2);
}

#[test]
fn composite_projection_refuses_model_and_procedural_slots_before_growth() {
    for (bytes, operation) in [
        (
            composite_curve_with_join_gap(0.001_001),
            "iges composite native point slots",
        ),
        (
            composite_curve_with_join_gap(0.001_001),
            "iges composite native vertex slots",
        ),
        (
            composite_curve_with_join_gap(0.001_001),
            "iges composite native curve slots",
        ),
        (
            composite_curve_with_join_gap(0.001_001),
            "iges composite native edge slots",
        ),
        (composite_curve_file(), "iges composite solved point slots"),
        (composite_curve_file(), "iges composite solved vertex slots"),
        (composite_curve_file(), "iges composite solved curve slots"),
        (composite_curve_file(), "iges composite solved edge slots"),
        (
            composite_curve_file(),
            "iges composite procedural boundaries",
        ),
        (
            composite_curve_file(),
            "iges composite procedural components",
        ),
        (
            composite_curve_file(),
            "iges composite procedural curve slots",
        ),
        (composite_curve_file(), "iges composite wire edge ids"),
    ] {
        let mut cap = 0_u64;
        let mut found = false;
        for _ in 0..4096 {
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            match IgesCodec.decode(
                &mut Cursor::new(&bytes),
                &DecodeOptions {
                    policy,
                    ..DecodeOptions::default()
                },
            ) {
                Err(DecodeFailure::Codec(CodecError::ResourceLimit(limit))) => {
                    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                    if limit.operation == operation {
                        found = true;
                        break;
                    }
                    cap = limit.used.checked_add(limit.additional).unwrap();
                }
                Ok(_) => panic!("composite projection succeeded before {operation}"),
                Err(error) => {
                    panic!("unexpected composite projection failure at {operation}: {error}")
                }
            }
        }
        assert!(
            found,
            "composite projection boundary was not reached: {operation}"
        );
    }
}

#[test]
fn native_composite_segment_curve_ids_refuse_retained_copy() {
    let bytes = composite_curve_with_join_gap(0.001_001);
    let mut cap = 0_u64;
    for _ in 0..4096 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = cap;
        match IgesCodec.decode(
            &mut Cursor::new(&bytes),
            &DecodeOptions {
                policy,
                ..DecodeOptions::default()
            },
        ) {
            Err(DecodeFailure::Codec(CodecError::ResourceLimit(limit))) => {
                assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
                if limit.operation == "iges composite native segment curve ids" {
                    return;
                }
                cap = limit.used.checked_add(limit.additional).unwrap();
            }
            Ok(_) => panic!("native segment copy succeeded before retained refusal"),
            Err(error) => panic!("unexpected native segment copy failure: {error}"),
        }
    }
    panic!("native composite segment retained boundary was not reached");
}

#[test]
fn composite_projection_identity_copies_refuse_retained_limit() {
    for bytes in [
        composite_curve_with_join_gap(0.001_001),
        composite_curve_file(),
    ] {
        IgesCodec
            .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
            .unwrap();
        let mut cap = 0_u64;
        let mut found = false;
        for _ in 0..4096 {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            match IgesCodec.decode(
                &mut Cursor::new(&bytes),
                &DecodeOptions {
                    policy,
                    ..DecodeOptions::default()
                },
            ) {
                Err(DecodeFailure::Codec(CodecError::ResourceLimit(limit))) => {
                    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
                    if limit.operation == "iges composite projection identity copy" {
                        found = true;
                        break;
                    }
                    cap = limit.used.checked_add(limit.additional).unwrap();
                }
                Ok(_) => panic!("composite identity copy succeeded before refusal at cap {cap}"),
                Err(error) => panic!("unexpected composite identity copy failure: {error}"),
            }
        }
        assert!(found, "composite projection identity copy was not reached");
    }
}

#[test]
fn composite_child_carriers_refuse_nested_collection_admission() {
    let bytes = composite_curve_file();
    for operation in [
        "iges composite child pointer slots",
        "iges composite child carrier nodes",
        "iges composite curve child sequences",
        "iges composite child curve ids",
        "iges composite line knots",
        "iges composite line points",
    ] {
        let mut cap = 0_u64;
        let mut found = false;
        for _ in 0..4096 {
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            match IgesCodec.decode(
                &mut Cursor::new(&bytes),
                &DecodeOptions {
                    policy,
                    ..DecodeOptions::default()
                },
            ) {
                Err(DecodeFailure::Codec(CodecError::ResourceLimit(limit))) => {
                    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                    if limit.operation == operation {
                        found = true;
                        break;
                    }
                    cap = limit.used.checked_add(limit.additional).unwrap();
                }
                Ok(_) => panic!("composite child admission succeeded before {operation}"),
                Err(error) => panic!("unexpected composite child admission failure: {error}"),
            }
        }
        assert!(
            found,
            "composite child collection boundary was not reached: {operation}"
        );
    }
}

#[test]
fn composite_child_curve_id_copies_refuse_retained_budget() {
    let bytes = composite_curve_file();
    for operation in [
        "iges composite child curve ID copies",
        "iges composite projected child curve IDs",
    ] {
        let mut cap = 0_u64;
        let mut found = false;
        for _ in 0..4096 {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            match IgesCodec.decode(
                &mut Cursor::new(&bytes),
                &DecodeOptions {
                    policy,
                    ..DecodeOptions::default()
                },
            ) {
                Err(DecodeFailure::Codec(CodecError::ResourceLimit(limit))) => {
                    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
                    if limit.operation == operation {
                        found = true;
                        break;
                    }
                    cap = limit.used.checked_add(limit.additional).unwrap();
                }
                Ok(_) => panic!("composite child identity copy succeeded before {operation}"),
                Err(error) => panic!("unexpected composite child identity failure: {error}"),
            }
        }
        assert!(
            found,
            "composite child retained boundary was not reached: {operation}"
        );
    }
}

#[test]
fn composite_source_object_refusal_survives_candidate_projection() {
    let bytes = composite_curve_file();
    let mut cap = 0_u64;
    let mut source_objects = 0;
    for _ in 0..4096 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = cap;
        let result = IgesCodec.decode(
            &mut Cursor::new(&bytes),
            &DecodeOptions {
                policy,
                ..DecodeOptions::default()
            },
        );
        match result {
            Err(DecodeFailure::Codec(CodecError::ResourceLimit(limit))) => {
                assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
                if limit.operation == "iges source object ID" {
                    source_objects += 1;
                    if source_objects == 3 {
                        return;
                    }
                }
                let next = limit.used.checked_add(limit.additional).unwrap();
                assert!(next > cap, "limit did not advance from {cap}: {limit:?}");
                cap = next;
            }
            other => panic!("composite source object refusal was erased at cap {cap}: {other:?}"),
        }
    }
    panic!("composite source object was not reached within 4096 admission boundaries");
}

use super::{
    bounded_nurbs_for_curve, bounded_nurbs_for_curve_with_tolerance, close, close_with_tolerance,
    composite_child_type_allowed, composite_line_font_valid, composite_logical_connector_use_valid,
    composite_minimum_child_count, composite_use_flag_valid, concatenate_nurbs,
    elevate_nurbs_to_degree, homogeneous_control_points, insert_homogeneous_knot,
    trim_nurbs_to_interval, CompositeIndex,
};

#[test]
fn nonfinite_points_do_not_coincide_under_default_composite_tolerance() {
    assert!(!close(
        Point3::new(f64::INFINITY, 0.0, 0.0),
        Point3::new(0.0, 0.0, 0.0)
    ));
    assert!(!close(
        Point3::new(f64::NAN, 0.0, 0.0),
        Point3::new(0.0, 0.0, 0.0)
    ));
}
use crate::global::GlobalTable;
use cadmpeg_ir::geometry::{CompositeCurveSegment, CompositeCurveTransition};

fn test_nurbs(
    degree: u32,
    knots: Vec<f64>,
    control_points: Vec<Point3>,
    weights: Option<Vec<f64>>,
) -> NurbsCurve {
    NurbsCurve::from_lanes(degree, knots, control_points, weights, false).expect("valid test NURBS")
}

#[test]
fn composite_child_types_follow_the_declared_dialect() {
    assert!(composite_child_type_allowed(116, 0, GlobalTable::V4_0));
    assert!(composite_child_type_allowed(132, 0, GlobalTable::V4_0));
    assert!(composite_child_type_allowed(112, 0, GlobalTable::V4_0));
    assert!(!composite_child_type_allowed(112, 1, GlobalTable::V4_0));
    assert!(!composite_child_type_allowed(112, 3, GlobalTable::V5_0));
    assert!(!composite_child_type_allowed(106, 1, GlobalTable::V4_0));
    assert!(!composite_child_type_allowed(130, 0, GlobalTable::V4_0));
    assert!(composite_child_type_allowed(106, 1, GlobalTable::V5_0));
    assert!(composite_child_type_allowed(130, 0, GlobalTable::V5_0));
    assert!(composite_child_type_allowed(142, 0, GlobalTable::V5Later));
}

#[test]
fn composite_child_count_follows_the_declared_dialect() {
    assert_eq!(composite_minimum_child_count(GlobalTable::V4_0), 2);
    assert_eq!(composite_minimum_child_count(GlobalTable::V5_0), 1);
    assert_eq!(composite_minimum_child_count(GlobalTable::V5Later), 1);
}

#[test]
fn composite_entity_use_flag_follows_the_declared_dialect() {
    assert!(UseFlag::parse(0, crate::global::GlobalTable::V5Later)
        .is_some_and(|use_flag| composite_use_flag_valid(use_flag, GlobalTable::V4_0)));
    for use_flag in [1, 2, 3, 4, 5] {
        assert!(
            !UseFlag::parse(use_flag, crate::global::GlobalTable::V5Later)
                .is_some_and(|use_flag| composite_use_flag_valid(use_flag, GlobalTable::V4_0)),
            "{use_flag}"
        );
    }
    for use_flag in 0..=6 {
        assert!(
            UseFlag::parse(use_flag, crate::global::GlobalTable::V5Later)
                .is_some_and(|use_flag| composite_use_flag_valid(use_flag, GlobalTable::V5_0)),
            "{use_flag}"
        );
    }
    assert!(!UseFlag::parse(7, crate::global::GlobalTable::V5Later)
        .is_some_and(|use_flag| composite_use_flag_valid(use_flag, GlobalTable::V5_0)));
}

#[test]
fn composite_line_font_follows_the_declared_dialect_and_hierarchy() {
    assert!(composite_line_font_valid(
        1,
        crate::directory::Hierarchy::parse(0),
        GlobalTable::V4_0
    ));
    assert!(composite_line_font_valid(
        -3,
        crate::directory::Hierarchy::parse(2),
        GlobalTable::V4_0
    ));
    assert!(!composite_line_font_valid(
        0,
        crate::directory::Hierarchy::parse(0),
        GlobalTable::V4_0
    ));
    assert!(composite_line_font_valid(
        0,
        crate::directory::Hierarchy::parse(1),
        GlobalTable::V4_0
    ));
    assert!(composite_line_font_valid(
        0,
        crate::directory::Hierarchy::parse(0),
        GlobalTable::V5_0
    ));
}

#[test]
fn composite_logical_connector_use_flag_is_a_v5_rule() {
    assert!(composite_logical_connector_use_valid(
        UseFlag::parse(0, crate::global::GlobalTable::V5Later).unwrap(),
        true,
        GlobalTable::V4_0
    ));
    assert!(!composite_logical_connector_use_valid(
        UseFlag::parse(0, crate::global::GlobalTable::V5Later).unwrap(),
        true,
        GlobalTable::V5_0
    ));
    assert!(composite_logical_connector_use_valid(
        UseFlag::parse(4, crate::global::GlobalTable::V5Later).unwrap(),
        true,
        GlobalTable::V5Later
    ));
    assert!(!composite_logical_connector_use_valid(
        UseFlag::parse(5, crate::global::GlobalTable::V5Later).unwrap(),
        true,
        GlobalTable::V5_0
    ));
    assert!(composite_logical_connector_use_valid(
        UseFlag::parse(0, crate::global::GlobalTable::V5Later).unwrap(),
        false,
        GlobalTable::V5_0
    ));
}

#[test]
fn decode_rejects_a_nonzero_v4_composite_entity_use_flag() {
    const GLOBAL_V4: &[u8] = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file_with_global(
                &[
                    OwnedTestEntity {
                        entity_type: 110,
                        form: 0,
                        label: "CHILD1".into(),
                        status: "00010000",
                        parameters: "110,0,0,0,1,0,0;".into(),
                    },
                    OwnedTestEntity {
                        entity_type: 110,
                        form: 0,
                        label: "CHILD2".into(),
                        status: "00010000",
                        parameters: "110,1,0,0,2,0,0;".into(),
                    },
                    OwnedTestEntity {
                        entity_type: 102,
                        form: 0,
                        label: "COMPOSIT".into(),
                        status: "00000100",
                        parameters: "102,2,1,3;".into(),
                    },
                ],
                GLOBAL_V4,
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(!result
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.id.as_str() == "iges:model:curve#D5"));
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == IgesLossCode::EntityNotProjected.kind()
            && loss
                .message
                .contains("Type 102 Entity Use Flag must be 00 in IGES 4.0")
    }));
}

#[test]
fn decode_rejects_a_v5_logical_connector_without_entity_use_flag_04() {
    const GLOBAL_V5_0: &[u8] = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file_with_global(
                &[
                    OwnedTestEntity {
                        entity_type: 132,
                        form: 0,
                        label: "CP1".into(),
                        status: "00010000",
                        parameters: "132,0,0,0,0,1,1,2HP1,0,3HCP1,0,1,1,0,0;".into(),
                    },
                    OwnedTestEntity {
                        entity_type: 132,
                        form: 0,
                        label: "CP2".into(),
                        status: "00010000",
                        parameters: "132,1,0,0,0,1,1,2HP2,0,3HCP2,0,1,1,0,0;".into(),
                    },
                    OwnedTestEntity {
                        entity_type: 102,
                        form: 0,
                        label: "CONN".into(),
                        status: "00000000",
                        parameters: "102,2,1,3;".into(),
                    },
                ],
                GLOBAL_V5_0,
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result.report().losses.iter().any(|loss| {
        loss.code == IgesLossCode::EntityNotProjected.kind()
            && loss.message.contains(
                "Type 102 logical connectors made of exactly two Type 132 Connect Points require Entity Use Flag 04",
            )
    }));
}

#[test]
fn decode_rejects_a_zero_v4_composite_line_font() {
    const GLOBAL_V4: &[u8] = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file_with_global(
                &[OwnedTestEntity {
                    entity_type: 102,
                    form: 0,
                    label: "COMPOSIT".into(),
                    status: "00000000",
                    parameters: "102,1,1;".into(),
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
                .contains("Type 102 Line Font must be nonzero in IGES 4.0")
    }));
}

#[test]
fn decode_rejects_a_single_v4_composite_constituent() {
    const GLOBAL_V4: &[u8] = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file_with_global_and_directory_fields(
                &[
                    OwnedTestEntity {
                        entity_type: 110,
                        form: 0,
                        label: "CHILD".into(),
                        status: "00010000",
                        parameters: "110,0,0,0,1,0,0;".into(),
                    },
                    OwnedTestEntity {
                        entity_type: 102,
                        form: 0,
                        label: "COMPOSIT".into(),
                        status: "00000000",
                        parameters: "102,1,1;".into(),
                    },
                ],
                GLOBAL_V4,
                &[],
                &[(1, 1), (3, 1)],
                &[],
                &[],
                &[],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result.ir().model.procedural_curves.is_empty());
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == IgesLossCode::EntityNotProjected.kind())
            .count(),
        1
    );
}

#[test]
fn decode_projects_a_single_v5_composite_constituent() {
    const GLOBAL_V5_0: &[u8] = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file_with_global(
                &[
                    OwnedTestEntity {
                        entity_type: 110,
                        form: 0,
                        label: "CHILD".into(),
                        status: "00010000",
                        parameters: "110,0,0,0,1,0,0;".into(),
                    },
                    OwnedTestEntity {
                        entity_type: 102,
                        form: 0,
                        label: "COMPOSIT".into(),
                        status: "00000000",
                        parameters: "102,1,1;".into(),
                    },
                ],
                GLOBAL_V5_0,
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.procedural_curves.len(), 1);
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| { loss.code == IgesLossCode::EntityNotProjected.kind() }));
}

#[test]
fn decode_projects_a_v5_type_142_constituent_through_its_model_curve() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 108,
                    form: 0,
                    label: "PLANE".into(),
                    status: "00010000",
                    parameters: "108,0,0,1,0,0,0,0,0,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 106,
                    form: 63,
                    label: "MODEL".into(),
                    status: "00010000",
                    parameters: "106,1,5,0,0,0,1,0,1,1,0,1,0,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 106,
                    form: 63,
                    label: "PCURVE".into(),
                    status: "00010500",
                    parameters: "106,1,5,0,0,0,1,0,1,1,0,1,0,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 142,
                    form: 0,
                    label: "CURVSRF".into(),
                    status: "00010000",
                    parameters: "142,0,1,5,3,3;".into(),
                },
                OwnedTestEntity {
                    entity_type: 102,
                    form: 0,
                    label: "COMPOSIT".into(),
                    status: "00000000",
                    parameters: "102,1,7;".into(),
                },
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();

    let composite = result
        .ir()
        .model
        .procedural_curves
        .iter()
        .find(|curve| {
            result.ir().model.procedural_curve_owner(&curve.id)
                == Some(&CurveId::mint("iges:model:curve#D9").expect("identity grammar"))
        })
        .expect("Type 102 neutral carrier");
    let ProceduralCurveDefinition::Compound(compound) = composite.definition() else {
        panic!("expected a compound neutral carrier");
    };
    let components = compound.components();

    assert_eq!(
        components
            .iter()
            .map(|item| item.component.clone())
            .collect::<Vec<_>>(),
        &[CurveId::mint("iges:model:curve#D3").expect("identity grammar")]
    );
    assert!(
        result.report().losses.is_empty(),
        "{:?}",
        result.report().losses
    );
}

#[test]
fn decode_projects_a_v5_type_130_constituent_after_its_offset_carrier() {
    const GLOBAL_V5_0: &[u8] = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file_with_global(
                &[
                    OwnedTestEntity {
                        entity_type: 110,
                        form: 0,
                        label: "BASE".into(),
                        status: "00010000",
                        parameters: "110,0,0,0,1,0,0;".into(),
                    },
                    OwnedTestEntity {
                        entity_type: 130,
                        form: 0,
                        label: "OFFSET".into(),
                        status: "00010000",
                        parameters: "130,1,1,0,,,0.5,,,,0,0,1,0,1;".into(),
                    },
                    OwnedTestEntity {
                        entity_type: 102,
                        form: 0,
                        label: "COMPOSIT".into(),
                        status: "00000000",
                        parameters: "102,1,3;".into(),
                    },
                ],
                GLOBAL_V5_0,
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    let composite = result
        .ir()
        .model
        .procedural_curves
        .iter()
        .find(|curve| {
            result.ir().model.procedural_curve_owner(&curve.id)
                == Some(&CurveId::mint("iges:model:curve#D5").expect("identity grammar"))
        })
        .expect("Type 102 neutral carrier");
    let ProceduralCurveDefinition::Compound(compound) = composite.definition() else {
        panic!("expected a compound neutral carrier");
    };
    let components = compound.components();

    assert_eq!(
        components
            .iter()
            .map(|item| item.component.clone())
            .collect::<Vec<_>>(),
        &[CurveId::mint("iges:model:curve#D3").expect("identity grammar")]
    );
    assert!(
        result.report().losses.is_empty(),
        "{:?}",
        result.report().losses
    );
}

#[test]
fn decode_projects_a_v4_composite_with_a_nonzero_line_font() {
    const GLOBAL_V4: &[u8] = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file_with_global_and_directory_fields(
                &[
                    OwnedTestEntity {
                        entity_type: 110,
                        form: 0,
                        label: "CHILD1".into(),
                        status: "00010000",
                        parameters: "110,0,0,0,1,0,0;".into(),
                    },
                    OwnedTestEntity {
                        entity_type: 110,
                        form: 0,
                        label: "CHILD2".into(),
                        status: "00010000",
                        parameters: "110,1,0,0,2,0,0;".into(),
                    },
                    OwnedTestEntity {
                        entity_type: 102,
                        form: 0,
                        label: "COMPOSIT".into(),
                        status: "00000000",
                        parameters: "102,2,1,3;".into(),
                    },
                ],
                GLOBAL_V4,
                &[],
                &[(1, 1), (3, 1), (5, 1)],
                &[],
                &[],
                &[],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.procedural_curves.len(), 1);
    assert!(!result.report().losses.iter().any(|loss| {
        loss.message
            .contains("Type 102 Line Font must be nonzero in IGES 4.0")
    }));
}

#[test]
fn zero_join_tolerance_requires_exact_endpoint_equality() {
    let left = Point3::new(1.0, 2.0, 3.0);
    let right = Point3::new(1.0, 2.0, 3.0 + f64::EPSILON * 4.0);

    assert!(close_with_tolerance(left, left, Some(0.0)));
    assert!(!close_with_tolerance(left, right, Some(0.0)));
}

#[test]
fn positive_join_tolerance_excludes_the_resolution_boundary() {
    let left = Point3::new(0.0, 0.0, 0.0);
    let inside = Point3::new(0.000_999, 0.0, 0.0);
    let boundary = Point3::new(0.001, 0.0, 0.0);

    assert!(close_with_tolerance(left, inside, Some(0.001)));
    assert!(!close_with_tolerance(left, boundary, Some(0.001)));
}

#[test]
fn bounded_line_carrier_excludes_an_endpoint_at_the_resolution_boundary() {
    let curve_id = CurveId::mint("test:model:curve#line").expect("identity grammar");
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
    });
    ir.model.points.extend([
        Point::new(
            PointId::mint("test:model:point#start-point").expect("identity grammar"),
            cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.001, 0.0, 0.0))
                .expect("a finite position is a point"),
            None,
        ),
        Point::new(
            PointId::mint("test:model:point#end-point").expect("identity grammar"),
            cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 0.0, 0.0))
                .expect("a finite position is a point"),
            None,
        ),
    ]);
    ir.model.vertices.extend([
        Vertex {
            id: VertexId::mint("test:model:vertex#start").expect("identity grammar"),
            point: PointId::mint("test:model:point#start-point").expect("identity grammar"),
            tolerance: None,
        },
        Vertex {
            id: VertexId::mint("test:model:vertex#end").expect("identity grammar"),
            point: PointId::mint("test:model:point#end-point").expect("identity grammar"),
            tolerance: None,
        },
    ]);
    ir.model.edges.push(Edge {
        id: EdgeId::mint("test:model:edge#edge").expect("identity grammar"),
        carrier: cadmpeg_ir::topology::EdgeCarrier::new(Some(curve_id.clone()), Some([0.0, 1.0]))
            .unwrap(),
        start: VertexId::mint("test:model:vertex#start").expect("identity grammar"),
        end: VertexId::mint("test:model:vertex#end").expect("identity grammar"),
        tolerance: None,
    });

    assert!(
        bounded_nurbs_for_curve_with_tolerance(&ir, &curve_id, Some(0.001), None, None)
            .expect("carrier lanes pair")
            .is_none()
    );

    ir.model.points[0].set_position(
        cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.000_999, 0.0, 0.0))
            .expect("a finite position is a point"),
    );
    assert!(
        bounded_nurbs_for_curve_with_tolerance(&ir, &curve_id, Some(0.001), None, None)
            .expect("carrier lanes pair")
            .is_some()
    );
}

#[test]
fn decode_refuses_a_composite_child_count_over_its_projection_limit() {
    let error = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[OwnedTestEntity {
                entity_type: 102,
                form: 0,
                label: "COMPOSIT".into(),
                status: "00000000",
                parameters: "102,100001;".into(),
            }])),
            &DecodeOptions::default(),
        )
        .unwrap_err();

    assert!(matches!(
        error,
        cadmpeg_ir::DecodeFailure::Codec(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::Codec("iges_composite_children")
                && limit.limit == 100_000
                && limit.used == 100_000
                && limit.additional == 1
    ));
}

#[test]
fn composite_flattening_over_its_depth_limit_fuses_the_decode_session() {
    let base_id = CurveId::mint("test:model:curve#base").expect("identity grammar");
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: base_id.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(test_nurbs(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            None,
        ))),
        source_object: None,
    });
    ir.model.points.extend([
        Point::new(
            PointId::mint("test:model:point#base-start-point").expect("identity grammar"),
            cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
                .expect("a finite position is a point"),
            None,
        ),
        Point::new(
            PointId::mint("test:model:point#base-end-point").expect("identity grammar"),
            cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 0.0, 0.0))
                .expect("a finite position is a point"),
            None,
        ),
    ]);
    ir.model.vertices.extend([
        Vertex {
            id: VertexId::mint("test:model:vertex#base-start").expect("identity grammar"),
            point: PointId::mint("test:model:point#base-start-point").expect("identity grammar"),
            tolerance: None,
        },
        Vertex {
            id: VertexId::mint("test:model:vertex#base-end").expect("identity grammar"),
            point: PointId::mint("test:model:point#base-end-point").expect("identity grammar"),
            tolerance: None,
        },
    ]);
    ir.model.edges.push(Edge {
        id: EdgeId::mint("test:model:edge#base-edge").expect("identity grammar"),
        carrier: cadmpeg_ir::topology::EdgeCarrier::new(Some(base_id.clone()), Some([0.0, 1.0]))
            .unwrap(),
        start: VertexId::mint("test:model:vertex#base-start").expect("identity grammar"),
        end: VertexId::mint("test:model:vertex#base-end").expect("identity grammar"),
        tolerance: None,
    });

    let mut child_id = base_id;
    for level in 0..65 {
        let composite_id =
            CurveId::mint(format!("test:model:curve#composite-{level}")).expect("identity grammar");
        ir.model.curves.push(Curve {
            id: composite_id.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Composite {
                segments: cadmpeg_ir::geometry::CompositeCurveSegments::try_from(vec![
                    CompositeCurveSegment {
                        curve: child_id,
                        same_sense: true,
                        transition: CompositeCurveTransition::Continuous,
                    },
                ])
                .unwrap(),
                self_intersect: None,
            }),
            source_object: None,
        });
        child_id = composite_id;
    }

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &DecodePolicy::default()).unwrap();
    // The depth refusal is the reader's answer, not an absent carrier: it names
    // the limit it exceeds and the session carries the same refusal.
    let error = bounded_nurbs_for_curve(&ir, &child_id, Some(&ctx), None)
        .expect_err("a composite deeper than the limit is refused")
        .to_string();
    assert!(error.contains("iges_composite_depth"), "{error}");
    assert!(matches!(
        ctx.finish_session(),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::Codec("iges_composite_depth")
                && limit.limit == 64
                && limit.used == 64
                && limit.additional == 1
    ));
}

#[test]
fn bounded_line_carrier_selects_a_curve_valid_edge_occurrence() {
    let curve_id = CurveId::mint("test:model:curve#line").expect("identity grammar");
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
    });
    ir.model.points.extend([
        Point::new(
            PointId::mint("test:model:point#wrong-start-point").expect("identity grammar"),
            cadmpeg_ir::features::FinitePoint3::new(Point3::new(10.0, 0.0, 0.0))
                .expect("a finite position is a point"),
            None,
        ),
        Point::new(
            PointId::mint("test:model:point#wrong-end-point").expect("identity grammar"),
            cadmpeg_ir::features::FinitePoint3::new(Point3::new(11.0, 0.0, 0.0))
                .expect("a finite position is a point"),
            None,
        ),
        Point::new(
            PointId::mint("test:model:point#matching-start-point").expect("identity grammar"),
            cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
                .expect("a finite position is a point"),
            None,
        ),
        Point::new(
            PointId::mint("test:model:point#matching-end-point").expect("identity grammar"),
            cadmpeg_ir::features::FinitePoint3::new(Point3::new(2.0, 0.0, 0.0))
                .expect("a finite position is a point"),
            None,
        ),
    ]);
    ir.model.vertices.extend([
        Vertex {
            id: VertexId::mint("test:model:vertex#wrong-start").expect("identity grammar"),
            point: PointId::mint("test:model:point#wrong-start-point").expect("identity grammar"),
            tolerance: None,
        },
        Vertex {
            id: VertexId::mint("test:model:vertex#wrong-end").expect("identity grammar"),
            point: PointId::mint("test:model:point#wrong-end-point").expect("identity grammar"),
            tolerance: None,
        },
        Vertex {
            id: VertexId::mint("test:model:vertex#matching-start").expect("identity grammar"),
            point: PointId::mint("test:model:point#matching-start-point")
                .expect("identity grammar"),
            tolerance: None,
        },
        Vertex {
            id: VertexId::mint("test:model:vertex#matching-end").expect("identity grammar"),
            point: PointId::mint("test:model:point#matching-end-point").expect("identity grammar"),
            tolerance: None,
        },
    ]);
    ir.model.edges.extend([
        Edge {
            id: EdgeId::mint("test:model:edge#wrong-occurrence").expect("identity grammar"),
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(
                Some(curve_id.clone()),
                Some([5.0, 6.0]),
            )
            .unwrap(),
            start: VertexId::mint("test:model:vertex#wrong-start").expect("identity grammar"),
            end: VertexId::mint("test:model:vertex#wrong-end").expect("identity grammar"),
            tolerance: None,
        },
        Edge {
            id: EdgeId::mint("test:model:edge#matching-occurrence").expect("identity grammar"),
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(Some(curve_id), Some([0.0, 2.0]))
                .unwrap(),
            start: VertexId::mint("test:model:vertex#matching-start").expect("identity grammar"),
            end: VertexId::mint("test:model:vertex#matching-end").expect("identity grammar"),
            tolerance: None,
        },
    ]);

    let (carrier, range) = bounded_nurbs_for_curve(
        &ir,
        &CurveId::mint("test:model:curve#line").expect("identity grammar"),
        None,
        None,
    )
    .expect("carrier lanes pair")
    .expect("the curve-valid edge occurrence");
    assert_eq!(range, [0.0, 1.0]);
    assert_eq!(carrier.control_points()[0], Point3::new(0.0, 0.0, 0.0));
    assert_eq!(carrier.control_points()[1], Point3::new(2.0, 0.0, 0.0));
}

#[test]
fn bounded_line_carrier_rejects_conflicting_valid_edge_ranges() {
    let curve_id = CurveId::mint("test:model:curve#line").expect("identity grammar");
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
    });
    for (index, end) in [(0, 1.0), (1, 2.0)] {
        let start_point = PointId::mint(format!("test:model:point#start-point-{index}"))
            .expect("identity grammar");
        let end_point =
            PointId::mint(format!("test:model:point#end-point-{index}")).expect("identity grammar");
        let start_vertex =
            VertexId::mint(format!("test:model:vertex#start-{index}")).expect("identity grammar");
        let end_vertex =
            VertexId::mint(format!("test:model:vertex#end-{index}")).expect("identity grammar");
        ir.model.points.extend([
            Point::new(
                start_point.clone(),
                cadmpeg_ir::features::FinitePoint3::new(Point3::new(index as f64, 0.0, 0.0))
                    .expect("a finite position is a point"),
                None,
            ),
            Point::new(
                end_point.clone(),
                cadmpeg_ir::features::FinitePoint3::new(Point3::new(end, 0.0, 0.0))
                    .expect("a finite position is a point"),
                None,
            ),
        ]);
        ir.model.vertices.extend([
            Vertex {
                id: start_vertex.clone(),
                point: start_point,
                tolerance: None,
            },
            Vertex {
                id: end_vertex.clone(),
                point: end_point,
                tolerance: None,
            },
        ]);
        ir.model.edges.push(Edge {
            id: EdgeId::mint(format!("test:model:edge#edge-{index}")).expect("identity grammar"),
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(
                Some(curve_id.clone()),
                Some([index as f64, end]),
            )
            .unwrap(),
            start: start_vertex,
            end: end_vertex,
            tolerance: None,
        });
    }

    assert!(bounded_nurbs_for_curve(&ir, &curve_id, None, None)
        .expect("carrier lanes pair")
        .is_none());
}

#[test]
fn composite_index_lookups_match_the_unindexed_scan() {
    let bounded = CurveId::mint("test:model:curve#bounded").expect("identity grammar");
    let edgeless = CurveId::mint("test:model:curve#edgeless").expect("identity grammar");
    let absent = CurveId::mint("test:model:curve#absent").expect("identity grammar");
    let mut ir = CadIr::empty();
    for id in [bounded.clone(), edgeless.clone()] {
        ir.model.curves.push(Curve {
            id,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            )),
            source_object: None,
        });
    }
    ir.model.points.extend([
        Point::new(
            PointId::mint("test:model:point#start-point").expect("identity grammar"),
            cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
                .expect("a finite position is a point"),
            None,
        ),
        Point::new(
            PointId::mint("test:model:point#end-point").expect("identity grammar"),
            cadmpeg_ir::features::FinitePoint3::new(Point3::new(2.0, 0.0, 0.0))
                .expect("a finite position is a point"),
            None,
        ),
    ]);
    ir.model.vertices.extend([
        Vertex {
            id: VertexId::mint("test:model:vertex#start").expect("identity grammar"),
            point: PointId::mint("test:model:point#start-point").expect("identity grammar"),
            tolerance: None,
        },
        Vertex {
            id: VertexId::mint("test:model:vertex#end").expect("identity grammar"),
            point: PointId::mint("test:model:point#end-point").expect("identity grammar"),
            tolerance: None,
        },
    ]);
    ir.model.edges.push(Edge {
        id: EdgeId::mint("test:model:edge#edge").expect("identity grammar"),
        carrier: cadmpeg_ir::topology::EdgeCarrier::new(Some(bounded.clone()), Some([0.0, 2.0]))
            .unwrap(),
        start: VertexId::mint("test:model:vertex#start").expect("identity grammar"),
        end: VertexId::mint("test:model:vertex#end").expect("identity grammar"),
        tolerance: None,
    });

    let index = CompositeIndex::from_ir(&ir, None).unwrap();
    for curve_id in [bounded, edgeless, absent] {
        let scanned =
            bounded_nurbs_for_curve(&ir, &curve_id, None, None).expect("carrier lanes pair");
        let indexed = bounded_nurbs_for_curve(&ir, &curve_id, None, Some(&index))
            .expect("carrier lanes pair");
        assert_eq!(
            scanned.as_ref().map(|(carrier, range)| (
                carrier.degree(),
                carrier.control_points(),
                *range
            )),
            indexed.as_ref().map(|(carrier, range)| (
                carrier.degree(),
                carrier.control_points(),
                *range
            )),
        );
    }

    assert!(bounded_nurbs_for_curve(
        &ir,
        &CurveId::mint("test:model:curve#bounded").expect("identity grammar"),
        None,
        Some(&index)
    )
    .expect("carrier lanes pair")
    .is_some());
    assert!(bounded_nurbs_for_curve(
        &ir,
        &CurveId::mint("test:model:curve#edgeless").expect("identity grammar"),
        None,
        Some(&index)
    )
    .expect("carrier lanes pair")
    .is_none());
    assert!(bounded_nurbs_for_curve(
        &ir,
        &CurveId::mint("test:model:curve#absent").expect("identity grammar"),
        None,
        Some(&index)
    )
    .expect("carrier lanes pair")
    .is_none());
}

#[test]
fn composite_scanned_edges_admit_slots_and_endpoint_ids() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(composite_curve_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let ir = decoded.ir();
    let curve_id = ir.model.edges.iter().find_map(|edge| edge.curve()).unwrap();
    for (retained, operation) in [
        (false, "iges composite scanned edge candidates"),
        (true, "iges composite scanned edge start ID"),
        (true, "iges composite scanned edge end ID"),
    ] {
        let mut cap = 0_u64;
        let mut found = false;
        for _ in 0..4096 {
            let mut policy = DecodePolicy::service();
            if retained {
                policy.limits.max_retained_bytes = cap;
            } else {
                policy.limits.max_collection_items = cap;
            }
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            match super::bounded_edge_for_curve(ir, curve_id, 0.0, None, Some(&ctx)) {
                Err(CodecError::ResourceLimit(limit)) => {
                    let dimension = if retained {
                        ResourceDimension::RetainedBytes
                    } else {
                        ResourceDimension::CollectionItems
                    };
                    assert_eq!(limit.dimension, dimension);
                    if limit.operation == operation {
                        found = true;
                        break;
                    }
                    cap = limit.used.checked_add(limit.additional).unwrap();
                }
                Ok(_) => panic!("scanned edge succeeded before {operation} at cap {cap}"),
                Err(error) => panic!("unexpected scanned edge error: {error}"),
            }
        }
        assert!(found, "scanned edge admission was not reached: {operation}");
    }
}

#[test]
fn rational_linear_degree_elevation_preserves_the_curve() {
    let mut curve = test_nurbs(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)],
        Some(vec![1.0, 3.0]),
    );
    let before = cadmpeg_ir::eval::nurbs_curve_point_at(&curve, 0.25)
        .expect("valid rational linear NURBS evaluates before degree elevation");
    elevate_nurbs_to_degree(None, &mut curve, [0.0, 1.0], 2, None).expect("elevation lanes pair");
    let after = cadmpeg_ir::eval::nurbs_curve_point_at(&curve, 0.25)
        .expect("valid rational quadratic NURBS evaluates after degree elevation");
    assert!(before.distance(after.get()) <= 1.0e-12);
    assert_eq!(curve.control_points()[1], Point3::new(1.5, 0.0, 0.0));
    assert_eq!(curve.pole_rows().weights(), Some(vec![1.0, 2.0, 3.0]));
}

#[test]
fn homogeneous_control_points_refuse_collection_limit() {
    let curve = test_nurbs(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        None,
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = homogeneous_control_points(Some(&ctx), &curve).unwrap_err();
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.used == 0
                && limit.additional == 2
    ));
}

#[test]
fn composite_knot_insertion_refuses_knot_collection_limit() {
    let controls = [[1.0, 0.0, 0.0, 0.0], [1.0, 1.0, 0.0, 0.0]];
    let knots = [0.0, 0.0, 1.0, 1.0];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 4;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = insert_homogeneous_knot(Some(&ctx), &controls, &knots, 1, 0.5).unwrap_err();
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.used == 0
                && limit.additional == 5
    ));
}

#[test]
fn composite_knot_insertion_refuses_control_collection_limit() {
    let controls = [[1.0, 0.0, 0.0, 0.0], [1.0, 1.0, 0.0, 0.0]];
    let knots = [0.0, 0.0, 1.0, 1.0];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 7;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = insert_homogeneous_knot(Some(&ctx), &controls, &knots, 1, 0.5).unwrap_err();
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.used == 5
                && limit.additional == 3
    ));
}

#[test]
fn composite_elevated_knots_refuse_collection_limit() {
    let mut curve = test_nurbs(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        Some(vec![1.0, 2.0]),
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 19;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = elevate_nurbs_to_degree(Some(&ctx), &mut curve, [0.0, 1.0], 2, None)
        .unwrap_err()
        .non_resource()
        .unwrap_err();
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.used == 17
                && limit.additional == 3
    ));
}

#[test]
fn composite_trimmed_lanes_refuse_fallible_copies_and_weight_conversion() {
    let curve = test_nurbs(
        2,
        vec![0.0, 0.0, 0.0, 1.0, 2.0, 2.0, 2.0],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 2.0, 0.0),
            Point3::new(2.0, -1.0, 0.0),
            Point3::new(4.0, 0.0, 0.0),
        ],
        Some(vec![1.0, 0.5, 2.0, 1.0]),
    );
    for operation in [
        "iges composite trim knot copy",
        "iges composite trimmed controls",
        "iges composite trimmed knots",
        "iges composite trimmed weight conversion",
    ] {
        let mut cap = 0_u64;
        let mut found = false;
        for _ in 0..4096 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            match trim_nurbs_to_interval(Some(&ctx), &curve, [0.25, 1.5]) {
                Err(error) => match error.non_resource() {
                    Err(CodecError::ResourceLimit(limit)) => {
                        assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                        if limit.operation == operation {
                            found = true;
                            break;
                        }
                        cap = limit.used.checked_add(limit.additional).unwrap();
                    }
                    other => panic!("unexpected trimmed lane failure at {operation}: {other:?}"),
                },
                Ok(_) => panic!("trimmed lane succeeded before {operation}"),
            }
        }
        assert!(found, "trimmed lane boundary was not reached: {operation}");
    }
}

#[test]
fn composite_elevation_refuses_nested_knot_and_weight_storage() {
    let source = || {
        test_nurbs(
            2,
            vec![0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0],
            vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 2.0, 0.0),
                Point3::new(2.0, -1.0, 0.0),
                Point3::new(3.0, 0.0, 0.0),
            ],
            Some(vec![1.0, 2.0, 1.0, 3.0]),
        )
    };
    for operation in [
        "iges composite elevation knot copy",
        "iges composite internal knot values",
        "iges composite elevated knot suffix",
        "iges composite elevated weight conversion",
        "iges composite elevated span",
    ] {
        let mut cap = 0_u64;
        let mut found = false;
        for _ in 0..4096 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut curve = source();
            match elevate_nurbs_to_degree(Some(&ctx), &mut curve, [0.0, 1.0], 3, None) {
                Err(error) => match error.non_resource() {
                    Err(CodecError::ResourceLimit(limit)) => {
                        assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                        if limit.operation == operation {
                            found = true;
                            break;
                        }
                        cap = limit.used.checked_add(limit.additional).unwrap();
                    }
                    other => panic!("unexpected elevation failure at {operation}: {other:?}"),
                },
                Ok(()) => panic!("elevation succeeded before {operation}"),
            }
        }
        assert!(found, "elevation boundary was not reached: {operation}");
    }
}

#[test]
fn composite_child_weights_refuse_collection_limit() {
    let curve = test_nurbs(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        None,
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = concatenate_nurbs(Some(&ctx), vec![(curve, [0.0, 1.0], ())], None)
        .unwrap_err()
        .non_resource()
        .unwrap_err();
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.used == 2
                && limit.additional == 2
    ));
}

#[test]
fn composite_join_refuses_child_and_joined_lane_storage() {
    let children = |rational: bool| {
        let first_weights = rational.then(|| vec![1.0, 2.0]);
        let second_weights = rational.then(|| vec![2.0, 1.0]);
        vec![
            (
                test_nurbs(
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
                    first_weights,
                ),
                [0.0, 1.0],
                (),
            ),
            (
                test_nurbs(
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![Point3::new(1.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)],
                    second_weights,
                ),
                [0.0, 1.0],
                (),
            ),
        ]
    };
    for (rational, operation) in [
        (false, "iges composite child control points"),
        (true, "iges composite child weight copy"),
        (false, "iges composite segment slots"),
        (false, "iges composite joined knots"),
        (false, "iges composite joined controls"),
        (false, "iges composite joined weights"),
        (true, "iges composite joined weighted poles"),
    ] {
        let mut cap = 0_u64;
        let mut found = false;
        for _ in 0..128 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            match concatenate_nurbs(Some(&ctx), children(rational), Some(0.0)) {
                Err(error) => match error.non_resource() {
                    Err(CodecError::ResourceLimit(limit)) => {
                        assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                        if limit.operation == operation {
                            found = true;
                            break;
                        }
                        cap = limit.used.checked_add(limit.additional).unwrap();
                    }
                    other => panic!("unexpected composite join refusal at {operation}: {other:?}"),
                },
                Ok(_) => panic!("composite join succeeded before {operation}"),
            }
        }
        assert!(
            found,
            "composite join boundary was not reached: {operation}"
        );
    }
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert!(concatenate_nurbs(Some(&ctx), children(true), Some(0.0))
        .unwrap()
        .is_some());
}

mod attachments;
mod carrier_projection;
