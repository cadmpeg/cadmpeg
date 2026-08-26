// SPDX-License-Identifier: Apache-2.0
//! Tests: saved line.

use super::{
    extruded_segment_surface, placed_section_curve_geometry, section_segment_intersection_carrier,
    section_skamp_constraints, trimmed_section_segment_geometry,
};
use crate::decode::sketch::{
    is_full_circle_geometry, resolved_section_coordinates, resolved_section_points,
    resolved_section_radii, resolved_section_segment_geometry, resolved_trim_vertex_coordinates,
    saved_profile_chains, saved_section_arc_carrier, saved_section_arc_geometry,
    saved_section_circle_values, saved_section_entity_geometry, saved_section_line_geometry,
    saved_section_missing_line_geometry, saved_section_segment_point_coordinates, trim_segment_id,
};
use crate::decode::sketch_transfer::{
    ambiguous_section_segment_external_ids, joined_relation_incidence_entities,
    materialized_saved_section_external_ids, relation_incidence_entities,
    saved_section_external_id, section_dimension_constraints, section_entity_external_ids,
    section_skamp_constraints_for_geometry, section_skamp_incidence_locus,
    section_skamp_point_locus, semantic_saved_section_entities, solver_only_section_entity_family,
    unique_saved_section_internal_ids, unique_section_incidence_curve_family,
    unresolved_saved_section_entity, SectionEntityIncidenceFamily,
};
use cadmpeg_ir::features::{Angle, Length};
use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{SketchConstraintDefinition, SketchEntityId, SketchGeometry, SketchId};
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn saved_line_joins_through_order_table() {
    let segment = crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Line,
        directions: [None; 3],
        point_ids: [7, 9],
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 42,
        body: Vec::new(),
        offset: 40,
    };
    let definition = crate::feature::FeatureDefinition {
        id: 5,
        owner_feature_id: Some(6),
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: Some(crate::feature::FeatureOrderTable {
            declared_count: 1,
            has_prototype: false,
            entity_ref: None,
            rows: vec![crate::feature::FeatureOrderRow {
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
        saved_section: Some(crate::feature::FeatureSavedSection {
            entities: vec![crate::feature::FeatureSavedEntity::Line(
                crate::feature::FeatureSavedLine {
                    entity_id: 3,
                    references: Vec::new(),
                    attributes: Vec::new(),
                    endpoints: [
                        [Some(-8.0), Some(-0.85), Some(0.0)],
                        [Some(8.0), Some(-0.85), None],
                    ],
                    body: Vec::new(),
                    offset: 20,
                },
            )],
            offset: 18,
        }),
        offset: 0,
    };

    assert_eq!(
        saved_section_line_geometry(&definition, &segment),
        Some(SketchGeometry::Line {
            start: cadmpeg_ir::math::Point2::new(-8.0, -0.85),
            end: cadmpeg_ir::math::Point2::new(8.0, -0.85),
        })
    );
    assert!(resolved_section_segment_geometry(
        &definition,
        &BTreeMap::from([(7, [-8.0, -0.85]), (9, [8.0, -0.85])]),
        &segment,
    )
    .is_some());
    assert!(resolved_section_segment_geometry(
        &definition,
        &BTreeMap::from([(7, [-8.0, -0.85]), (9, [8.0, 0.85])]),
        &segment,
    )
    .is_none());
    assert_eq!(
        section_entity_external_ids(&definition),
        BTreeSet::from([42])
    );
    assert_eq!(
        materialized_saved_section_external_ids(&definition),
        BTreeSet::from([42])
    );
    let mut coordinate_definition = definition.clone();
    coordinate_definition.variables = Some(crate::feature::FeatureVariableTable {
        declared_count: 0,
        entity_ref: None,
        rows: Vec::new(),
        points: vec![
            crate::feature::FeatureSectionPoint {
                point_id: 7,
                u: None,
                v: None,
            },
            crate::feature::FeatureSectionPoint {
                point_id: 9,
                u: None,
                v: None,
            },
        ],
        offset: 30,
    });
    coordinate_definition.segments = Some(crate::feature::FeatureSegmentTable {
        declared_count: 1,
        has_elided_prototype: false,
        entity_ref: None,
        rows: vec![segment.clone()],
        circle_rows: Vec::new(),
        point_rows: Vec::new(),
        centered_line_rows: Vec::new(),
        reference_line_rows: Vec::new(),
        bounded_curve_rows: Vec::new(),
        conic_rows: Vec::new(),
        opaque_rows: Vec::new(),
        offset: 38,
    });
    assert_eq!(
        resolved_section_points(&coordinate_definition),
        BTreeMap::from([(7, [-8.0, -0.85]), (9, [8.0, -0.85])])
    );
    coordinate_definition
        .variables
        .as_mut()
        .expect("variables")
        .points[0]
        .u = Some(7.0);
    assert_eq!(
        resolved_section_coordinates(&coordinate_definition),
        BTreeMap::from([(7, [Some(7.0), Some(-0.85)]), (9, [Some(8.0), Some(-0.85)]),])
    );
    let mut incomplete = definition.clone();
    let crate::feature::FeatureSavedEntity::Line(incomplete_line) = &mut incomplete
        .saved_section
        .as_mut()
        .expect("saved section")
        .entities[0]
    else {
        panic!("saved line");
    };
    incomplete_line.endpoints[1][1] = None;
    assert!(saved_section_entity_geometry(
        &incomplete
            .saved_section
            .as_ref()
            .expect("saved section")
            .entities[0]
    )
    .is_none());
    assert_eq!(
        section_entity_external_ids(&incomplete),
        BTreeSet::from([42])
    );
    assert!(materialized_saved_section_external_ids(&incomplete).is_empty());
    let (native_entity, offset) = unresolved_saved_section_entity(
        &incomplete,
        &SketchId("creo:model:sketch#5".into()),
        &incomplete
            .saved_section
            .as_ref()
            .expect("saved section")
            .entities[0],
        &unique_saved_section_internal_ids(&incomplete),
        &BTreeSet::new(),
    );
    assert_eq!(offset, 20);
    assert_eq!(native_entity.id.0, "creo:featdefs:sketch_entity#5:42");
    assert!(matches!(
        native_entity.geometry,
        SketchGeometry::Native { ref native_kind } if native_kind == "saved_line"
    ));
    let mut duplicate_order_row = definition.clone();
    duplicate_order_row
        .order_table
        .as_mut()
        .expect("order table")
        .rows
        .push(crate::feature::FeatureOrderRow {
            external_id: 42,
            internal_id: 4,
            bitmask: 0,
            offset: 11,
        });
    assert_eq!(
        saved_section_line_geometry(&duplicate_order_row, &segment),
        None
    );
    let mut duplicate_saved_line = definition.clone();
    let duplicate = duplicate_saved_line
        .saved_section
        .as_ref()
        .expect("saved section")
        .entities[0]
        .clone();
    duplicate_saved_line
        .saved_section
        .as_mut()
        .expect("saved section")
        .entities
        .push(duplicate);
    assert_eq!(
        saved_section_line_geometry(&duplicate_saved_line, &segment),
        None
    );
    assert_eq!(
        saved_section_external_id(
            definition.order_table.as_ref().expect("order table"),
            &unique_saved_section_internal_ids(&definition),
            &ambiguous_section_segment_external_ids(&definition),
            3,
        ),
        Some(42)
    );
    let mut constrained = definition.clone();
    constrained.segments = Some(crate::feature::FeatureSegmentTable {
        declared_count: 0,
        has_elided_prototype: false,
        entity_ref: None,
        rows: Vec::new(),
        circle_rows: Vec::new(),
        point_rows: Vec::new(),
        centered_line_rows: Vec::new(),
        reference_line_rows: Vec::new(),
        bounded_curve_rows: Vec::new(),
        conic_rows: Vec::new(),
        opaque_rows: Vec::new(),
        offset: 0,
    });
    constrained.dimensions = Some(crate::feature::FeatureDimensionTable {
        declared_count: 1,
        entity_ref: None,
        rows: vec![crate::feature::FeatureDimension {
            dimension_type: 1,
            value: Some(2.0),
            value_body: Vec::new(),
            unresolved_value_token: None,
            value_unit: crate::feature::DimensionUnit::Millimeters,
            direction_byte: 0,
            auxiliary_value: None,
            auxiliary_body: Vec::new(),
            external_id: 4,
            references: None,
            offset: 27,
        }],
        offset: 26,
    });
    constrained.relations = Some(crate::feature::FeatureRelationTable {
        declared_count: 3,
        entity_ref: None,
        rows: vec![crate::feature::FeatureRelation {
            relation_id: 7,
            used: 1,
            operands: Vec::new(),
            operand_vectors: Some([
                [Some(42), Some(99), None, Some(1)],
                [Some(0); 4],
                [Some(15), Some(16), Some(15), Some(1)],
            ]),
            sign: 0,
            dimension_id: 0,
            relation_type: 0,
            body: Vec::new(),
            offset: 28,
        }],
        skamps: vec![crate::feature::FeatureSkamp {
            id: 5,
            kind: 99,
            flags: 0,
            status: 1,
            items: vec![
                crate::feature::FeatureSkampItem {
                    entity_id: 42,
                    sense: 4,
                },
                crate::feature::FeatureSkampItem {
                    entity_id: 99,
                    sense: 0,
                },
            ],
            offset: 30,
        }],
        skamp_header: Some(crate::feature::FeatureSolverTableHeader {
            declared_count: 1,
            entity_ref: 1,
            offset: 29,
        }),
        triples: vec![crate::feature::FeatureRelationTriple {
            relation_id: Some(7),
            equation_id: Some(11),
            skamp_id: Some(5),
            offset: 31,
        }],
        triples_header: Some(crate::feature::FeatureSolverTableHeader {
            declared_count: 1,
            entity_ref: 2,
            offset: 31,
        }),
        offset: 28,
    });
    let constraints =
        section_skamp_constraints(&constrained, &SketchId("creo:model:sketch#5".to_string()));
    assert!(matches!(
        &constraints[0].0.definition,
        SketchConstraintDefinition::Native { entities, .. }
            if entities == &[SketchEntityId(
                "creo:featdefs:sketch_entity#5:42".to_string()
            )]
    ));
    let SketchConstraintDefinition::Native { operands, .. } = &constraints[0].0.definition else {
        unreachable!();
    };
    assert!(operands.iter().any(|operand| {
        operand.native_kind == "triples_ptr"
            && operand.native_field.as_deref() == Some("equation_id")
            && operand.native_role.is_none()
            && operand.object_index == 11
    }));
    let mut equation_only_incidence = constrained.clone();
    equation_only_incidence
        .relations
        .as_mut()
        .expect("relations")
        .triples[0]
        .relation_id = None;
    let equation_only_constraints = section_skamp_constraints(
        &equation_only_incidence,
        &SketchId("creo:model:sketch#5".to_string()),
    );
    let SketchConstraintDefinition::Native { operands, .. } =
        &equation_only_constraints[0].0.definition
    else {
        unreachable!();
    };
    assert!(operands.iter().any(|operand| {
        operand.native_field.as_deref() == Some("equation_id") && operand.object_index == 11
    }));
    let mut missing_equation = equation_only_incidence.clone();
    missing_equation
        .relations
        .as_mut()
        .expect("relations")
        .triples[0]
        .equation_id = None;
    let missing_equation_constraints = section_skamp_constraints(
        &missing_equation,
        &SketchId("creo:model:sketch#5".to_string()),
    );
    let SketchConstraintDefinition::Native { operands, .. } =
        &missing_equation_constraints[0].0.definition
    else {
        unreachable!();
    };
    assert!(!operands
        .iter()
        .any(|operand| operand.native_field.as_deref() == Some("equation_id")));
    let mut duplicate_equation = equation_only_incidence.clone();
    let duplicate_equation_relations = duplicate_equation.relations.as_mut().expect("relations");
    duplicate_equation_relations
        .triples
        .push(crate::feature::FeatureRelationTriple {
            relation_id: None,
            equation_id: Some(11),
            skamp_id: Some(5),
            offset: 32,
        });
    duplicate_equation_relations
        .triples_header
        .as_mut()
        .expect("triples header")
        .declared_count = 2;
    let duplicate_equation_constraints = section_skamp_constraints(
        &duplicate_equation,
        &SketchId("creo:model:sketch#5".to_string()),
    );
    let SketchConstraintDefinition::Native { operands, .. } =
        &duplicate_equation_constraints[0].0.definition
    else {
        unreachable!();
    };
    assert!(!operands
        .iter()
        .any(|operand| operand.native_field.as_deref() == Some("equation_id")));
    assert_eq!(
        relation_incidence_entities(
            &constrained,
            &SketchId("creo:model:sketch#5".to_string()),
            7,
        ),
        vec![
            SketchEntityId("creo:featdefs:sketch_entity#5:42".to_string()),
            SketchEntityId("creo:featdefs:sketch_entity#5:99".to_string()),
        ]
    );
    let dimension_constraints =
        section_dimension_constraints(&constrained, &SketchId("creo:model:sketch#5".to_string()));
    assert!(
        matches!(
            &dimension_constraints[0].0.definition,
            SketchConstraintDefinition::Distance { entities, .. }
                if entities == &[
                    SketchEntityId("creo:featdefs:sketch_entity#5:42".to_string()),
                    SketchEntityId("creo:featdefs:sketch_entity#5:99".to_string()),
                ]
        ),
        "{:?}",
        dimension_constraints[0].0.definition
    );
    let mut native_join = constrained.clone();
    native_join.relations.as_mut().expect("relations").rows[0].relation_type = 99;
    let native_join_constraints =
        section_dimension_constraints(&native_join, &SketchId("creo:model:sketch#5".to_string()));
    let SketchConstraintDefinition::Native { operands, .. } =
        &native_join_constraints[0].0.definition
    else {
        panic!("untyped relation must remain native");
    };
    assert!(operands.iter().any(|operand| {
        operand.native_kind == "skamp_ptr"
            && operand.native_field.as_deref() == Some("triples_ptr.skamp_id")
            && operand.object_index == 5
    }));
    assert!(operands.iter().any(|operand| {
        operand.native_kind == "triples_ptr"
            && operand.native_field.as_deref() == Some("equation_id")
            && operand.object_index == 11
    }));
    native_join
        .relations
        .as_mut()
        .expect("relations")
        .triples
        .push(crate::feature::FeatureRelationTriple {
            relation_id: Some(7),
            equation_id: None,
            skamp_id: Some(5),
            offset: 32,
        });
    native_join
        .relations
        .as_mut()
        .expect("relations")
        .triples_header
        .as_mut()
        .expect("triples header")
        .declared_count = 2;
    let ambiguous_join_constraints =
        section_dimension_constraints(&native_join, &SketchId("creo:model:sketch#5".to_string()));
    let SketchConstraintDefinition::Native { operands, .. } =
        &ambiguous_join_constraints[0].0.definition
    else {
        panic!("untyped relation must remain native");
    };
    assert!(!operands
        .iter()
        .any(|operand| operand.native_field.as_deref() == Some("triples_ptr.skamp_id")));
    assert!(!operands
        .iter()
        .any(|operand| operand.native_field.as_deref() == Some("equation_id")));
    let mut solver_families = constrained.clone();
    let family_relations = solver_families.relations.as_mut().expect("relations");
    family_relations.skamps = vec![crate::feature::FeatureSkamp {
        id: 6,
        kind: 0,
        flags: 0,
        status: 0,
        items: vec![
            crate::feature::FeatureSkampItem {
                entity_id: 99,
                sense: 2,
            },
            crate::feature::FeatureSkampItem {
                entity_id: 100,
                sense: 3,
            },
        ],
        offset: 32,
    }];
    assert_eq!(
        solver_only_section_entity_family(&solver_families, 99),
        Some(SectionEntityIncidenceFamily::BoundedCurve)
    );
    solver_families
        .relations
        .as_mut()
        .expect("relations")
        .skamps[0]
        .items = vec![
        crate::feature::FeatureSkampItem {
            entity_id: 99,
            sense: 0,
        },
        crate::feature::FeatureSkampItem {
            entity_id: 12,
            sense: 2,
        },
    ];
    solver_families
        .relations
        .as_mut()
        .expect("relations")
        .skamps[0]
        .status = 0;
    assert_eq!(
        solver_only_section_entity_family(&solver_families, 99),
        Some(SectionEntityIncidenceFamily::Point)
    );
    solver_families
        .relations
        .as_mut()
        .expect("relations")
        .skamps[0]
        .items[0]
        .sense = 4;
    assert_eq!(
        solver_only_section_entity_family(&solver_families, 99),
        Some(SectionEntityIncidenceFamily::Circular)
    );
    solver_families
        .relations
        .as_mut()
        .expect("relations")
        .skamps[0]
        .items[0]
        .sense = 2;
    solver_families
        .relations
        .as_mut()
        .expect("relations")
        .skamps[0]
        .status = 1;
    assert_eq!(
        solver_only_section_entity_family(&solver_families, 99),
        Some(SectionEntityIncidenceFamily::BoundedCurve)
    );
    let family_relations = solver_families.relations.as_mut().expect("relations");
    family_relations.skamps = vec![crate::feature::FeatureSkamp {
        id: 6,
        kind: 1,
        flags: 0,
        status: 0,
        items: vec![crate::feature::FeatureSkampItem {
            entity_id: 99,
            sense: 0,
        }],
        offset: 32,
    }];
    assert_eq!(
        solver_only_section_entity_family(&solver_families, 99),
        Some(SectionEntityIncidenceFamily::Line)
    );
    let solver_geometry = BTreeMap::from([(
        SketchEntityId("creo:featdefs:sketch_entity#5:99".to_string()),
        SketchGeometry::Native {
            native_kind: "solver_only_section_entity".to_string(),
        },
    )]);
    assert!(matches!(
        section_skamp_constraints_for_geometry(
            &solver_families,
            &SketchId("creo:model:sketch#5".to_string()),
            Some(&solver_geometry),
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Horizontal { .. }
    ));
    let unary = &mut solver_families
        .relations
        .as_mut()
        .expect("relations")
        .skamps[0];
    unary.kind = 2;
    unary.status = 1;
    assert!(matches!(
        section_skamp_constraints_for_geometry(
            &solver_families,
            &SketchId("creo:model:sketch#5".to_string()),
            Some(&solver_geometry),
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Vertical { .. }
    ));
    let family_relations = solver_families.relations.as_mut().expect("relations");
    family_relations.skamps = vec![crate::feature::FeatureSkamp {
        id: 6,
        kind: 0,
        flags: 0,
        status: 0,
        items: vec![
            crate::feature::FeatureSkampItem {
                entity_id: 99,
                sense: 0,
            },
            crate::feature::FeatureSkampItem {
                entity_id: 42,
                sense: 2,
            },
        ],
        offset: 32,
    }];
    assert_eq!(
        solver_only_section_entity_family(&solver_families, 99),
        Some(SectionEntityIncidenceFamily::Point)
    );
    solver_families
        .relations
        .as_mut()
        .expect("relations")
        .skamps[0]
        .status = 1;
    assert_eq!(
        solver_only_section_entity_family(&solver_families, 99),
        Some(SectionEntityIncidenceFamily::Point)
    );
    let solver_geometry = BTreeMap::from([
        (
            SketchEntityId("creo:featdefs:sketch_entity#5:42".to_string()),
            SketchGeometry::Native {
                native_kind: "line".to_string(),
            },
        ),
        (
            SketchEntityId("creo:featdefs:sketch_entity#5:99".to_string()),
            SketchGeometry::Native {
                native_kind: "point".to_string(),
            },
        ),
    ]);
    let solver_constraints = section_skamp_constraints_for_geometry(
        &solver_families,
        &SketchId("creo:model:sketch#5".to_string()),
        Some(&solver_geometry),
    );
    let point_item = &solver_families
        .relations
        .as_ref()
        .expect("relations")
        .skamps[0]
        .items[0];
    let line_item = &solver_families
        .relations
        .as_ref()
        .expect("relations")
        .skamps[0]
        .items[1];
    assert!(section_skamp_point_locus(
        &solver_families,
        &SketchId("creo:model:sketch#5".to_string()),
        point_item
    )
    .is_some());
    assert!(section_skamp_incidence_locus(
        &solver_families,
        &SketchId("creo:model:sketch#5".to_string()),
        line_item,
        Some(&solver_geometry)
    )
    .is_some());
    assert!(
        matches!(
            solver_constraints[0].0.definition,
            SketchConstraintDefinition::CoincidentLoci { .. }
        ),
        "{:?}",
        solver_constraints[0].0.definition
    );
    let family_relations = solver_families.relations.as_mut().expect("relations");
    family_relations.skamps = vec![crate::feature::FeatureSkamp {
        id: 6,
        kind: 6,
        flags: 0,
        status: 1,
        items: vec![
            crate::feature::FeatureSkampItem {
                entity_id: 99,
                sense: 0,
            },
            crate::feature::FeatureSkampItem {
                entity_id: 100,
                sense: 0,
            },
        ],
        offset: 33,
    }];
    family_relations
        .skamp_header
        .as_mut()
        .expect("skamp header")
        .declared_count = 1;
    assert_eq!(
        solver_only_section_entity_family(&solver_families, 99),
        Some(SectionEntityIncidenceFamily::Circular)
    );
    let mut disabled_line_family = solver_families.clone();
    let disabled_line_relations = disabled_line_family.relations.as_mut().expect("relations");
    disabled_line_relations.skamps[0] = crate::feature::FeatureSkamp {
        id: 7,
        kind: 5,
        flags: 0,
        status: 0,
        items: vec![
            crate::feature::FeatureSkampItem {
                entity_id: 99,
                sense: 0,
            },
            crate::feature::FeatureSkampItem {
                entity_id: 101,
                sense: 0,
            },
        ],
        offset: 34,
    };
    assert_eq!(
        solver_only_section_entity_family(&disabled_line_family, 99),
        Some(SectionEntityIncidenceFamily::Line)
    );
    let mut disabled_circular_family = constrained.clone();
    disabled_circular_family
        .segments
        .as_mut()
        .expect("segments")
        .declared_count = 1;
    disabled_circular_family
        .segments
        .as_mut()
        .expect("segments")
        .opaque_rows
        .push(crate::feature::FeatureOpaqueSegment {
            kind: 25,
            directions: [None; 3],
            point_ids: [None; 2],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id: 101,
            body: Vec::new(),
            offset: 35,
        });
    let disabled_circular_relations = disabled_circular_family
        .relations
        .as_mut()
        .expect("relations");
    disabled_circular_relations.skamps = vec![crate::feature::FeatureSkamp {
        id: 8,
        kind: 99,
        flags: 0,
        status: 0,
        items: vec![crate::feature::FeatureSkampItem {
            entity_id: 101,
            sense: 4,
        }],
        offset: 35,
    }];
    disabled_circular_relations
        .skamp_header
        .as_mut()
        .expect("skamp header")
        .declared_count = 1;
    assert_eq!(
        unique_section_incidence_curve_family(&disabled_circular_family, 101),
        Some(SectionEntityIncidenceFamily::Circular)
    );
    let family_relations = solver_families.relations.as_mut().expect("relations");
    family_relations.skamps.push(crate::feature::FeatureSkamp {
        id: 7,
        kind: 5,
        flags: 0,
        status: 1,
        items: vec![
            crate::feature::FeatureSkampItem {
                entity_id: 99,
                sense: 0,
            },
            crate::feature::FeatureSkampItem {
                entity_id: 101,
                sense: 0,
            },
        ],
        offset: 34,
    });
    family_relations
        .skamp_header
        .as_mut()
        .expect("skamp header")
        .declared_count = 2;
    assert_eq!(
        solver_only_section_entity_family(&solver_families, 99),
        None
    );
    let mut duplicate_incidence = constrained.clone();
    let duplicate_relations = duplicate_incidence.relations.as_mut().expect("relations");
    let mut duplicate = duplicate_relations.skamps[0].clone();
    duplicate.status = 34;
    duplicate.offset = 32;
    duplicate_relations.skamps.push(duplicate);
    duplicate_relations
        .skamp_header
        .as_mut()
        .expect("skamp header")
        .declared_count = 2;
    assert!(relation_incidence_entities(
        &duplicate_incidence,
        &SketchId("creo:model:sketch#5".to_string()),
        7,
    )
    .is_empty());
    constrained.relations.as_mut().expect("relations").skamps[0].status = 34;
    assert!(relation_incidence_entities(
        &constrained,
        &SketchId("creo:model:sketch#5".to_string()),
        7,
    )
    .is_empty());
    assert_eq!(
        joined_relation_incidence_entities(
            &constrained,
            &SketchId("creo:model:sketch#5".to_string()),
            7,
        ),
        vec![
            SketchEntityId("creo:featdefs:sketch_entity#5:42".to_string()),
            SketchEntityId("creo:featdefs:sketch_entity#5:99".to_string()),
        ]
    );
    assert_eq!(
        section_skamp_constraints(&constrained, &SketchId("creo:model:sketch#5".to_string()))[0]
            .0
            .active,
        Some(false)
    );
    constrained.segments = None;
    let constraints =
        section_skamp_constraints(&constrained, &SketchId("creo:model:sketch#5".to_string()));
    assert!(matches!(
        &constraints[0].0.definition,
        SketchConstraintDefinition::Native { entities, .. }
            if entities == &[SketchEntityId(
                "creo:featdefs:sketch_entity#5:42".to_string()
            )]
    ));

    let mut completed = definition;
    completed
        .order_table
        .as_mut()
        .expect("test definition has an order table")
        .rows
        .clear();
    completed
        .order_table
        .as_mut()
        .expect("test definition has an order table")
        .declared_count = 0;
    completed.segments = Some(crate::feature::FeatureSegmentTable {
        declared_count: 1,
        has_elided_prototype: false,
        entity_ref: None,
        rows: vec![segment.clone()],
        circle_rows: Vec::new(),
        point_rows: Vec::new(),
        centered_line_rows: Vec::new(),
        reference_line_rows: Vec::new(),
        bounded_curve_rows: Vec::new(),
        conic_rows: Vec::new(),
        opaque_rows: Vec::new(),
        offset: 4,
    });
    completed.trim_entities = Some(crate::feature::FeatureTrimEntityTable {
        declared_count: None,
        entity_ref: None,
        entry_ref: None,
        buckets: Vec::new(),
        rows: vec![crate::feature::FeatureTrimEntity {
            external_id: 42,
            mode: Some(0),
            vertices: [1, 2],
            center_vertex: None,
            kind: crate::feature::TrimEntityKind::Line,
            offset: 6,
        }],
        solved_external_ids: vec![42],
        offset: 5,
    });
    assert_eq!(
        saved_section_line_geometry(&completed, &segment),
        Some(SketchGeometry::Line {
            start: cadmpeg_ir::math::Point2::new(-8.0, -0.85),
            end: cadmpeg_ir::math::Point2::new(8.0, -0.85),
        })
    );
    let mut replay_mismatched = completed.clone();
    replay_mismatched
        .trim_entities
        .as_mut()
        .expect("trim table")
        .rows[0]
        .external_id = 99;
    assert_eq!(
        trim_segment_id(
            &replay_mismatched,
            &replay_mismatched
                .trim_entities
                .as_ref()
                .expect("trim table")
                .rows[0],
        ),
        Some(42)
    );
    assert_eq!(
        saved_section_line_geometry(&replay_mismatched, &segment),
        Some(SketchGeometry::Line {
            start: cadmpeg_ir::math::Point2::new(-8.0, -0.85),
            end: cadmpeg_ir::math::Point2::new(8.0, -0.85),
        })
    );
    let mut incomplete_order = completed.clone();
    incomplete_order
        .order_table
        .as_mut()
        .expect("test definition has an order table")
        .declared_count = 1;
    assert_eq!(
        saved_section_line_geometry(&incomplete_order, &segment),
        None
    );
    let mut incomplete_segments = completed.clone();
    incomplete_segments
        .segments
        .as_mut()
        .expect("segment table")
        .declared_count = 2;
    assert_eq!(
        saved_section_line_geometry(&incomplete_segments, &segment),
        None
    );
    let trim = completed.trim_entities.as_ref().expect("trim table").rows[0].clone();
    assert_eq!(trim_segment_id(&completed, &trim), Some(42));

    let mut missing_line = completed.clone();
    missing_line
        .order_table
        .as_mut()
        .expect("order table")
        .declared_count = 1;
    missing_line
        .order_table
        .as_mut()
        .expect("order table")
        .rows
        .push(crate::feature::FeatureOrderRow {
            external_id: 42,
            internal_id: 3,
            bitmask: 0,
            offset: 10,
        });
    let mut omitted_segment = segment.clone();
    omitted_segment.external_id = 43;
    omitted_segment.point_ids = [11, 12];
    missing_line
        .segments
        .as_mut()
        .expect("segment table")
        .declared_count = 2;
    missing_line
        .segments
        .as_mut()
        .expect("segment table")
        .rows
        .push(omitted_segment.clone());
    missing_line
        .trim_entities
        .as_mut()
        .expect("trim table")
        .rows
        .push(crate::feature::FeatureTrimEntity {
            external_id: 43,
            mode: Some(0),
            vertices: [3, 4],
            center_vertex: None,
            kind: crate::feature::TrimEntityKind::Line,
            offset: 7,
        });
    missing_line
        .trim_entities
        .as_mut()
        .expect("trim table")
        .solved_external_ids
        .push(43);
    assert!(saved_section_missing_line_geometry(&missing_line).is_none());
    assert!(
        resolved_section_segment_geometry(&missing_line, &BTreeMap::new(), &omitted_segment,)
            .is_none()
    );

    omitted_segment.vertical_horizontal = Some(1);
    missing_line.segments.as_mut().expect("segment table").rows[1] = omitted_segment.clone();
    assert_eq!(
        saved_section_missing_line_geometry(&missing_line),
        Some((
            omitted_segment.offset,
            SketchGeometry::Line {
                start: cadmpeg_ir::math::Point2::new(-8.0, -0.85),
                end: cadmpeg_ir::math::Point2::new(8.0, -0.85),
            },
        ))
    );
    assert_eq!(
        resolved_section_segment_geometry(&missing_line, &BTreeMap::new(), &omitted_segment),
        Some(SketchGeometry::Line {
            start: cadmpeg_ir::math::Point2::new(-8.0, -0.85),
            end: cadmpeg_ir::math::Point2::new(8.0, -0.85),
        })
    );

    omitted_segment.vertical_horizontal = Some(0);
    missing_line.segments.as_mut().expect("segment table").rows[1] = omitted_segment;
    assert!(saved_section_missing_line_geometry(&missing_line).is_none());
    assert!(resolved_section_segment_geometry(
        &missing_line,
        &BTreeMap::new(),
        &missing_line.segments.as_ref().expect("segment table").rows[1],
    )
    .is_none());

    let mut duplicate_segment = completed.clone();
    duplicate_segment
        .segments
        .as_mut()
        .expect("segment table")
        .rows
        .push(segment);
    assert_eq!(trim_segment_id(&duplicate_segment, &trim), None);
    let mut duplicate_trim = completed;
    duplicate_trim
        .trim_entities
        .as_mut()
        .expect("trim table")
        .rows
        .push(trim.clone());
    assert_eq!(trim_segment_id(&duplicate_trim, &trim), None);
}

#[test]
fn saved_circle_defines_full_section_geometry_with_incomplete_segment_table() {
    let entity = crate::feature::FeatureSavedEntity::Circle(crate::feature::FeatureSavedCircle {
        entity_id: 7,
        center: [Some(2.0), Some(-3.0), Some(0.0)],
        radius: Some(4.5),
        body: Vec::new(),
        offset: 19,
    });

    assert_eq!(
        saved_section_entity_geometry(&entity),
        Some((
            7,
            SketchGeometry::Circle {
                center: Point2::new(2.0, -3.0),
                radius: Length(4.5),
            },
            19,
        ))
    );
    let (_, geometry, _) = saved_section_entity_geometry(&entity).expect("complete saved circle");
    assert!(is_full_circle_geometry(&geometry));

    let circle = crate::feature::FeatureCircleSegment {
        center_id: 11,
        radius_ref: 12,
        external_id: 13,
        offset: 20,
    };
    let definition = crate::feature::FeatureDefinition {
        id: 5,
        owner_feature_id: Some(6),
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(crate::feature::FeatureVariableTable {
            declared_count: 0,
            entity_ref: None,
            rows: Vec::new(),
            points: vec![crate::feature::FeatureSectionPoint {
                point_id: 11,
                u: None,
                v: None,
            }],
            offset: 30,
        }),
        segments: Some(crate::feature::FeatureSegmentTable {
            declared_count: 2,
            has_elided_prototype: false,
            entity_ref: None,
            rows: Vec::new(),
            circle_rows: vec![circle.clone()],
            point_rows: Vec::new(),
            centered_line_rows: Vec::new(),
            reference_line_rows: Vec::new(),
            bounded_curve_rows: Vec::new(),
            conic_rows: Vec::new(),
            opaque_rows: Vec::new(),
            offset: 31,
        }),
        trim_entities: None,
        trim_vertices: None,
        order_table: Some(crate::feature::FeatureOrderTable {
            declared_count: 1,
            has_prototype: false,
            entity_ref: None,
            rows: vec![crate::feature::FeatureOrderRow {
                external_id: 13,
                internal_id: 7,
                bitmask: 0,
                offset: 32,
            }],
            offset: 32,
        }),
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: Some(crate::feature::FeatureSavedSection {
            entities: vec![entity],
            offset: 18,
        }),
        offset: 0,
    };
    assert_eq!(
        saved_section_circle_values(&definition, &circle),
        Some(([2.0, -3.0], 4.5))
    );
    assert_eq!(
        resolved_section_points(&definition),
        BTreeMap::from([(11, [2.0, -3.0])])
    );
    assert_eq!(
        resolved_section_radii(&definition),
        BTreeMap::from([(12, 4.5)])
    );
    let mut conflicting_radius = definition.clone();
    let variables = conflicting_radius.variables.as_mut().expect("variables");
    variables.declared_count = 1;
    variables.rows.push(crate::feature::FeatureVariableRow {
        variable_type: 3,
        key: 12,
        value: Some(5.0),
        value_body: Vec::new(),
        guess: None,
        guess_body: Vec::new(),
        guess_dimension_driven: false,
        known: None,
        homogeneity: None,
        uvar_id: None,
        dimension_driven: false,
        offset: 33,
    });
    assert!(resolved_section_radii(&conflicting_radius).is_empty());
    let mut conflicting = definition;
    conflicting.variables.as_mut().expect("variables").points[0].u = Some(3.0);
    assert_eq!(
        resolved_section_coordinates(&conflicting),
        BTreeMap::from([(11, [Some(3.0), Some(-3.0)])])
    );
    conflicting
        .variables
        .as_mut()
        .expect("variables")
        .points
        .push(crate::feature::FeatureSectionPoint {
            point_id: 11,
            u: Some(4.0),
            v: None,
        });
    assert!(resolved_section_coordinates(&conflicting).is_empty());
}

#[test]
fn generated_saved_geometry_forms_closed_profiles() {
    let line = |external_id: u32, start: (f64, f64), end: (f64, f64)| {
        (
            external_id,
            SketchGeometry::Line {
                start: Point2::new(start.0, start.1),
                end: Point2::new(end.0, end.1),
            },
        )
    };
    let geometries = vec![
        line(12, (0.0, 1.0), (1.0, 1.0)),
        (
            10,
            SketchGeometry::Nurbs {
                degree: 1,
                knots: vec![0.0, 0.0, 1.0, 1.0],
                control_points: vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
                weights: None,
                periodic: false,
            },
        ),
        line(13, (0.0, 0.0), (0.0, 1.0)),
        line(11, (1.0, 1.0), (1.0, 0.0)),
        line(20, (5.0, 5.0), (6.0, 5.0)),
        (
            30,
            SketchGeometry::Arc {
                center: Point2::new(8.0, 8.0),
                radius: Length(2.0),
                start_angle: Angle(0.0),
                end_angle: Angle(std::f64::consts::TAU),
            },
        ),
    ];

    let profiles =
        saved_profile_chains(&SketchId("creo:model:sketch#917".to_string()), &geometries);

    assert_eq!(profiles.len(), 2);
    assert_eq!(
        profiles[0][0].entity.0,
        "creo:featdefs:sketch_entity#917:30"
    );
    assert_eq!(profiles[1].len(), 4);
    assert_eq!(
        profiles[1][0].entity.0,
        "creo:featdefs:sketch_entity#917:10"
    );
    assert!(!profiles[1][0].reversed);
    assert!(profiles[1][1..].iter().all(|entity| entity.reversed));
    assert!(profiles
        .iter()
        .flatten()
        .all(|entity| !entity.entity.0.ends_with(":20")));
}

#[test]
fn saved_arc_joins_through_order_table() {
    let segment = crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Arc,
        directions: [None; 3],
        point_ids: [7, 9],
        center_id: Some(8),
        arc_orientation: Some(0),
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 42,
        body: Vec::new(),
        offset: 40,
    };
    let definition = crate::feature::FeatureDefinition {
        id: 5,
        owner_feature_id: Some(6),
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: Some(crate::feature::FeatureOrderTable {
            declared_count: 1,
            has_prototype: false,
            entity_ref: None,
            rows: vec![crate::feature::FeatureOrderRow {
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
        saved_section: Some(crate::feature::FeatureSavedSection {
            entities: vec![crate::feature::FeatureSavedEntity::Arc(
                crate::feature::FeatureSavedArc {
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
        saved_section_arc_geometry(&definition, &segment),
        Some(SketchGeometry::Arc {
            center: cadmpeg_ir::math::Point2::new(0.0, 0.0),
            radius: Length(2.0),
            start_angle: Angle(std::f64::consts::PI),
            end_angle: Angle(3.0 * std::f64::consts::FRAC_PI_2),
        })
    );
    assert_eq!(
        saved_section_segment_point_coordinates(&definition, &segment),
        Some(vec![(7, [0.0, -2.0]), (9, [-2.0, 0.0]), (8, [0.0, 0.0]),])
    );
    let mut coordinate_definition = definition.clone();
    coordinate_definition.variables = Some(crate::feature::FeatureVariableTable {
        declared_count: 0,
        entity_ref: None,
        rows: Vec::new(),
        points: [7, 8, 9]
            .map(|point_id| crate::feature::FeatureSectionPoint {
                point_id,
                u: None,
                v: None,
            })
            .to_vec(),
        offset: 30,
    });
    coordinate_definition.segments = Some(crate::feature::FeatureSegmentTable {
        declared_count: 1,
        has_elided_prototype: false,
        entity_ref: None,
        rows: vec![segment.clone()],
        circle_rows: Vec::new(),
        point_rows: Vec::new(),
        centered_line_rows: Vec::new(),
        reference_line_rows: Vec::new(),
        bounded_curve_rows: Vec::new(),
        conic_rows: Vec::new(),
        opaque_rows: Vec::new(),
        offset: 38,
    });
    assert_eq!(
        resolved_section_points(&coordinate_definition),
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
        .push(crate::feature::FeatureOrderRow {
            external_id: 42,
            internal_id: 4,
            bitmask: 0,
            offset: 11,
        });
    assert_eq!(
        saved_section_arc_geometry(&duplicate_order_row, &segment),
        None
    );
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
    assert_eq!(
        saved_section_arc_geometry(&duplicate_saved_arc, &segment),
        None
    );

    let segment_table = crate::feature::FeatureSegmentTable {
        declared_count: 2,
        has_elided_prototype: true,
        entity_ref: None,
        rows: vec![segment.clone()],
        circle_rows: Vec::new(),
        point_rows: Vec::new(),
        centered_line_rows: Vec::new(),
        reference_line_rows: Vec::new(),
        bounded_curve_rows: Vec::new(),
        conic_rows: Vec::new(),
        opaque_rows: Vec::new(),
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
    if let crate::feature::FeatureSavedEntity::Arc(arc) = &mut prototype {
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
    assert!(saved_section_arc_geometry(&elided_prototype, &segment).is_some());
    assert_eq!(
        semantic_saved_section_entities(&elided_prototype).count(),
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
    if let crate::feature::FeatureSavedEntity::Arc(arc) = &mut complete_elided_prototype
        .saved_section
        .as_mut()
        .expect("saved section")
        .entities[0]
    {
        arc.offset = 18;
    }
    assert_eq!(
        semantic_saved_section_entities(&complete_elided_prototype).count(),
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
    if let crate::feature::FeatureSavedEntity::Arc(arc) = &mut unique_at_table_origin
        .saved_section
        .as_mut()
        .expect("saved section")
        .entities[0]
    {
        arc.offset = 18;
    }
    assert!(saved_section_arc_geometry(&unique_at_table_origin, &segment).is_some());

    let mut trimmed = definition;
    trimmed.segments = Some(crate::feature::FeatureSegmentTable {
        declared_count: 1,
        has_elided_prototype: false,
        entity_ref: None,
        rows: vec![segment],
        circle_rows: Vec::new(),
        point_rows: Vec::new(),
        centered_line_rows: Vec::new(),
        reference_line_rows: Vec::new(),
        bounded_curve_rows: Vec::new(),
        conic_rows: Vec::new(),
        opaque_rows: Vec::new(),
        offset: 38,
    });
    trimmed.trim_entities = Some(crate::feature::FeatureTrimEntityTable {
        declared_count: None,
        entity_ref: None,
        entry_ref: None,
        buckets: Vec::new(),
        rows: vec![crate::feature::FeatureTrimEntity {
            external_id: 42,
            mode: Some(0),
            vertices: [1, 2],
            center_vertex: None,
            kind: crate::feature::TrimEntityKind::Arc,
            offset: 30,
        }],
        solved_external_ids: vec![42],
        offset: 28,
    });
    assert_eq!(
        resolved_trim_vertex_coordinates(&trimmed, &BTreeMap::new()),
        BTreeMap::from([(1, [0.0, -2.0]), (2, [-2.0, 0.0])])
    );
    let mut conflicting_vertex = trimmed.clone();
    conflicting_vertex.trim_vertices = Some(crate::feature::FeatureTrimVertexTable {
        declared_count: None,
        entity_ref: None,
        entry_ref: None,
        buckets: Vec::new(),
        rows: vec![
            crate::feature::FeatureTrimVertex {
                vertex_id: 1,
                entities: vec![42, 43],
                section_coordinates: Some([0.0, -2.0]),
                offset: 31,
            },
            crate::feature::FeatureTrimVertex {
                vertex_id: 1,
                entities: vec![42, 44],
                section_coordinates: Some([9.0, 9.0]),
                offset: 32,
            },
        ],
        offset: 30,
    });
    assert_eq!(
        resolved_trim_vertex_coordinates(&conflicting_vertex, &BTreeMap::new()),
        BTreeMap::from([(2, [-2.0, 0.0])])
    );
    if let crate::feature::FeatureSavedEntity::Arc(arc) = &mut trimmed
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
        .rows[0];
    assert_eq!(
        saved_section_arc_carrier(&trimmed, segment),
        Some(([0.0, 0.0], 2.0))
    );
    if let crate::feature::FeatureSavedEntity::Arc(arc) = &mut trimmed
        .saved_section
        .as_mut()
        .expect("test definition has a saved section")
        .entities[0]
    {
        arc.center[1] = Some(0.0);
        arc.radius = Some(2.0);
    }
    if let crate::feature::FeatureSavedEntity::Arc(arc) = &mut trimmed
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
        resolved_trim_vertex_coordinates(&trimmed, &BTreeMap::new()),
        BTreeMap::from([(2, [-2.0, 0.0])])
    );
    if let crate::feature::FeatureSavedEntity::Arc(arc) = &mut trimmed
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
        .rows[0];
    assert!(saved_section_arc_geometry(&trimmed, segment).is_none());
    assert_eq!(
        section_segment_intersection_carrier(
            &trimmed,
            &resolved_section_radii(&trimmed),
            &BTreeMap::new(),
            segment,
        )
        .map(|carrier| carrier.geometry),
        Some(SketchGeometry::Arc {
            center: cadmpeg_ir::math::Point2::new(0.0, 0.0),
            radius: Length(2.0),
            start_angle: Angle(0.0),
            end_angle: Angle(std::f64::consts::TAU),
        })
    );
}

#[test]
fn trimmed_line_reconciles_carrier_and_solver_orientation() {
    let segment = crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Line,
        directions: [None; 3],
        point_ids: [7, 9],
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 42,
        body: Vec::new(),
        offset: 40,
    };
    let anchor = crate::feature::FeatureSegment {
        point_ids: [5, 6],
        external_id: 41,
        body: Vec::new(),
        offset: 39,
        ..segment.clone()
    };
    let horizontal = crate::feature::FeatureSkamp {
        id: 1,
        kind: 1,
        flags: 0,
        status: 1,
        items: vec![crate::feature::FeatureSkampItem {
            entity_id: 41,
            sense: 0,
        }],
        offset: 50,
    };
    let parallel = crate::feature::FeatureSkamp {
        id: 2,
        kind: 7,
        flags: 0,
        status: 1,
        items: vec![
            crate::feature::FeatureSkampItem {
                entity_id: 41,
                sense: 0,
            },
            crate::feature::FeatureSkampItem {
                entity_id: 42,
                sense: 0,
            },
        ],
        offset: 55,
    };
    let mut definition = crate::feature::FeatureDefinition {
        id: 5,
        owner_feature_id: Some(6),
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: Some(crate::feature::FeatureSegmentTable {
            declared_count: 2,
            has_elided_prototype: false,
            entity_ref: None,
            rows: vec![anchor, segment.clone()],
            circle_rows: Vec::new(),
            point_rows: Vec::new(),
            centered_line_rows: Vec::new(),
            reference_line_rows: Vec::new(),
            bounded_curve_rows: Vec::new(),
            conic_rows: Vec::new(),
            opaque_rows: Vec::new(),
            offset: 20,
        }),
        trim_entities: Some(crate::feature::FeatureTrimEntityTable {
            declared_count: None,
            entity_ref: None,
            entry_ref: None,
            buckets: Vec::new(),
            rows: vec![crate::feature::FeatureTrimEntity {
                external_id: 42,
                mode: Some(0),
                vertices: [1, 2],
                center_vertex: None,
                kind: crate::feature::TrimEntityKind::Line,
                offset: 30,
            }],
            solved_external_ids: vec![42],
            offset: 28,
        }),
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: Some(crate::feature::FeatureRelationTable {
            declared_count: 2,
            entity_ref: None,
            rows: Vec::new(),
            skamps: vec![horizontal, parallel],
            skamp_header: Some(crate::feature::FeatureSolverTableHeader {
                declared_count: 2,
                entity_ref: 70,
                offset: 45,
            }),
            triples: Vec::new(),
            triples_header: None,
            offset: 44,
        }),
        saved_section: None,
        offset: 0,
    };
    let trim_vertices = BTreeMap::from([(1, [-2.0, 3.0]), (2, [4.0, 3.0])]);

    assert_eq!(
        trimmed_section_segment_geometry(&definition, &BTreeMap::new(), &trim_vertices, &segment,),
        Some(SketchGeometry::Line {
            start: cadmpeg_ir::math::Point2::new(-2.0, 3.0),
            end: cadmpeg_ir::math::Point2::new(4.0, 3.0),
        })
    );
    let mut disabled_parallel = definition.clone();
    disabled_parallel
        .relations
        .as_mut()
        .expect("solver relations")
        .skamps[1]
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
    relations.skamps.push(crate::feature::FeatureSkamp {
        id: 3,
        kind: 2,
        flags: 0,
        status: 1,
        items: vec![crate::feature::FeatureSkampItem {
            entity_id: 41,
            sense: 0,
        }],
        offset: 60,
    });
    relations
        .skamp_header
        .as_mut()
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
    let segment = crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Arc,
        directions: [None; 3],
        point_ids: [7, 9],
        center_id: Some(8),
        arc_orientation: Some(0),
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 42,
        body: Vec::new(),
        offset: 40,
    };
    let definition = crate::feature::FeatureDefinition {
        id: 5,
        owner_feature_id: Some(6),
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: None,
        trim_entities: Some(crate::feature::FeatureTrimEntityTable {
            declared_count: None,
            entity_ref: None,
            entry_ref: None,
            buckets: Vec::new(),
            rows: vec![crate::feature::FeatureTrimEntity {
                external_id: 42,
                mode: Some(0),
                vertices: [1, 2],
                center_vertex: None,
                kind: crate::feature::TrimEntityKind::Arc,
                offset: 30,
            }],
            solved_external_ids: vec![42],
            offset: 28,
        }),
        trim_vertices: None,
        order_table: Some(crate::feature::FeatureOrderTable {
            declared_count: 1,
            has_prototype: false,
            entity_ref: None,
            rows: vec![crate::feature::FeatureOrderRow {
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
        saved_section: Some(crate::feature::FeatureSavedSection {
            entities: vec![crate::feature::FeatureSavedEntity::Arc(
                crate::feature::FeatureSavedArc {
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
        Some(SketchGeometry::Arc {
            center: cadmpeg_ir::math::Point2::new(0.0, 0.0),
            radius: Length(2.0),
            start_angle: Angle(-std::f64::consts::FRAC_PI_2),
            end_angle: Angle(std::f64::consts::PI),
        })
    );

    let mut var_segment = segment.clone();
    var_segment.radius_ref = Some(10);
    let mut var_arc = definition;
    var_arc.variables = Some(crate::feature::FeatureVariableTable {
        declared_count: 0,
        entity_ref: None,
        rows: Vec::new(),
        points: vec![
            crate::feature::FeatureSectionPoint {
                point_id: 7,
                u: Some(2.0),
                v: Some(0.0),
            },
            crate::feature::FeatureSectionPoint {
                point_id: 8,
                u: Some(0.0),
                v: Some(0.0),
            },
            crate::feature::FeatureSectionPoint {
                point_id: 9,
                u: Some(0.0),
                v: Some(2.0),
            },
        ],
        offset: 5,
    });
    var_arc.segments = Some(crate::feature::FeatureSegmentTable {
        declared_count: 1,
        has_elided_prototype: false,
        entity_ref: None,
        rows: vec![var_segment.clone()],
        circle_rows: Vec::new(),
        point_rows: Vec::new(),
        centered_line_rows: Vec::new(),
        reference_line_rows: Vec::new(),
        bounded_curve_rows: Vec::new(),
        conic_rows: Vec::new(),
        opaque_rows: Vec::new(),
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
        Some(SketchGeometry::Arc {
            center: cadmpeg_ir::math::Point2::new(0.0, 0.0),
            radius: Length(2.0),
            start_angle: Angle(-std::f64::consts::FRAC_PI_2),
            end_angle: Angle(std::f64::consts::PI),
        })
    );
}

#[test]
fn placed_extrusion_line_defines_plane() {
    let transform = crate::placement::FeatureSectionTransform {
        definition_id: 5,
        feature_id: Some(5),
        origin: [10.0, 20.0, 30.0],
        u_axis: [0.0, 1.0, 0.0],
        v_axis: [0.0, 0.0, 1.0],
        normal: [1.0, 0.0, 0.0],
        offset: 7,
    };
    let segment = crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Line,
        directions: [None; 3],
        point_ids: [1, 2],
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 3,
        body: Vec::new(),
        offset: 9,
    };
    let points = BTreeMap::from([(1, [2.0, 3.0]), (2, [6.0, 3.0])]);
    assert_eq!(
        extruded_segment_surface(&transform, &points, &segment),
        Some(SurfaceGeometry::Plane {
            origin: Point3::new(10.0, 22.0, 33.0),
            normal: Vector3::new(0.0, 0.0, -1.0),
            u_axis: Vector3::new(0.0, 1.0, 0.0),
        })
    );
    assert_eq!(
        placed_section_curve_geometry(&transform, &points, &segment),
        Some(CurveGeometry::Line {
            origin: Point3::new(10.0, 22.0, 33.0),
            direction: Vector3::new(0.0, 1.0, 0.0),
        })
    );
}
