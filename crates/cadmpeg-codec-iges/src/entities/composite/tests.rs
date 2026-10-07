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
        assert_context_refusal(ResourceDimension::CollectionItems, operation, |ctx| {
            CompositeIndex::from_ir(ir, ctx)
        });
    }
}

#[test]
fn composite_index_refuses_each_identity_copy() {
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
        let dimension = if operation == "iges composite point index keys" {
            ResourceDimension::MaterializedBytes
        } else {
            ResourceDimension::RetainedBytes
        };
        assert_context_refusal(dimension, operation, |ctx| CompositeIndex::from_ir(ir, ctx));
    }
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let index = CompositeIndex::from_ir(ir, &ctx).unwrap();
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
            ctx,
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
        assert_context_refusal(dimension, operation, |ctx| {
            add(&mut CompositeIndex::default(), ctx)
        });
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
            "store procedural curve constructions",
        ),
        (composite_curve_file(), "iges composite wire edge ids"),
    ] {
        assert_decode_refusal(&bytes, ResourceDimension::CollectionItems, operation);
    }
}

#[test]
fn native_composite_segment_curve_ids_refuse_retained_copy() {
    let bytes = composite_curve_with_join_gap(0.001_001);
    assert_decode_refusal(
        &bytes,
        ResourceDimension::RetainedBytes,
        "iges composite native segment curve ids",
    );
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
        assert_decode_refusal(
            &bytes,
            ResourceDimension::RetainedBytes,
            "iges composite projection identity copy",
        );
    }
}

#[test]
fn composite_child_carriers_refuse_nested_collection_admission() {
    let bytes = composite_curve_file();
    for operation in [
        "iges composite child pointer slots",
        "iges composite child carrier nodes",
        "iges composite child curve ids",
        "iges composite line knots",
        "iges composite line points",
    ] {
        assert_decode_refusal(&bytes, ResourceDimension::CollectionItems, operation);
    }
}

#[test]
fn composite_child_curve_id_copies_refuse_storage_budget() {
    const CHILD_COUNT: usize = 32;
    let bytes = composite_curve_file();
    assert_decode_refusal(
        &bytes,
        ResourceDimension::RetainedBytes,
        "iges composite projected child curve IDs",
    );
    let pointers = (0..CHILD_COUNT)
        .map(|index| if index % 2 == 0 { "1" } else { "3" })
        .collect::<Vec<_>>()
        .join(",");
    let bytes = owned_test_file(&[
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
            parameters: "110,1,0,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 102,
            form: 0,
            label: "COMPOSIT".into(),
            status: "00000000",
            parameters: format!("102,{CHILD_COUNT},{pointers};"),
        },
    ]);
    assert_decode_refusal(
        &bytes,
        ResourceDimension::MaterializedBytes,
        "iges composite child curve ID copies",
    );
    let decoded = IgesCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.procedural_curves.len(), 1);
    let Some(SolvedCurveGeometry::Nurbs(curve)) =
        decoded.ir().model.curves.last().unwrap().geometry.solved()
    else {
        panic!("a joined degree-one composite has a NURBS cache");
    };
    assert_eq!(curve.pole_count(), CHILD_COUNT + 1);
}

