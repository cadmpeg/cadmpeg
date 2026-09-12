// SPDX-License-Identifier: Apache-2.0
//! Tests: saved line.

use super::{
    declared_solver_rows, extruded_segment_surface, placed_section_curve_geometry,
    section_segment_intersection_carrier, section_skamp_constraints,
};
use crate::decode::sketch::{
    is_full_circle_geometry, resolved_section_coordinates, resolved_section_points,
    resolved_section_radii, resolved_section_segment_geometry, resolved_trim_vertex_coordinates,
    saved_profile_chains, saved_section_arc, saved_section_arc_carrier,
    saved_section_circle_values, saved_section_entity_geometry, saved_section_line_geometry,
    saved_section_missing_line_geometry, saved_section_segment_point_coordinates, trim_segment_id,
};
use crate::decode::sketch_transfer::constraints::{
    joined_relation_incidence_entities, relation_incidence_entities, section_dimension_constraints,
};
use crate::decode::sketch_transfer::identity::{
    ambiguous_section_segment_external_ids, materialized_saved_section_external_ids,
    saved_section_external_id, section_entity_external_ids, semantic_saved_section_entities,
    unique_saved_section_internal_ids, unresolved_saved_section_entity,
};
use crate::decode::sketch_transfer::loci::{
    section_skamp_incidence_locus, section_skamp_point_locus,
};
use crate::decode::sketch_transfer::profiles::{
    solver_only_section_entity_family, unique_section_incidence_curve_family,
    SectionEntityIncidenceFamily,
};
use crate::decode::sketch_transfer::skamp_constraints::section_skamp_constraints_for_geometry;
use crate::feature::definitions::ScalarLane;
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::{Angle, Length};
use cadmpeg_ir::sketches::{
    SketchConstraintDefinitionInput, SketchEntityId, SketchGeometry, SketchGeometryDefinition,
    SketchId,
};
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn saved_line_joins_through_order_table() {
    let segment = crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Line([7, 9]),
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
    let definition = crate::feature::FeatureDefinition {
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
        Some(
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: cadmpeg_ir::math::Point2::new(-8.0, -0.85),
                end: cadmpeg_ir::math::Point2::new(8.0, -0.85),
            })
            .expect("valid test fixture")
        )
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
    coordinate_definition.variables = Some(crate::feature::definitions::test_support::with_points(
        crate::feature::FeatureVariableTable {
            declared_count: 0,
            entity_ref: None,
            rows: Vec::new(),
            offset: 30,
        },
        vec![
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
    ));
    coordinate_definition.segments = Some(crate::feature::FeatureSegmentTable {
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
        resolved_section_points(&coordinate_definition),
        BTreeMap::from([(7, [-8.0, -0.85]), (9, [8.0, -0.85])])
    );
    coordinate_definition
        .variables
        .as_mut()
        .expect("variables")
        .rows[0]
        .value = ScalarLane::Value(7.0);
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
        &SketchId::mint("creo:model:sketch#5").expect("valid test fixture"),
        &incomplete
            .saved_section
            .as_ref()
            .expect("saved section")
            .entities[0],
        &unique_saved_section_internal_ids(&incomplete),
        &BTreeSet::new(),
    )
    .expect("valid test fixture");
    assert_eq!(offset, 20);
    assert_eq!(
        native_entity.id().as_str(),
        "creo:featdefs:sketch_entity#5:42"
    );
    assert!(matches!(*native_entity.geometry.definition(),
        SketchGeometryDefinition::Native { ref native_kind } if native_kind == "saved_line"
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
        rows: crate::feature::segment_rows::SegmentRows::default(),
        offset: 0,
    });
    constrained.dimensions = Some(crate::feature::FeatureDimensionTable {
        declared_count: 1,
        entity_ref: None,
        rows: vec![crate::feature::FeatureDimension {
            dimension_type: 1,
            value: crate::feature::definitions::DimensionValue::Resolved(2.0),
            value_body: Vec::new(),
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
        skamps: Some(crate::feature::definitions::SolverSubtable::Declared {
            header: crate::feature::definitions::FeatureSolverTableHeader {
                declared_count: 1,
                entity_ref: 1,
                offset: 29,
            },
            rows: vec![crate::feature::FeatureSkamp {
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
        }),
        triples: Some(crate::feature::definitions::SolverSubtable::Declared {
            header: crate::feature::definitions::FeatureSolverTableHeader {
                declared_count: 1,
                entity_ref: 2,
                offset: 31,
            },
            rows: vec![crate::feature::FeatureRelationTriple {
                relation_id: Some(7),
                equation_id: Some(11),
                skamp_id: Some(5),
                offset: 31,
            }],
        }),
        offset: 28,
    });
    let constraints = section_skamp_constraints(
        &constrained,
        &SketchId::mint("creo:model:sketch#5".to_string()).expect("valid test fixture"),
    );
    assert!(matches!(
        constraints[0].0.definition.kind(),
        SketchConstraintDefinitionInput::Native { entities, .. }
            if entities == &[SketchEntityId::mint(
                "creo:featdefs:sketch_entity#5:42".to_string()
            ).expect("valid test fixture")]
    ));
    let SketchConstraintDefinitionInput::Native { operands, .. } =
        constraints[0].0.definition.kind()
    else {
        unreachable!();
    };
    assert!(operands.iter().any(|operand| {
        operand.native_kind == "triples_ptr"
            && operand.field.as_ref().map(|field| field.name.as_str()) == Some("equation_id")
            && operand
                .field
                .as_ref()
                .and_then(|field| field.role)
                .is_none()
            && operand.object_index == Some(11)
    }));
    let mut equation_only_incidence = constrained.clone();
    equation_only_incidence
        .relations
        .as_mut()
        .expect("relations")
        .triples
        .as_mut()
        .expect("triples table")
        .rows_mut()[0]
        .relation_id = None;
    let equation_only_constraints = section_skamp_constraints(
        &equation_only_incidence,
        &SketchId::mint("creo:model:sketch#5".to_string()).expect("valid test fixture"),
    );
    let SketchConstraintDefinitionInput::Native { operands, .. } =
        equation_only_constraints[0].0.definition.kind()
    else {
        unreachable!();
    };
    assert!(operands.iter().any(|operand| {
        operand.field.as_ref().map(|field| field.name.as_str()) == Some("equation_id")
            && operand.object_index == Some(11)
    }));
    let mut missing_equation = equation_only_incidence.clone();
    missing_equation
        .relations
        .as_mut()
        .expect("relations")
        .triples
        .as_mut()
        .expect("triples table")
        .rows_mut()[0]
        .equation_id = None;
    let missing_equation_constraints = section_skamp_constraints(
        &missing_equation,
        &SketchId::mint("creo:model:sketch#5".to_string()).expect("valid test fixture"),
    );
    let SketchConstraintDefinitionInput::Native { operands, .. } =
        missing_equation_constraints[0].0.definition.kind()
    else {
        unreachable!();
    };
    assert!(!operands.iter().any(|operand| {
        operand.field.as_ref().map(|field| field.name.as_str()) == Some("equation_id")
    }));
    let mut duplicate_equation = equation_only_incidence.clone();
    let duplicate_equation_relations = duplicate_equation.relations.as_mut().expect("relations");
    declared_solver_rows(&mut duplicate_equation_relations.triples).push(
        crate::feature::FeatureRelationTriple {
            relation_id: None,
            equation_id: Some(11),
            skamp_id: Some(5),
            offset: 32,
        },
    );
    duplicate_equation_relations
        .triples
        .as_mut()
        .expect("triples table")
        .header_mut()
        .expect("triples header")
        .declared_count = 2;
    let duplicate_equation_constraints = section_skamp_constraints(
        &duplicate_equation,
        &SketchId::mint("creo:model:sketch#5".to_string()).expect("valid test fixture"),
    );
    let SketchConstraintDefinitionInput::Native { operands, .. } =
        duplicate_equation_constraints[0].0.definition.kind()
    else {
        unreachable!();
    };
    assert!(!operands.iter().any(|operand| {
        operand.field.as_ref().map(|field| field.name.as_str()) == Some("equation_id")
    }));
    assert_eq!(
        relation_incidence_entities(
            &constrained,
            &SketchId::mint("creo:model:sketch#5".to_string()).expect("valid test fixture"),
            7,
        ),
        vec![
            SketchEntityId::mint("creo:featdefs:sketch_entity#5:42".to_string())
                .expect("valid test fixture"),
            SketchEntityId::mint("creo:featdefs:sketch_entity#5:99".to_string())
                .expect("valid test fixture"),
        ]
    );
    let dimension_constraints = section_dimension_constraints(
        &constrained,
        &SketchId::mint("creo:model:sketch#5".to_string()).expect("valid test fixture"),
    );
    assert!(
        matches!(
            dimension_constraints[0].0.definition.kind(),
            SketchConstraintDefinitionInput::Distance { entities, .. }
                if entities == &[
                    SketchEntityId::mint("creo:featdefs:sketch_entity#5:42".to_string()).expect("valid test fixture"),
                    SketchEntityId::mint("creo:featdefs:sketch_entity#5:99".to_string()).expect("valid test fixture"),
                ]
        ),
        "{:?}",
        dimension_constraints[0].0.definition
    );
    let mut native_join = constrained.clone();
    native_join.relations.as_mut().expect("relations").rows[0].relation_type = 99;
    let native_join_constraints = section_dimension_constraints(
        &native_join,
        &SketchId::mint("creo:model:sketch#5".to_string()).expect("valid test fixture"),
    );
    let SketchConstraintDefinitionInput::Native { operands, .. } =
        native_join_constraints[0].0.definition.kind()
    else {
        panic!("untyped relation must remain native");
    };
    assert!(operands.iter().any(|operand| {
        operand.native_kind == "skamp_ptr"
            && operand.field.as_ref().map(|field| field.name.as_str())
                == Some("triples_ptr.skamp_id")
            && operand.object_index == Some(5)
    }));
    assert!(operands.iter().any(|operand| {
        operand.native_kind == "triples_ptr"
            && operand.field.as_ref().map(|field| field.name.as_str()) == Some("equation_id")
            && operand.object_index == Some(11)
    }));
    declared_solver_rows(&mut native_join.relations.as_mut().expect("relations").triples).push(
        crate::feature::FeatureRelationTriple {
            relation_id: Some(7),
            equation_id: None,
            skamp_id: Some(5),
            offset: 32,
        },
    );
    native_join
        .relations
        .as_mut()
        .expect("relations")
        .triples
        .as_mut()
        .expect("triples table")
        .header_mut()
        .expect("triples header")
        .declared_count = 2;
    let ambiguous_join_constraints = section_dimension_constraints(
        &native_join,
        &SketchId::mint("creo:model:sketch#5".to_string()).expect("valid test fixture"),
    );
    let SketchConstraintDefinitionInput::Native { operands, .. } =
        ambiguous_join_constraints[0].0.definition.kind()
    else {
        panic!("untyped relation must remain native");
    };
    assert!(!operands.iter().any(|operand| {
        operand.field.as_ref().map(|field| field.name.as_str()) == Some("triples_ptr.skamp_id")
    }));
    assert!(!operands.iter().any(|operand| {
        operand.field.as_ref().map(|field| field.name.as_str()) == Some("equation_id")
    }));
    let mut solver_families = constrained.clone();
    let family_relations = solver_families.relations.as_mut().expect("relations");
    *declared_solver_rows(&mut family_relations.skamps) = vec![crate::feature::FeatureSkamp {
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
        .skamps
        .as_mut()
        .expect("skamp table")
        .rows_mut()[0]
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
        .skamps
        .as_mut()
        .expect("skamp table")
        .rows_mut()[0]
        .status = 0;
    assert_eq!(
        solver_only_section_entity_family(&solver_families, 99),
        Some(SectionEntityIncidenceFamily::Point)
    );
    solver_families
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .as_mut()
        .expect("skamp table")
        .rows_mut()[0]
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
        .skamps
        .as_mut()
        .expect("skamp table")
        .rows_mut()[0]
        .items[0]
        .sense = 2;
    solver_families
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .as_mut()
        .expect("skamp table")
        .rows_mut()[0]
        .status = 1;
    assert_eq!(
        solver_only_section_entity_family(&solver_families, 99),
        Some(SectionEntityIncidenceFamily::BoundedCurve)
    );
    let family_relations = solver_families.relations.as_mut().expect("relations");
    *declared_solver_rows(&mut family_relations.skamps) = vec![crate::feature::FeatureSkamp {
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
        SketchEntityId::mint("creo:featdefs:sketch_entity#5:99".to_string())
            .expect("valid test fixture"),
        SketchGeometry::native(
            cadmpeg_ir::products::NonEmptyString::new("solver_only_section_entity")
                .expect("nonempty source identity"),
        ),
    )]);
    assert!(matches!(
        section_skamp_constraints_for_geometry(
            &solver_families,
            &SketchId::mint("creo:model:sketch#5".to_string()).expect("valid test fixture"),
            Some(&solver_geometry),
        )[0]
        .0
        .definition
        .kind(),
        SketchConstraintDefinitionInput::Horizontal { .. }
    ));
    let unary = &mut solver_families
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .as_mut()
        .expect("skamp table")
        .rows_mut()[0];
    unary.kind = 2;
    unary.status = 1;
    assert!(matches!(
        section_skamp_constraints_for_geometry(
            &solver_families,
            &SketchId::mint("creo:model:sketch#5".to_string()).expect("valid test fixture"),
            Some(&solver_geometry),
        )[0]
        .0
        .definition
        .kind(),
        SketchConstraintDefinitionInput::Vertical { .. }
    ));
    let family_relations = solver_families.relations.as_mut().expect("relations");
    *declared_solver_rows(&mut family_relations.skamps) = vec![crate::feature::FeatureSkamp {
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
        .skamps
        .as_mut()
        .expect("skamp table")
        .rows_mut()[0]
        .status = 1;
    assert_eq!(
        solver_only_section_entity_family(&solver_families, 99),
        Some(SectionEntityIncidenceFamily::Point)
    );
    let solver_geometry = BTreeMap::from([
        (
            SketchEntityId::mint("creo:featdefs:sketch_entity#5:42".to_string())
                .expect("valid test fixture"),
            SketchGeometry::native(
                cadmpeg_ir::products::NonEmptyString::new("line")
                    .expect("nonempty source identity"),
            ),
        ),
        (
            SketchEntityId::mint("creo:featdefs:sketch_entity#5:99".to_string())
                .expect("valid test fixture"),
            SketchGeometry::native(
                cadmpeg_ir::products::NonEmptyString::new("point")
                    .expect("nonempty source identity"),
            ),
        ),
    ]);
    let solver_constraints = section_skamp_constraints_for_geometry(
        &solver_families,
        &SketchId::mint("creo:model:sketch#5".to_string()).expect("valid test fixture"),
        Some(&solver_geometry),
    );
    let point_item = &solver_families
        .relations
        .as_ref()
        .expect("relations")
        .skamps()[0]
        .items[0];
    let line_item = &solver_families
        .relations
        .as_ref()
        .expect("relations")
        .skamps()[0]
        .items[1];
    assert!(section_skamp_point_locus(
        &solver_families,
        &SketchId::mint("creo:model:sketch#5".to_string()).expect("valid test fixture"),
        point_item
    )
    .is_some());
    assert!(section_skamp_incidence_locus(
        &solver_families,
        &SketchId::mint("creo:model:sketch#5".to_string()).expect("valid test fixture"),
        line_item,
        Some(&solver_geometry)
    )
    .is_some());
    assert!(
        matches!(
            solver_constraints[0].0.definition.kind(),
            SketchConstraintDefinitionInput::CoincidentLoci { .. }
        ),
        "{:?}",
        solver_constraints[0].0.definition
    );
    let family_relations = solver_families.relations.as_mut().expect("relations");
    *declared_solver_rows(&mut family_relations.skamps) = vec![crate::feature::FeatureSkamp {
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
        .skamps
        .as_mut()
        .expect("skamp table")
        .header_mut()
        .expect("skamp header")
        .declared_count = 1;
    assert_eq!(
        solver_only_section_entity_family(&solver_families, 99),
        Some(SectionEntityIncidenceFamily::Circular)
    );
    let mut disabled_line_family = solver_families.clone();
    let disabled_line_relations = disabled_line_family.relations.as_mut().expect("relations");
    disabled_line_relations
        .skamps
        .as_mut()
        .expect("skamp table")
        .rows_mut()[0] = crate::feature::FeatureSkamp {
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
        .rows
        .insert(crate::feature::segment_rows::SegmentRow::Opaque(
            crate::feature::FeatureOpaqueSegment {
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
            },
        ));
    let disabled_circular_relations = disabled_circular_family
        .relations
        .as_mut()
        .expect("relations");
    *declared_solver_rows(&mut disabled_circular_relations.skamps) =
        vec![crate::feature::FeatureSkamp {
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
        .skamps
        .as_mut()
        .expect("skamp table")
        .header_mut()
        .expect("skamp header")
        .declared_count = 1;
    assert_eq!(
        unique_section_incidence_curve_family(&disabled_circular_family, 101),
        Some(SectionEntityIncidenceFamily::Circular)
    );
    let family_relations = solver_families.relations.as_mut().expect("relations");
    declared_solver_rows(&mut family_relations.skamps).push(crate::feature::FeatureSkamp {
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
        .skamps
        .as_mut()
        .expect("skamp table")
        .header_mut()
        .expect("skamp header")
        .declared_count = 2;
    assert_eq!(
        solver_only_section_entity_family(&solver_families, 99),
        None
    );
    let mut duplicate_incidence = constrained.clone();
    let duplicate_relations = duplicate_incidence.relations.as_mut().expect("relations");
    let mut duplicate = duplicate_relations.skamps()[0].clone();
    duplicate.status = 34;
    duplicate.offset = 32;
    declared_solver_rows(&mut duplicate_relations.skamps).push(duplicate);
    duplicate_relations
        .skamps
        .as_mut()
        .expect("skamp table")
        .header_mut()
        .expect("skamp header")
        .declared_count = 2;
    assert!(relation_incidence_entities(
        &duplicate_incidence,
        &SketchId::mint("creo:model:sketch#5".to_string()).expect("valid test fixture"),
        7,
    )
    .is_empty());
    constrained
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .as_mut()
        .expect("skamp table")
        .rows_mut()[0]
        .status = 34;
    assert!(relation_incidence_entities(
        &constrained,
        &SketchId::mint("creo:model:sketch#5".to_string()).expect("valid test fixture"),
        7,
    )
    .is_empty());
    assert_eq!(
        joined_relation_incidence_entities(
            &constrained,
            &SketchId::mint("creo:model:sketch#5".to_string()).expect("valid test fixture"),
            7,
        ),
        vec![
            SketchEntityId::mint("creo:featdefs:sketch_entity#5:42".to_string())
                .expect("valid test fixture"),
            SketchEntityId::mint("creo:featdefs:sketch_entity#5:99".to_string())
                .expect("valid test fixture"),
        ]
    );
    assert_eq!(
        section_skamp_constraints(
            &constrained,
            &SketchId::mint("creo:model:sketch#5".to_string()).expect("valid test fixture")
        )[0]
        .0
        .active,
        Some(false)
    );
    constrained.segments = None;
    let constraints = section_skamp_constraints(
        &constrained,
        &SketchId::mint("creo:model:sketch#5".to_string()).expect("valid test fixture"),
    );
    assert!(matches!(
        constraints[0].0.definition.kind(),
        SketchConstraintDefinitionInput::Native { entities, .. }
            if entities == &[SketchEntityId::mint(
                "creo:featdefs:sketch_entity#5:42".to_string()
            ).expect("valid test fixture")]
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
        rows: (vec![segment.clone()])
            .into_iter()
            .map(crate::feature::segment_rows::SegmentRow::Ordinary)
            .collect(),
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
            kind: crate::feature::TrimEntityKind::Line,
            offset: 6,
        }],
        solved_external_ids: vec![42],
        offset: 5,
    });
    assert_eq!(
        saved_section_line_geometry(&completed, &segment),
        Some(
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: cadmpeg_ir::math::Point2::new(-8.0, -0.85),
                end: cadmpeg_ir::math::Point2::new(8.0, -0.85),
            })
            .expect("valid test fixture")
        )
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
        Some(
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: cadmpeg_ir::math::Point2::new(-8.0, -0.85),
                end: cadmpeg_ir::math::Point2::new(8.0, -0.85),
            })
            .expect("valid test fixture")
        )
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
    omitted_segment.kind = crate::feature::FeatureSegmentKind::Line([11, 12]);
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
        .insert(crate::feature::segment_rows::SegmentRow::Ordinary(
            omitted_segment.clone(),
        ));
    missing_line
        .trim_entities
        .as_mut()
        .expect("trim table")
        .rows
        .push(crate::feature::FeatureTrimEntity {
            external_id: 43,
            mode: Some(0),
            vertices: [3, 4],
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
    missing_line
        .segments
        .as_mut()
        .expect("segment table")
        .rows
        .edit_ordinary(|rows| rows[1] = omitted_segment.clone());
    assert_eq!(
        saved_section_missing_line_geometry(&missing_line),
        Some((
            omitted_segment.offset,
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: cadmpeg_ir::math::Point2::new(-8.0, -0.85),
                end: cadmpeg_ir::math::Point2::new(8.0, -0.85),
            })
            .expect("valid test fixture"),
        ))
    );
    assert_eq!(
        resolved_section_segment_geometry(&missing_line, &BTreeMap::new(), &omitted_segment),
        Some(
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: cadmpeg_ir::math::Point2::new(-8.0, -0.85),
                end: cadmpeg_ir::math::Point2::new(8.0, -0.85),
            })
            .expect("valid test fixture")
        )
    );

    omitted_segment.vertical_horizontal = Some(0);
    missing_line
        .segments
        .as_mut()
        .expect("segment table")
        .rows
        .edit_ordinary(|rows| rows[1] = omitted_segment);
    assert!(saved_section_missing_line_geometry(&missing_line).is_none());
    assert!(resolved_section_segment_geometry(
        &missing_line,
        &BTreeMap::new(),
        &missing_line
            .segments
            .as_ref()
            .expect("segment table")
            .rows
            .ordinary()
            .cloned()
            .collect::<Vec<_>>()[1],
    )
    .is_none());

    let mut duplicate_segment = completed.clone();
    duplicate_segment
        .segments
        .as_mut()
        .expect("segment table")
        .rows
        .insert(crate::feature::segment_rows::SegmentRow::Ordinary(segment));
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
            SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                center: Point2::new(2.0, -3.0),
                radius: Length::new(4.5).expect("finite length fixture"),
            })
            .expect("valid test fixture"),
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
        identity: crate::feature::definitions::DefinitionIdentity::Parsed {
            schema_id: std::num::NonZeroU32::new(5),
            owner_feature_id: Some(6),
        },
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(crate::feature::definitions::test_support::with_points(
            crate::feature::FeatureVariableTable {
                declared_count: 0,
                entity_ref: None,
                rows: Vec::new(),
                offset: 30,
            },
            vec![crate::feature::FeatureSectionPoint {
                point_id: 11,
                u: None,
                v: None,
            }],
        )),
        segments: Some(crate::feature::FeatureSegmentTable {
            declared_count: 2,
            has_elided_prototype: false,
            entity_ref: None,
            rows: (vec![circle.clone()])
                .into_iter()
                .map(crate::feature::segment_rows::SegmentRow::Circle)
                .collect(),
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
    variables.declared_count += 1;
    variables.rows.push(crate::feature::FeatureVariableRow {
        variable_type: crate::feature::definitions::VariableType::Radius,
        key: 12,
        value: crate::feature::definitions::ScalarLane::Value(5.0),
        value_body: Vec::new(),
        guess: crate::feature::definitions::ScalarLane::Undefined,
        guess_body: Vec::new(),

        known: None,
        homogeneity: None,
        uvar_id: None,

        offset: 33,
    });
    assert!(resolved_section_radii(&conflicting_radius).is_empty());
    let mut conflicting = definition;
    conflicting.variables.as_mut().expect("variables").rows[0].value = ScalarLane::Value(3.0);
    assert_eq!(
        resolved_section_coordinates(&conflicting),
        BTreeMap::from([(11, [Some(3.0), Some(-3.0)])])
    );
    let variables = conflicting.variables.as_mut().expect("variables");
    let mut duplicate = variables.rows[0].clone();
    duplicate.value = ScalarLane::Value(4.0);
    variables.rows.push(duplicate);
    variables.declared_count += 1;
    assert!(resolved_section_coordinates(&conflicting).is_empty());
}

#[test]
fn generated_saved_geometry_forms_closed_profiles() {
    let line = |external_id: u32, start: (f64, f64), end: (f64, f64)| {
        (
            external_id,
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(start.0, start.1),
                end: Point2::new(end.0, end.1),
            })
            .expect("valid test fixture"),
        )
    };
    let geometries = vec![
        line(12, (0.0, 1.0), (1.0, 1.0)),
        (
            10,
            SketchGeometry::try_from(SketchGeometryDefinition::Nurbs {
                curve: cadmpeg_ir::geometry::PcurveNurbs::from_lanes(
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
                    None,
                    false,
                )
                .expect("valid test pcurve"),
            })
            .expect("valid test fixture"),
        ),
        line(13, (0.0, 0.0), (0.0, 1.0)),
        line(11, (1.0, 1.0), (1.0, 0.0)),
        line(20, (5.0, 5.0), (6.0, 5.0)),
        (
            30,
            SketchGeometry::try_from(SketchGeometryDefinition::Arc {
                center: Point2::new(8.0, 8.0),
                radius: Length::new(2.0).expect("finite length fixture"),
                start_angle: Angle::new(0.0).expect("finite angle fixture"),
                end_angle: Angle::new(std::f64::consts::TAU).expect("finite angle fixture"),
            })
            .expect("valid test fixture"),
        ),
    ];

    let profiles = saved_profile_chains(
        &SketchId::mint("creo:model:sketch#917".to_string()).expect("valid test fixture"),
        &geometries,
    );

    assert_eq!(profiles.len(), 2);
    assert_eq!(
        profiles[0][0].entity.as_str(),
        "creo:featdefs:sketch_entity#917:30"
    );
    assert_eq!(profiles[1].len(), 4);
    assert_eq!(
        profiles[1][0].entity.as_str(),
        "creo:featdefs:sketch_entity#917:10"
    );
    assert!(!profiles[1][0].reversed);
    assert!(profiles[1][1..].iter().all(|entity| entity.reversed));
    assert!(profiles
        .iter()
        .flatten()
        .all(|entity| !entity.entity.as_str().ends_with(":20")));
}

#[test]
fn saved_arc_joins_through_order_table() {
    let segment = crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Arc([7, 9]),
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
    let definition = crate::feature::FeatureDefinition {
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
        saved_section_arc(&definition, &segment),
        Some(crate::decode::sketch::SavedSectionArc {
            center: cadmpeg_ir::units::FinitePoint2::new(cadmpeg_ir::math::Point2::new(0.0, 0.0))
                .expect("finite center fixture"),
            radius: cadmpeg_ir::scalar::PositiveLength::new(2.0).expect("positive length fixture"),
            start_angle: Angle::new(std::f64::consts::PI).expect("finite angle fixture"),
            end_angle: Angle::new(3.0 * std::f64::consts::FRAC_PI_2).expect("finite angle fixture"),
        })
    );
    assert_eq!(
        saved_section_segment_point_coordinates(&definition, &segment),
        Some(vec![(7, [0.0, -2.0]), (9, [-2.0, 0.0]), (8, [0.0, 0.0]),])
    );
    let mut coordinate_definition = definition.clone();
    coordinate_definition.variables = Some(crate::feature::definitions::test_support::with_points(
        crate::feature::FeatureVariableTable {
            declared_count: 0,
            entity_ref: None,
            rows: Vec::new(),
            offset: 30,
        },
        [7, 8, 9]
            .map(|point_id| crate::feature::FeatureSectionPoint {
                point_id,
                u: None,
                v: None,
            })
            .to_vec(),
    ));
    coordinate_definition.segments = Some(crate::feature::FeatureSegmentTable {
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
    assert_eq!(saved_section_arc(&duplicate_order_row, &segment), None);
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
    assert_eq!(saved_section_arc(&duplicate_saved_arc, &segment), None);

    let segment_table = crate::feature::FeatureSegmentTable {
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
    assert!(saved_section_arc(&elided_prototype, &segment).is_some());
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
    assert!(saved_section_arc(&unique_at_table_origin, &segment).is_some());

    let mut trimmed = definition;
    trimmed.segments = Some(crate::feature::FeatureSegmentTable {
        declared_count: 1,
        has_elided_prototype: false,
        entity_ref: None,
        rows: (vec![segment])
            .into_iter()
            .map(crate::feature::segment_rows::SegmentRow::Ordinary)
            .collect(),
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
            kind: crate::feature::TrimEntityKind::Arc { center_vertex: 3 },
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
        .rows
        .ordinary()
        .cloned()
        .collect::<Vec<_>>()[0];
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
        .rows
        .ordinary()
        .cloned()
        .collect::<Vec<_>>()[0];
    assert!(saved_section_arc(&trimmed, segment).is_none());
    assert_eq!(
        section_segment_intersection_carrier(
            &trimmed,
            &resolved_section_radii(&trimmed),
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

#[test]
fn placed_extrusion_line_defines_plane() {
    let transform = crate::placement::FeatureSectionTransform::new(
        5,
        Some(5),
        [10.0, 20.0, 30.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        7,
    )
    .expect("valid section frame");
    let segment = crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Line([1, 2]),
        directions: [None; 3],
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
        Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::PlaneSurface::try_new(
                Point3::new(10.0, 22.0, 33.0),
                Vector3::new(0.0, 0.0, -1.0),
                Vector3::new(0.0, 1.0, 0.0)
            )
            .expect("valid PlaneSurface fixture")
        )))
    );
    assert_eq!(
        placed_section_curve_geometry(&transform, &points, &segment),
        Some(CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::LineCurve::try_new(
                Point3::new(10.0, 22.0, 33.0),
                Vector3::new(0.0, 1.0, 0.0)
            )
            .expect("valid LineCurve fixture")
        )))
    );
}
