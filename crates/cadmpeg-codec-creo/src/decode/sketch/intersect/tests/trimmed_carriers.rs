// SPDX-License-Identifier: Apache-2.0

use super::super::{
    resolved_trim_vertex_coordinates, trimmed_section_segment_geometry_with_missing_line,
};
use crate::decode::sketch::coordinates::resolved_section_points;
use crate::decode::sketch::geometry::saved_section_missing_line_geometry;
use crate::decode::tests::declared_solver_rows;
use cadmpeg_ir::scalar::{Angle, Length};
use cadmpeg_ir::sketches::{SketchGeometry, SketchGeometryDefinition};
use std::collections::BTreeMap;

fn trimmed_section_segment_geometry(
    definition: &crate::feature::definitions::FeatureDefinition,
    points: &BTreeMap<u32, [f64; 2]>,
    trim_vertices: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::definitions::FeatureSegment,
) -> Option<SketchGeometry> {
    let missing_line = saved_section_missing_line_geometry(definition);
    trimmed_section_segment_geometry_with_missing_line(
        definition,
        points,
        trim_vertices,
        segment,
        missing_line.as_ref(),
    )
}

#[test]
fn trimmed_line_reconciles_carrier_and_solver_orientation() {
    let segment = crate::feature::definitions::FeatureSegment {
        kind: crate::feature::definitions::FeatureSegmentKind::Line([7, 9]),
        directions: [None; 3],
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 42,
        body: Vec::new(),
        offset: 40,
    };
    let anchor = crate::feature::definitions::FeatureSegment {
        kind: crate::feature::definitions::FeatureSegmentKind::Line([5, 6]),
        external_id: 41,
        body: Vec::new(),
        offset: 39,
        ..segment.clone()
    };
    let horizontal = crate::feature::definitions::FeatureSkamp {
        id: 1,
        kind: 1,
        flags: 0,
        status: 1,
        items: vec![crate::feature::definitions::FeatureSkampItem {
            entity_id: 41,
            sense: 0,
        }],
        offset: 50,
    };
    let parallel = crate::feature::definitions::FeatureSkamp {
        id: 2,
        kind: 7,
        flags: 0,
        status: 1,
        items: vec![
            crate::feature::definitions::FeatureSkampItem {
                entity_id: 41,
                sense: 0,
            },
            crate::feature::definitions::FeatureSkampItem {
                entity_id: 42,
                sense: 0,
            },
        ],
        offset: 55,
    };
    let mut definition = crate::feature::definitions::FeatureDefinition {
        identity: crate::feature::definitions::DefinitionIdentity::Parsed {
            schema_id: std::num::NonZeroU32::new(5),
            owner_feature_id: Some(6),
        },
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: Some(crate::feature::definitions::FeatureSegmentTable {
            declared_count: 2,
            has_elided_prototype: false,
            entity_ref: None,
            rows: (vec![anchor, segment.clone()])
                .into_iter()
                .map(crate::feature::segment_rows::SegmentRow::Ordinary)
                .collect(),
            offset: 20,
        }),
        trim_entities: Some(crate::feature::definitions::FeatureTrimEntityTable {
            declared_count: None,
            entity_ref: None,
            entry_ref: None,
            buckets: Vec::new(),
            rows: vec![crate::feature::definitions::FeatureTrimEntity {
                external_id: 42,
                mode: Some(0),
                vertices: [1, 2],
                kind: crate::feature::definitions::TrimEntityKind::Line,
                offset: 30,
            }],
            solved_external_ids: vec![42],
            offset: 28,
        }),
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: Some(crate::feature::definitions::FeatureRelationTable {
            declared_count: 2,
            entity_ref: None,
            rows: Vec::new(),
            skamps: Some(crate::feature::definitions::SolverSubtable::Declared {
                header: crate::feature::definitions::FeatureSolverTableHeader {
                    declared_count: 2,
                    entity_ref: 70,
                    offset: 45,
                },
                rows: vec![horizontal, parallel],
            }),
            triples: None,
            offset: 44,
        }),
        saved_section: None,
        offset: 0,
    };
    let trim_vertices = BTreeMap::from([(1, [-2.0, 3.0]), (2, [4.0, 3.0])]);

    assert_eq!(
        trimmed_section_segment_geometry(&definition, &BTreeMap::new(), &trim_vertices, &segment,),
        Some(
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: cadmpeg_ir::math::Point2::new(-2.0, 3.0),
                end: cadmpeg_ir::math::Point2::new(4.0, 3.0),
            })
            .expect("valid test fixture")
        )
    );
    let mut disabled_parallel = definition.clone();
    disabled_parallel
        .relations
        .as_mut()
        .expect("solver relations")
        .skamps
        .as_mut()
        .expect("skamp table")
        .rows_mut()[1]
        .status = 34;
    assert_eq!(
        trimmed_section_segment_geometry(
            &disabled_parallel,
            &BTreeMap::new(),
            &trim_vertices,
            &segment,
        ),
        None
    );

    let carrier_points = BTreeMap::from([(7, [0.0, 3.0]), (9, [2.0, 3.0])]);
    assert!(trimmed_section_segment_geometry(
        &definition,
        &carrier_points,
        &trim_vertices,
        &segment,
    )
    .is_some());
    let off_carrier_vertices = BTreeMap::from([(1, [-2.0, 3.0]), (2, [4.0, 4.0])]);
    assert!(trimmed_section_segment_geometry(
        &definition,
        &carrier_points,
        &off_carrier_vertices,
        &segment,
    )
    .is_none());

    let relations = definition.relations.as_mut().expect("solver relations");
    declared_solver_rows(&mut relations.skamps).push(crate::feature::definitions::FeatureSkamp {
        id: 3,
        kind: 2,
        flags: 0,
        status: 1,
        items: vec![crate::feature::definitions::FeatureSkampItem {
            entity_id: 41,
            sense: 0,
        }],
        offset: 60,
    });
    relations
        .skamps
        .as_mut()
        .expect("skamp table")
        .header_mut()
        .expect("solver header")
        .declared_count = 3;
    assert!(trimmed_section_segment_geometry(
        &definition,
        &BTreeMap::new(),
        &trim_vertices,
        &segment,
    )
    .is_none());
}