#[test]
fn composite_source_object_refusal_survives_candidate_projection() {
    let bytes = composite_curve_file();
    crate::test_support::with_service_context(&bytes, |setup_ctx| {
        let scan = crate::card::scan_with_context(&bytes, setup_ctx).unwrap();
        let (global, _) = crate::global::parse(&scan, setup_ctx).unwrap();
        let (directory, quarantined) =
            crate::directory::parse(&scan, global.global_table(setup_ctx).unwrap(), setup_ctx)
                .unwrap();
        let assembled = crate::parameter::assemble_with_context(
            &scan,
            &directory,
            &quarantined,
            &global,
            setup_ctx,
        )
        .unwrap();
        let global = global.length_context(setup_ctx).unwrap().unwrap();
        let entries = directory
            .iter()
            .map(|entry| (entry.sequence, entry))
            .collect();
        let records = assembled
            .records
            .iter()
            .map(|record| (record.directory_sequence, record))
            .collect();
        assert_context_refusal(
            ResourceDimension::RetainedBytes,
            "iges source object ID",
            |ctx| {
                let mut ir = CadIr::empty();
                super::super::geometry::project_geometry(
                    &mut ir,
                    &directory[..2],
                    &assembled.records,
                    &assembled.trailing_pointer_analysis,
                    &global,
                    setup_ctx,
                )
                .unwrap();
                let mut sequences = super::super::geometry::SourceSequences::new(ctx)?;
                super::project(
                    &mut ir,
                    &directory[2..],
                    (&entries, &records),
                    &global,
                    ctx,
                    &mut sequences,
                )
                .map(|_| ())
            },
        );
    });
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
    NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        degree,
        knots,
        control_points,
        weights,
        false,
    )
    .expect("fixture constructor admission")
    .expect("valid test NURBS")
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
    crate::test_support::with_service_context(&[], |decode_ctx| {
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
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(
                Some(curve_id.clone()),
                Some([0.0, 1.0]),
            )
            .unwrap(),
            start: VertexId::mint("test:model:vertex#start").expect("identity grammar"),
            end: VertexId::mint("test:model:vertex#end").expect("identity grammar"),
            tolerance: None,
        });

        assert!(bounded_nurbs_for_curve_with_tolerance(
            &ir,
            &curve_id,
            Some(0.001),
            decode_ctx,
            None
        )
        .expect("carrier lanes pair")
        .is_none());

        ir.model.points[0].set_position(
            cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.000_999, 0.0, 0.0))
                .expect("a finite position is a point"),
        );
        assert!(bounded_nurbs_for_curve_with_tolerance(
            &ir,
            &curve_id,
            Some(0.001),
            decode_ctx,
            None
        )
        .expect("carrier lanes pair")
        .is_some());
    });
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
    let error = bounded_nurbs_for_curve(&ir, &child_id, &ctx, None)
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
    crate::test_support::with_service_context(&[], |decode_ctx| {
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
                point: PointId::mint("test:model:point#wrong-start-point")
                    .expect("identity grammar"),
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
                point: PointId::mint("test:model:point#matching-end-point")
                    .expect("identity grammar"),
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
                start: VertexId::mint("test:model:vertex#matching-start")
                    .expect("identity grammar"),
                end: VertexId::mint("test:model:vertex#matching-end").expect("identity grammar"),
                tolerance: None,
            },
        ]);

        let (carrier, range) = bounded_nurbs_for_curve(
            &ir,
            &CurveId::mint("test:model:curve#line").expect("identity grammar"),
            decode_ctx,
            None,
        )
        .expect("carrier lanes pair")
        .expect("the curve-valid edge occurrence");
        assert_eq!(range, [0.0, 1.0]);
        assert_eq!(carrier.control_points()[0], Point3::new(0.0, 0.0, 0.0));
        assert_eq!(carrier.control_points()[1], Point3::new(2.0, 0.0, 0.0));
    });
}

#[test]
fn bounded_line_carrier_rejects_conflicting_valid_edge_ranges() {
    crate::test_support::with_service_context(&[], |decode_ctx| {
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
            let end_point = PointId::mint(format!("test:model:point#end-point-{index}"))
                .expect("identity grammar");
            let start_vertex = VertexId::mint(format!("test:model:vertex#start-{index}"))
                .expect("identity grammar");
            let end_vertex =
                VertexId::mint(format!("test:model:vertex#end-{index}")).expect("identity grammar");
            ir.model.points.extend([
                Point::new(
                    start_point.clone(),
                    cadmpeg_ir::features::FinitePoint3::new(Point3::new(
                        cadmpeg_core::convert::f64_from_index(index).expect("test index is exact"),
                        0.0,
                        0.0,
                    ))
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
                id: EdgeId::mint(format!("test:model:edge#edge-{index}"))
                    .expect("identity grammar"),
                carrier: cadmpeg_ir::topology::EdgeCarrier::new(
                    Some(curve_id.clone()),
                    Some([
                        cadmpeg_core::convert::f64_from_index(index).expect("test index is exact"),
                        end,
                    ]),
                )
                .unwrap(),
                start: start_vertex,
                end: end_vertex,
                tolerance: None,
            });
        }

        assert!(bounded_nurbs_for_curve(&ir, &curve_id, decode_ctx, None)
            .expect("carrier lanes pair")
            .is_none());
    });
}

