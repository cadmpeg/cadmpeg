// SPDX-License-Identifier: Apache-2.0

#![allow(clippy::unwrap_used)]

mod nurbs;

use crate::directory::UseFlag;

use std::io::Cursor;

use cadmpeg_core::decode::DecodeArena;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::decode::DecodePolicy;
use cadmpeg_ir::codec::Codec;
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
use crate::test_support::test_owned::owned_test_file;
use crate::test_support::test_owned::owned_test_file_with_global;
use crate::test_support::test_owned::owned_test_file_with_global_and_directory_fields;
use crate::test_support::test_owned::OwnedTestEntity;
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
fn composite_index_admits_added_entity_nodes_and_identity_keys() {
    let curve = CurveId::mint("test:model:curve#added").unwrap();
    let start = VertexId::mint("test:model:vertex#start").unwrap();
    let end = VertexId::mint("test:model:vertex#end").unwrap();
    let position = cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap();
    let mut index = CompositeIndex::default();
    index.add_model_entity(
        &curve,
        0,
        super::CompositeEdge {
            start: start.clone(),
            end: end.clone(),
            param_range: None,
        },
        [(start, position), (end, position)],
    );
    assert_eq!(index.curve_positions.len(), 1);
    assert_eq!(index.edges[&curve].len(), 1);
    assert_eq!(index.vertex_points.len(), 2);
}

use super::{
    bounded_nurbs_for_curve, bounded_nurbs_for_curve_with_tolerance, close, close_with_tolerance,
    composite_child_type_allowed, composite_line_font_valid, composite_logical_connector_use_valid,
    composite_minimum_child_count, composite_use_flag_valid, elevate_nurbs_to_degree,
    CompositeIndex,
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
        loss.code == IgesLossCode::GeometryNotProjected.kind()
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
        loss.code == IgesLossCode::GeometryNotProjected.kind()
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
        loss.code == IgesLossCode::DirectoryMetadataNoncanonical.kind()
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
            .filter(|loss| loss.code == IgesLossCode::GeometryNotProjected.kind())
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
        .any(|loss| { loss.code == IgesLossCode::GeometryNotProjected.kind() }));
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
fn decode_projects_v4_composites_with_canonical_and_noncanonical_line_fonts() {
    const GLOBAL_V4: &[u8] = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    for font in [0, 1] {
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
                    &[(1, font), (3, font), (5, font)],
                    &[],
                    &[],
                    &[],
                )),
                &DecodeOptions::default(),
            )
            .unwrap();

        assert_eq!(result.ir().model.procedural_curves.len(), 1);
        assert_eq!(
            result.report().losses.iter().any(|loss| {
                loss.message
                    .contains("Type 102 Line Font must be nonzero in IGES 4.0")
            }),
            font == 0
        );
    }
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
            parameter_range: None,
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
fn decode_skips_a_composite_child_count_over_its_projection_limit() {
    let decoded = IgesCodec
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
        .unwrap();

    assert!(decoded.ir().model.curves.is_empty());
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == IgesLossCode::GeometryNotProjected.kind()
            && loss.message.contains("child count is outside")
            && loss.message.contains("..=100000")
    }));
}

#[test]
fn composite_flattening_over_its_depth_limit_is_refused_without_fusing_the_session() {
    let base_id = CurveId::mint("test:model:curve#base").expect("identity grammar");
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        parameter_range: None,
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
            parameter_range: None,
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
    // the limit it exceeds, and the caller reports it as a geometry loss.
    let error = bounded_nurbs_for_curve(&ir, &child_id, &ctx, None)
        .expect_err("a composite deeper than the limit is refused");
    assert!(matches!(
        error,
        super::CompositeCurveError::NestingLimit { limit: 64 }
    ));
    ctx.finish_session().unwrap();
}

#[test]
fn bounded_line_carrier_selects_a_curve_valid_edge_occurrence() {
    crate::test_support::with_service_context(&[], |decode_ctx| {
        let curve_id = CurveId::mint("test:model:curve#line").expect("identity grammar");
        let mut ir = CadIr::empty();
        ir.model.curves.push(Curve {
            parameter_range: None,
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
            parameter_range: None,
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
                parameter_range: None,
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

        let index = CompositeIndex::from_ir(&ir);
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

mod attachments;
mod carrier_projection;
