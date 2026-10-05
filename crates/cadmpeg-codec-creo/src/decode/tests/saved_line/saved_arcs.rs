// SPDX-License-Identifier: Apache-2.0

use super::super::section_segment_intersection_carrier;
use super::resolved_section_segment_geometry;
use crate::decode::sketch::coordinates::{
    resolved_section_points, saved_section_coordinate_witnesses,
};
use crate::decode::sketch::geometry::{
    saved_section_arc, saved_section_arc_carrier, saved_section_segment_point_coordinates,
    SectionArcCarrier,
};
use crate::decode::sketch::intersect::resolved_trim_vertex_coordinates;
use crate::decode::sketch::radii::resolved_section_radii;
use crate::decode::sketch_transfer::identity::semantic_saved_section_entities;
use cadmpeg_ir::scalar::{Angle, Length};
use cadmpeg_ir::sketches::{SketchGeometry, SketchGeometryDefinition};
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn saved_arc_joins_through_order_table() {
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
        trim_entities: None,
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
                    endpoints: [
                        [Some(0.0), Some(-2.0), Some(0.0)],
                        [Some(-2.0), Some(0.0), Some(0.0)],
                    ],
                    parameters: [None; 2],
                    body: Vec::new(),
                    offset: 20,
                },
            )],
            offset: 18,
        }),
        offset: 0,
    };

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| saved_section_arc(ctx, &definition, &segment)).expect("admitted saved section geometry"),
        Some(crate::decode::sketch::geometry::SavedSectionArc {
            center: cadmpeg_ir::units::FinitePoint2::new(cadmpeg_ir::math::Point2::new(0.0, 0.0))
                .expect("finite center fixture"),
            radius: cadmpeg_ir::scalar::PositiveLength::new(2.0).expect("positive length fixture"),
            start_angle: Angle::new(std::f64::consts::PI).expect("finite angle fixture"),
            end_angle: Angle::new(3.0 * std::f64::consts::FRAC_PI_2).expect("finite angle fixture"),
        })
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| saved_section_segment_point_coordinates(ctx, &definition, &segment)).expect("admitted saved section geometry")
            .map(|points| points.into_iter().flatten().collect::<Vec<_>>()),
        Some(vec![(7, [0.0, -2.0]), (9, [-2.0, 0.0]), (8, [0.0, 0.0]),])
    );
    let mut witness_definition = definition.clone();
    witness_definition.segments = Some(crate::feature::definitions::FeatureSegmentTable {
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
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (limited_ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("test input admitted");
    assert!(
        matches!(saved_section_coordinate_witnesses(&limited_ctx, &witness_definition, &BTreeSet::new()),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "creo saved section coordinate witnesses")
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| saved_section_coordinate_witnesses(
            ctx,
            &witness_definition,
            &BTreeSet::new()
        ))
        .expect("saved witnesses admitted"),
        vec![(7, [0.0, -2.0]), (9, [-2.0, 0.0]), (8, [0.0, 0.0])]
    );
    let mut coordinate_definition = definition.clone();
    coordinate_definition.variables = Some(crate::feature::definitions::test_support::with_points(
        crate::feature::definitions::FeatureVariableTable {
            declared_count: 0,
            entity_ref: None,
            rows: Vec::new(),
            offset: 30,
        },
        [7, 8, 9]
            .map(
                |point_id| crate::feature::definitions::FeatureSectionPoint {
                    point_id,
                    u: None,
                    v: None,
                },
            )
            .to_vec(),
    ));
    coordinate_definition.segments = Some(crate::feature::definitions::FeatureSegmentTable {
        declared_count: 1,
        has_elided_prototype: false,
        entity_ref: None,
        rows: (vec![segment.clone()])
            .into_iter()
            .map(crate::feature::segment_rows::SegmentRow::Ordinary)
            .collect(),
        offset: 38,
    });
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(
            ctx,
            &coordinate_definition
        ))
        .expect("test section solve"),
        BTreeMap::from([(7, [0.0, -2.0]), (8, [0.0, 0.0]), (9, [-2.0, 0.0]),])
    );
    assert!(resolved_section_segment_geometry(
        &definition,
        &BTreeMap::from([(7, [0.0, -2.0]), (8, [0.0, 0.0]), (9, [-2.0, 0.0])]),
        &segment,
    )
    .is_some());
    assert!(resolved_section_segment_geometry(
        &definition,
        &BTreeMap::from([(7, [0.0, -3.0]), (8, [0.0, 0.0]), (9, [-3.0, 0.0])]),
        &segment,
    )
    .is_none());
    let mut duplicate_order_row = definition.clone();
    duplicate_order_row
        .order_table
        .as_mut()
        .expect("order table")
        .rows
        .push(crate::feature::definitions::FeatureOrderRow {
            external_id: 42,
            internal_id: 4,
            bitmask: 0,
            offset: 11,
        });
    assert_eq!(crate::decode::with_test_decode_ctx(|ctx| saved_section_arc(ctx, &duplicate_order_row, &segment)).expect("admitted saved section geometry"), None);
    let mut duplicate_saved_arc = definition.clone();
    let duplicate = duplicate_saved_arc
        .saved_section
        .as_ref()
        .expect("saved section")
        .entities[0]
        .clone();
    duplicate_saved_arc
        .saved_section
        .as_mut()
        .expect("saved section")
        .entities
        .push(duplicate);
    assert_eq!(crate::decode::with_test_decode_ctx(|ctx| saved_section_arc(ctx, &duplicate_saved_arc, &segment)).expect("admitted saved section geometry"), None);

    let segment_table = crate::feature::definitions::FeatureSegmentTable {
        declared_count: 2,
        has_elided_prototype: true,
        entity_ref: None,
        rows: (vec![segment.clone()])
            .into_iter()
            .map(crate::feature::segment_rows::SegmentRow::Ordinary)
            .collect(),
        offset: 38,
    };
    let mut elided_prototype = definition.clone();
    elided_prototype.segments = Some(segment_table.clone());
    let order = elided_prototype.order_table.as_mut().expect("order table");
    order.has_prototype = true;
    order.declared_count = 2;
    let mut prototype = elided_prototype
        .saved_section
        .as_ref()
        .expect("saved section")
        .entities[0]
        .clone();
    if let crate::feature::definitions::FeatureSavedEntity::Arc(arc) = &mut prototype {
        arc.center = [None; 3];
        arc.radius = None;
        arc.endpoints = [[None; 3]; 2];
        arc.offset = 18;
    }
    elided_prototype
        .saved_section
        .as_mut()
        .expect("saved section")
        .entities
        .insert(0, prototype);
    assert!(crate::decode::with_test_decode_ctx(|ctx| saved_section_arc(ctx, &elided_prototype, &segment)).expect("admitted saved section geometry").is_some());
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| semantic_saved_section_entities(ctx, &elided_prototype)
            .map(|(entities, _storage)| entities.len()))
        .expect("admitted semantic entities"),
        1
    );

    let mut complete_elided_prototype = elided_prototype.clone();
    let complete_arc = complete_elided_prototype
        .saved_section
        .as_ref()
        .expect("saved section")
        .entities[1]
        .clone();
    complete_elided_prototype
        .saved_section
        .as_mut()
        .expect("saved section")
        .entities[0] = complete_arc;
    if let crate::feature::definitions::FeatureSavedEntity::Arc(arc) =
        &mut complete_elided_prototype
            .saved_section
            .as_mut()
            .expect("saved section")
            .entities[0]
    {
        arc.offset = 18;
    }
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| semantic_saved_section_entities(ctx, &complete_elided_prototype)
            .map(|(entities, _storage)| entities.len()))
        .expect("admitted semantic entities"),
        1
    );

    let mut unique_at_table_origin = definition.clone();
    unique_at_table_origin.segments = Some(segment_table);
    let order = unique_at_table_origin
        .order_table
        .as_mut()
        .expect("order table");
    order.has_prototype = true;
    order.declared_count = 2;
    if let crate::feature::definitions::FeatureSavedEntity::Arc(arc) = &mut unique_at_table_origin
        .saved_section
        .as_mut()
        .expect("saved section")
        .entities[0]
    {
        arc.offset = 18;
    }
    assert!(crate::decode::with_test_decode_ctx(|ctx| saved_section_arc(ctx, &unique_at_table_origin, &segment)).expect("admitted saved section geometry").is_some());

    let mut trimmed = definition;
    trimmed.segments = Some(crate::feature::definitions::FeatureSegmentTable {
        declared_count: 1,
        has_elided_prototype: false,
        entity_ref: None,
        rows: (vec![segment])
            .into_iter()
            .map(crate::feature::segment_rows::SegmentRow::Ordinary)
            .collect(),
        offset: 38,
    });
    trimmed.trim_entities = Some(crate::feature::definitions::FeatureTrimEntityTable {
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
    });
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| {
            let radii = crate::decode::sketch::radii::resolved_section_radii(ctx, &trimmed)?;
            resolved_trim_vertex_coordinates(ctx, &trimmed, &BTreeMap::new(), &radii)
        })
        .expect("test section geometry"),
        BTreeMap::from([(1, [0.0, -2.0]), (2, [-2.0, 0.0])])
    );
    let mut conflicting_vertex = trimmed.clone();
    conflicting_vertex.trim_vertices = Some(crate::feature::definitions::FeatureTrimVertexTable {
        declared_count: None,
        entity_ref: None,
        entry_ref: None,
        buckets: Vec::new(),
        rows: vec![
            crate::feature::definitions::FeatureTrimVertex {
                vertex_id: 1,
                entities: vec![42, 43],
                section_coordinates: cadmpeg_ir::units::FinitePoint2::new(
                    cadmpeg_ir::math::Point2::new(0.0, -2.0),
                ),
                offset: 31,
            },
            crate::feature::definitions::FeatureTrimVertex {
                vertex_id: 1,
                entities: vec![42, 44],
                section_coordinates: cadmpeg_ir::units::FinitePoint2::new(
                    cadmpeg_ir::math::Point2::new(9.0, 9.0),
                ),
                offset: 32,
            },
        ],
        offset: 30,
    });
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| {
            let radii =
                crate::decode::sketch::radii::resolved_section_radii(ctx, &conflicting_vertex)?;
            resolved_trim_vertex_coordinates(ctx, &conflicting_vertex, &BTreeMap::new(), &radii)
        })
        .expect("test section geometry"),
        BTreeMap::from([(2, [-2.0, 0.0])])
    );
    if let crate::feature::definitions::FeatureSavedEntity::Arc(arc) = &mut trimmed
        .saved_section
        .as_mut()
        .expect("test definition has a saved section")
        .entities[0]
    {
        arc.center[1] = None;
        arc.radius = None;
    }
    let segment = &trimmed
        .segments
        .as_ref()
        .expect("test definition has a segment table")
        .rows
        .ordinary()
        .cloned()
        .collect::<Vec<_>>()[0];
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| saved_section_arc_carrier(ctx, &trimmed, segment)).expect("admitted saved section geometry").map(SectionArcCarrier::raw),
        Some(([0.0, 0.0], 2.0))
    );
    if let crate::feature::definitions::FeatureSavedEntity::Arc(arc) = &mut trimmed
        .saved_section
        .as_mut()
        .expect("test definition has a saved section")
        .entities[0]
    {
        arc.center[1] = Some(0.0);
        arc.radius = Some(2.0);
    }
    if let crate::feature::definitions::FeatureSavedEntity::Arc(arc) = &mut trimmed
        .saved_section
        .as_mut()
        .expect("test definition has a saved section")
        .entities[0]
    {
        arc.endpoints[0] = [None; 3];
    } else {
        panic!("test entity is an arc");
    }
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| {
            let radii = crate::decode::sketch::radii::resolved_section_radii(ctx, &trimmed)?;
            resolved_trim_vertex_coordinates(ctx, &trimmed, &BTreeMap::new(), &radii)
        })
        .expect("test section geometry"),
        BTreeMap::from([(2, [-2.0, 0.0])])
    );
    if let crate::feature::definitions::FeatureSavedEntity::Arc(arc) = &mut trimmed
        .saved_section
        .as_mut()
        .expect("test definition has a saved section")
        .entities[0]
    {
        arc.endpoints[1] = [None; 3];
    }
    let segment = &trimmed
        .segments
        .as_ref()
        .expect("test definition has a segment table")
        .rows
        .ordinary()
        .cloned()
        .collect::<Vec<_>>()[0];
    assert!(crate::decode::with_test_decode_ctx(|ctx| saved_section_arc(ctx, &trimmed, segment)).expect("admitted saved section geometry").is_none());
    assert_eq!(
        section_segment_intersection_carrier(
            &trimmed,
            &crate::decode::with_test_decode_ctx(|ctx| resolved_section_radii(ctx, &trimmed))
                .expect("test section solve"),
            &BTreeMap::new(),
            segment,
        ),
        Some(
            SketchGeometry::try_from(SketchGeometryDefinition::Arc {
                center: cadmpeg_ir::math::Point2::new(0.0, 0.0),
                radius: Length::new(2.0).expect("finite length fixture"),
                start_angle: Angle::new(0.0).expect("finite angle fixture"),
                end_angle: Angle::new(std::f64::consts::TAU).expect("finite angle fixture"),
            })
            .expect("valid test fixture")
        )
    );
}