#[test]
fn composite_index_lookups_match_the_unindexed_scan() {
    crate::test_support::with_service_context(&[], |decode_ctx| {
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
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(
                Some(bounded.clone()),
                Some([0.0, 2.0]),
            )
            .unwrap(),
            start: VertexId::mint("test:model:vertex#start").expect("identity grammar"),
            end: VertexId::mint("test:model:vertex#end").expect("identity grammar"),
            tolerance: None,
        });

        let index = CompositeIndex::from_ir(&ir, decode_ctx).unwrap();
        for curve_id in [bounded, edgeless, absent] {
            let scanned = bounded_nurbs_for_curve(&ir, &curve_id, decode_ctx, None)
                .expect("carrier lanes pair");
            let indexed = bounded_nurbs_for_curve(&ir, &curve_id, decode_ctx, Some(&index))
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
            decode_ctx,
            Some(&index)
        )
        .expect("carrier lanes pair")
        .is_some());
        assert!(bounded_nurbs_for_curve(
            &ir,
            &CurveId::mint("test:model:curve#edgeless").expect("identity grammar"),
            decode_ctx,
            Some(&index)
        )
        .expect("carrier lanes pair")
        .is_none());
        assert!(bounded_nurbs_for_curve(
            &ir,
            &CurveId::mint("test:model:curve#absent").expect("identity grammar"),
            decode_ctx,
            Some(&index)
        )
        .expect("carrier lanes pair")
        .is_none());
    });
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
        let dimension = if retained {
            ResourceDimension::MaterializedBytes
        } else {
            ResourceDimension::CollectionItems
        };
        assert_context_refusal(dimension, operation, |ctx| {
            super::bounded_edge_for_curve(ir, curve_id, 0.0, None, ctx)
        });
    }
}

#[test]
fn rational_linear_degree_elevation_preserves_the_curve() {
    crate::test_support::with_service_context(&[], |decode_ctx| {
        let mut curve = test_nurbs(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)],
            Some(vec![1.0, 3.0]),
        );
        let before = cadmpeg_ir::eval::decode::nurbs_curve_point_at(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &curve,
            0.25,
        )
        .expect("valid rational linear NURBS evaluates before degree elevation");
        elevate_nurbs_to_degree(decode_ctx, &mut curve, [0.0, 1.0], 2, None)
            .expect("elevation lanes pair");
        let after = cadmpeg_ir::eval::decode::nurbs_curve_point_at(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &curve,
            0.25,
        )
        .expect("valid rational quadratic NURBS evaluates after degree elevation");
        assert!(before.distance(after.get()) <= 1.0e-12);
        assert_eq!(curve.control_points()[1], Point3::new(1.5, 0.0, 0.0));
        assert_eq!(curve.pole_rows().weights(), Some(vec![1.0, 2.0, 3.0]));
    });
}

#[test]
fn homogeneous_control_points_refuse_collection_limit() {
    let curve = test_nurbs(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        None,
    );
    let error = assert_context_refusal(
        ResourceDimension::CollectionItems,
        "iges composite homogeneous control points",
        |ctx| homogeneous_control_points(ctx, &curve).map(|_| ()),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.additional == 2));
}

#[test]
fn composite_knot_insertion_refuses_knot_collection_limit() {
    let controls = [[1.0, 0.0, 0.0, 0.0], [1.0, 1.0, 0.0, 0.0]];
    let knots = [0.0, 0.0, 1.0, 1.0];
    let error = assert_context_refusal(
        ResourceDimension::CollectionItems,
        "iges composite inserted knots",
        |ctx| insert_homogeneous_knot(ctx, &controls, &knots, 1, 0.5).map(|_| ()),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.additional == 5));
}

#[test]
fn composite_knot_insertion_refuses_control_collection_limit() {
    let controls = [[1.0, 0.0, 0.0, 0.0], [1.0, 1.0, 0.0, 0.0]];
    let knots = [0.0, 0.0, 1.0, 1.0];
    let error = assert_context_refusal(
        ResourceDimension::CollectionItems,
        "iges composite knot-insertion control points",
        |ctx| insert_homogeneous_knot(ctx, &controls, &knots, 1, 0.5).map(|_| ()),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.additional == 3));
}

#[test]
fn composite_elevated_knots_refuse_collection_limit() {
    let curve = test_nurbs(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        Some(vec![1.0, 2.0]),
    );
    let error = assert_context_refusal(
        ResourceDimension::CollectionItems,
        "iges composite elevated knots",
        |ctx| {
            elevate_nurbs_to_degree(ctx, &mut curve.clone(), [0.0, 1.0], 2, None)
                .map_err(|error| error.non_resource().unwrap_err())
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.additional == 3));
}