#[test]
fn arc_carriers_use_trim_vertices() {
    let segment = crate::feature::definitions::FeatureSegment {
        kind: crate::feature::definitions::FeatureSegmentKind::Arc([7, 9]),
        directions: [None; 3],
        center_id: Some(8),
        arc_orientation: Some(0),
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 42,
        body: Vec::new(),
        offset: 40,
    };
    let definition = crate::feature::definitions::FeatureDefinition {
        identity: crate::feature::definitions::DefinitionIdentity::Parsed {
            schema_id: std::num::NonZeroU32::new(5),
            owner_feature_id: Some(6),
        },
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: None,
        trim_entities: Some(crate::feature::definitions::FeatureTrimEntityTable {
            declared_count: None,
            entity_ref: None,
            entry_ref: None,
            buckets: Vec::new(),
            rows: vec![crate::feature::definitions::FeatureTrimEntity {
                external_id: 42,
                mode: Some(0),
                vertices: [1, 2],
                kind: crate::feature::definitions::TrimEntityKind::Arc { center_vertex: 3 },
                offset: 30,
            }],
            solved_external_ids: vec![42],
            offset: 28,
        }),
        trim_vertices: None,
        order_table: Some(crate::feature::definitions::FeatureOrderTable {
            declared_count: 1,
            has_prototype: false,
            entity_ref: None,
            rows: vec![crate::feature::definitions::FeatureOrderRow {
                external_id: 42,
                internal_id: 3,
                bitmask: 0,
                offset: 10,
            }],
            offset: 8,
        }),
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: Some(crate::feature::definitions::FeatureSavedSection {
            entities: vec![crate::feature::definitions::FeatureSavedEntity::Arc(
                crate::feature::definitions::FeatureSavedArc {
                    entity_id: 3,
                    center: [Some(0.0), Some(0.0), Some(0.0)],
                    radius: Some(2.0),
                    endpoints: [[None; 3]; 2],
                    parameters: [None; 2],
                    body: Vec::new(),
                    offset: 20,
                },
            )],
            offset: 18,
        }),
        offset: 0,
    };
    let trim_vertices = BTreeMap::from([(1, [-2.0, 0.0]), (2, [0.0, -2.0])]);
    let points = BTreeMap::from([(7, [2.0, 0.0]), (8, [0.0, 0.0]), (9, [0.0, 2.0])]);

    assert_eq!(
        trimmed_section_segment_geometry(&definition, &points, &trim_vertices, &segment),
        Some(
            SketchGeometry::try_from(SketchGeometryDefinition::Arc {
                center: cadmpeg_ir::math::Point2::new(0.0, 0.0),
                radius: Length::new(2.0).expect("finite length fixture"),
                start_angle: Angle::new(-std::f64::consts::FRAC_PI_2)
                    .expect("finite angle fixture"),
                end_angle: Angle::new(std::f64::consts::PI).expect("finite angle fixture"),
            })
            .expect("valid test fixture")
        )
    );

    const SMALL_RADIUS: f64 = 1.0e-10;
    for radius in [SMALL_RADIUS, 1.0, 1e100] {
        let mut scaled = definition.clone();
        scaled.segments = Some(crate::feature::definitions::FeatureSegmentTable {
            declared_count: 1,
            has_elided_prototype: false,
            entity_ref: None,
            rows: vec![crate::feature::segment_rows::SegmentRow::Ordinary(
                segment.clone(),
            )]
            .into_iter()
            .collect(),
            offset: 0,
        });
        for factor in [1.0, 5.0, f64::INFINITY, f64::NAN] {
            let endpoints = [[-radius * factor, 0.0], [0.0, -radius * factor]];
            let saved = scaled.saved_section.as_mut().expect("saved arc fixture");
            let crate::feature::definitions::FeatureSavedEntity::Arc(arc) = &mut saved.entities[0]
            else {
                panic!("saved arc fixture");
            };
            arc.radius = Some(radius);
            arc.endpoints = endpoints.map(|[u, v]| [Some(u), Some(v), Some(0.0)]);
            let vertices = BTreeMap::from([(1, endpoints[0]), (2, endpoints[1])]);
            let geometry =
                trimmed_section_segment_geometry(&scaled, &BTreeMap::new(), &vertices, &segment);
            let resolved = resolved_trim_vertex_coordinates(&scaled, &BTreeMap::new());
            if factor == 1.0 {
                assert!(geometry.is_some(), "radius {radius}");
                assert_eq!(resolved, vertices);
            } else {
                assert!(geometry.is_none(), "radius {radius}, factor {factor}");
                assert!(resolved.is_empty(), "radius {radius}, factor {factor}");
            }
        }
    }

    let mut var_segment = segment.clone();
    var_segment.radius_ref = Some(10);
    let mut var_arc = definition;
    var_arc.variables = Some(crate::feature::definitions::test_support::with_points(
        crate::feature::definitions::FeatureVariableTable {
            declared_count: 0,
            entity_ref: None,
            rows: Vec::new(),
            offset: 5,
        },
        vec![
            crate::feature::definitions::FeatureSectionPoint {
                point_id: 7,
                u: Some(2.0),
                v: Some(0.0),
            },
            crate::feature::definitions::FeatureSectionPoint {
                point_id: 8,
                u: Some(0.0),
                v: Some(0.0),
            },
            crate::feature::definitions::FeatureSectionPoint {
                point_id: 9,
                u: Some(0.0),
                v: Some(2.0),
            },
        ],
    ));
    var_arc.segments = Some(crate::feature::definitions::FeatureSegmentTable {
        declared_count: 1,
        has_elided_prototype: false,
        entity_ref: None,
        rows: (vec![var_segment.clone()])
            .into_iter()
            .map(crate::feature::segment_rows::SegmentRow::Ordinary)
            .collect(),
        offset: 6,
    });
    var_arc.order_table = None;
    var_arc.saved_section = None;
    assert_eq!(
        trimmed_section_segment_geometry(
            &var_arc,
            &resolved_section_points(&var_arc),
            &trim_vertices,
            &var_segment,
        ),
        Some(
            SketchGeometry::try_from(SketchGeometryDefinition::Arc {
                center: cadmpeg_ir::math::Point2::new(0.0, 0.0),
                radius: Length::new(2.0).expect("finite length fixture"),
                start_angle: Angle::new(-std::f64::consts::FRAC_PI_2)
                    .expect("finite angle fixture"),
                end_angle: Angle::new(std::f64::consts::PI).expect("finite angle fixture"),
            })
            .expect("valid test fixture")
        )
    );
}