#[test]
fn composite_trimmed_lanes_refuse_output_and_scratch_collections() {
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
        "iges composite trimmed knots",
        "iges composite Euclidean control points",
        "iges composite Euclidean weights",
    ] {
        assert_context_refusal(ResourceDimension::CollectionItems, operation, |ctx| {
            trim_nurbs_to_interval(ctx, &curve, [0.25, 1.5])
                .map_err(|error| error.non_resource().unwrap_err())
        });
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
        "iges composite elevated span",
    ] {
        assert_context_refusal(ResourceDimension::CollectionItems, operation, |ctx| {
            elevate_nurbs_to_degree(ctx, &mut source(), [0.0, 1.0], 3, None)
                .map_err(|error| error.non_resource().unwrap_err())
        });
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
    let error = assert_context_refusal(
        ResourceDimension::CollectionItems,
        "iges composite child weights",
        |ctx| {
            concatenate_nurbs(ctx, vec![(curve.clone(), [0.0, 1.0], ())], None)
                .map(|_| ())
                .map_err(|error| error.non_resource().unwrap_err())
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.additional == 2));
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
        (true, "iges composite child control points"),
        (true, "iges composite child weight copy"),
        (false, "iges composite segment slots"),
        (false, "iges composite joined knots"),
        (false, "iges composite joined controls"),
        (false, "iges composite joined weights"),
        (true, "iges composite joined weighted poles"),
    ] {
        assert_context_refusal(ResourceDimension::CollectionItems, operation, |ctx| {
            concatenate_nurbs(ctx, children(rational), Some(0.0))
                .map(|_| ())
                .map_err(|error| error.non_resource().unwrap_err())
        });
    }
    assert_context_refusal(
        ResourceDimension::MaterializedBytes,
        "iges composite segment slots",
        |ctx| {
            concatenate_nurbs(ctx, children(false), Some(0.0))
                .map(|_| ())
                .map_err(|error| error.non_resource().unwrap_err())
        },
    );
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert!(concatenate_nurbs(&ctx, children(true), Some(0.0))
        .unwrap()
        .is_some());
}

mod attachments;
mod carrier_projection;

#[test]
fn bezier_degree_elevation_refuses_loop_work() {
    let controls = [[1.0, 0.0, 0.0, 0.0], [1.0, 2.0, 0.0, 0.0]];
    let service_arena = DecodeArena::new();
    let service_policy = DecodePolicy::service();
    let service_ctx = DecodeContext::new(&service_arena, &service_policy, false);
    assert!(
        super::elevate_bezier_homogeneous(&service_ctx, &controls, 1, 2)
            .unwrap()
            .is_some()
    );
    let error = assert_context_refusal(
        ResourceDimension::WorkUnits,
        "iges composite Bezier degree elevation",
        |ctx| super::elevate_bezier_homogeneous(ctx, &controls, 1, 2).map(|_| ()),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
        && limit.operation == "iges composite Bezier degree elevation"));
}

fn assert_work_refusal<T>(
    operation: &str,
    run: impl Fn(&DecodeContext<'_>) -> Result<T, CodecError>,
) {
    assert_context_refusal(ResourceDimension::WorkUnits, operation, run);
}

#[test]
fn composite_trim_multiplicity_refuses_work_before_scan() {
    let curve = test_nurbs(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)],
        None,
    );
    assert_work_refusal("iges composite trim knot multiplicity", |ctx| {
        super::trim_nurbs_lanes(ctx, &curve, [0.25, 0.75])
    });
}

fn assert_context_refusal<T>(
    dimension: ResourceDimension,
    operation: &str,
    run: impl Fn(&DecodeContext<'_>) -> Result<T, CodecError>,
) -> CodecError {
    cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = cap,
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
            _ => panic!("unsupported test dimension"),
        }
        let arena = DecodeArena::new();
        let ctx = DecodeContext::new(&arena, &policy, false);
        run(&ctx)
    })
}

fn assert_decode_refusal(bytes: &[u8], dimension: ResourceDimension, operation: &str) {
    cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = cap,
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
            _ => panic!("unsupported test dimension"),
        }
        IgesCodec
            .decode(
                &mut Cursor::new(bytes),
                &DecodeOptions {
                    policy,
                    ..DecodeOptions::default()
                },
            )
            .map_err(|failure| match failure {
                DecodeFailure::Codec(error) => error,
                error => panic!("unexpected decode failure: {error}"),
            })
    });
}
