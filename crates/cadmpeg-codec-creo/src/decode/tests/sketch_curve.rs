// SPDX-License-Identifier: Apache-2.0
//! Tests: sketch curve.

use super::{extruded_segment_surface, placed_section_curve_geometry};
use crate::decode::feature_history::{
    evaluated_sweep_body_kind, evaluated_sweep_output_bodies, feature_dimension_display,
    feature_dimension_parameter_id, feature_dimension_parameter_layout,
    feature_dimension_parameter_row_id, resolved_feature_dimension_parameter,
};
use crate::decode::sketch::{resolved_section_radii, section_circle_geometry};
use crate::decode::sketch_transfer::{
    section_segment_radius_constraints, section_segment_radius_constraints_for_emitted,
    section_segment_verhor_definition, section_skamp_active,
};
use crate::decode::sweep::{placed_section_geometry_curve, placed_sketch_curve_ref};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{DimensionDisplay, Length, ParameterId};
use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{SketchConstraintDefinition, SketchEntityId, SketchGeometry, SketchId};
use cadmpeg_ir::topology::{Body, BodyKind};
use cadmpeg_ir::units::Units;
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn sketch_curve_references_require_a_materialized_curve() {
    let transform = crate::placement::FeatureSectionTransform {
        definition_id: 5,
        feature_id: Some(5),
        origin: [10.0, 20.0, 30.0],
        u_axis: [0.0, 1.0, 0.0],
        v_axis: [0.0, 0.0, 1.0],
        normal: [1.0, 0.0, 0.0],
        offset: 7,
    };
    let sketch = SketchId("creo:model:sketch#5".to_string());
    let line = SketchGeometry::Line {
        start: Point2::new(0.0, 0.0),
        end: Point2::new(2.0, 0.0),
    };
    let point = SketchGeometry::Point {
        position: Point2::new(1.0, 2.0),
    };

    assert_eq!(
        placed_sketch_curve_ref(Some(&transform), &sketch, 3, &line),
        Some("creo:featdefs:section_curve#5:3".to_string())
    );
    assert_eq!(placed_sketch_curve_ref(None, &sketch, 3, &line), None);
    assert_eq!(
        placed_sketch_curve_ref(Some(&transform), &sketch, 4, &point),
        None
    );
}

#[test]
fn placed_extrusion_arc_defines_cylinder() {
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
        kind: crate::feature::FeatureSegmentKind::Arc,
        directions: [None; 3],
        point_ids: [1, 2],
        center_id: Some(3),
        arc_orientation: Some(0),
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 4,
        body: Vec::new(),
        offset: 9,
    };
    let points = BTreeMap::from([(1, [2.0, 0.0]), (2, [-2.0, 0.0]), (3, [0.0, 0.0])]);
    assert_eq!(
        extruded_segment_surface(&transform, &points, &segment),
        Some(SurfaceGeometry::Cylinder {
            origin: Point3::new(10.0, 20.0, 30.0),
            axis: Vector3::new(1.0, 0.0, 0.0),
            ref_direction: Vector3::new(0.0, 1.0, 0.0),
            radius: 2.0,
        })
    );
    assert_eq!(
        placed_section_curve_geometry(&transform, &points, &segment),
        Some(CurveGeometry::Circle {
            center: Point3::new(10.0, 20.0, 30.0),
            axis: Vector3::new(1.0, 0.0, 0.0),
            ref_direction: Vector3::new(0.0, 1.0, 0.0),
            radius: 2.0,
        })
    );
    assert_eq!(
        placed_section_geometry_curve(
            &transform,
            &SketchGeometry::Circle {
                center: Point2::new(3.0, -4.0),
                radius: Length(2.0),
            },
        ),
        Some(CurveGeometry::Circle {
            center: Point3::new(10.0, 23.0, 26.0),
            axis: Vector3::new(1.0, 0.0, 0.0),
            ref_direction: Vector3::new(0.0, 1.0, 0.0),
            radius: 2.0,
        })
    );
}

#[test]
fn segment_verhor_projection_is_closed_and_lossless() {
    let mut segment = crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Line,
        directions: [None; 3],
        point_ids: [7, 9],
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: Some(0),
        radius_ref: None,
        radius2_ref: None,
        external_id: 12,
        body: Vec::new(),
        offset: 40,
    };
    let entity = SketchEntityId("entity".into());
    let sketch = SketchId("sketch".into());
    assert_eq!(
        section_segment_verhor_definition(&segment, &sketch, entity.clone()),
        Some(SketchConstraintDefinition::Vertical {
            entity: entity.clone()
        })
    );
    segment.vertical_horizontal = Some(1);
    assert_eq!(
        section_segment_verhor_definition(&segment, &sketch, entity.clone()),
        Some(SketchConstraintDefinition::Horizontal {
            entity: entity.clone()
        })
    );
    segment.vertical_horizontal = Some(2);
    let Some(SketchConstraintDefinition::Native {
        native_properties,
        entities,
        operands,
        ..
    }) = section_segment_verhor_definition(&segment, &sketch, entity.clone())
    else {
        panic!("an undefined line selector must remain native");
    };
    assert_eq!(native_properties["verhor"], "2");
    assert_eq!(entities, std::slice::from_ref(&entity));
    assert_eq!(operands[0].native_kind, "segtab_ptr");
    assert_eq!(operands[0].native_field.as_deref(), Some("ext_id"));
    assert_eq!(operands[0].object_index, 12);
    segment.kind = crate::feature::FeatureSegmentKind::Arc;
    segment.vertical_horizontal = Some(0);
    assert!(matches!(
        section_segment_verhor_definition(&segment, &sketch, entity),
        Some(SketchConstraintDefinition::Native { .. })
    ));
    segment.vertical_horizontal = None;
    assert_eq!(
        section_segment_verhor_definition(&segment, &sketch, SketchEntityId("entity".into())),
        None
    );
}

#[test]
fn skamp_status_low_bit_controls_constraint_activity() {
    assert!(!section_skamp_active(2));
    assert!(section_skamp_active(3));
    assert!(!section_skamp_active(34));
    assert!(section_skamp_active(35));
    assert!(!section_skamp_active(50));
    assert!(!section_skamp_active(65_570));
}

#[test]
fn dimension_identity_includes_its_feature_definition() {
    let sketch_917 = SketchId("creo:model:sketch#917".to_string());
    let sketch_1104 = SketchId("creo:model:sketch#1104".to_string());
    let sketch_1200 = SketchId("creo:model:sketch#1200".to_string());
    assert_ne!(
        feature_dimension_parameter_id(&sketch_917, 3),
        feature_dimension_parameter_id(&sketch_1104, 3)
    );
    assert_eq!(
        feature_dimension_parameter_id(&sketch_917, 3).0,
        "creo:featdefs:parameter#917:3"
    );
    assert_eq!(
        feature_dimension_parameter_layout(&[
            (sketch_917.clone(), 3),
            (sketch_1104.clone(), 3),
            (sketch_1104.clone(), 4),
            (sketch_1200, 3),
        ]),
        Some(vec![
            (0, "d3".to_string(), None),
            (0, "d3".to_string(), None),
            (1, "d4".to_string(), None),
            (0, "d3".to_string(), None),
        ])
    );
    assert_eq!(
        feature_dimension_parameter_layout(&[(sketch_917.clone(), 3), (sketch_917.clone(), 3),]),
        Some(vec![
            (0, "d917_3_1".to_string(), Some(0)),
            (1, "d917_3_2".to_string(), Some(1)),
        ])
    );
    assert_ne!(
        feature_dimension_parameter_row_id(&sketch_917, 3, Some(0)),
        feature_dimension_parameter_row_id(&sketch_917, 3, Some(1))
    );
    let dimension = crate::feature::FeatureDimension {
        dimension_type: 2,
        value: Some(5.0),
        value_body: Vec::new(),
        unresolved_value_token: None,
        value_unit: crate::feature::DimensionUnit::Millimeters,
        direction_byte: 0,
        auxiliary_value: None,
        auxiliary_body: Vec::new(),
        external_id: 3,
        references: None,
        offset: 10,
    };
    let mut table = crate::feature::FeatureDimensionTable {
        declared_count: 1,
        entity_ref: None,
        rows: vec![dimension.clone()],
        offset: 9,
    };
    let mut definition = crate::feature::FeatureDefinition {
        id: 917,
        owner_feature_id: Some(40),
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: Some(table.clone()),
        relations: None,
        saved_section: None,
        offset: 8,
    };
    assert_eq!(
        resolved_feature_dimension_parameter(
            &sketch_917,
            definition.dimensions.as_ref().expect("dimension table"),
            0,
        ),
        Some((
            &dimension,
            ParameterId("creo:featdefs:parameter#917:3".to_string())
        ))
    );
    definition.segments = Some(crate::feature::FeatureSegmentTable {
        declared_count: 1,
        has_elided_prototype: false,
        entity_ref: None,
        rows: Vec::new(),
        circle_rows: vec![crate::feature::FeatureCircleSegment {
            center_id: 7,
            radius_ref: 0,
            external_id: 42,
            offset: 20,
        }],
        point_rows: Vec::new(),
        centered_line_rows: Vec::new(),
        reference_line_rows: Vec::new(),
        bounded_curve_rows: Vec::new(),
        conic_rows: Vec::new(),
        opaque_rows: Vec::new(),
        offset: 19,
    });
    definition
        .dimensions
        .as_mut()
        .expect("dimension table")
        .rows[0]
        .dimension_type = 3;
    assert_eq!(
        resolved_section_radii(&definition),
        BTreeMap::from([(0, 5.0)])
    );
    let radius = section_segment_radius_constraints(&definition, &sketch_917);
    assert_eq!(radius.len(), 1);
    assert_eq!(
        radius[0].0.definition,
        SketchConstraintDefinition::Radius {
            entity: SketchEntityId("creo:featdefs:sketch_entity#917:42".to_string()),
            parameter: ParameterId("creo:featdefs:parameter#917:3".to_string()),
        }
    );
    let retained_without_circle =
        section_segment_radius_constraints_for_emitted(&definition, &sketch_917, &BTreeSet::new());
    assert_eq!(retained_without_circle.len(), 1);
    let SketchConstraintDefinition::Native {
        native_kind,
        native_properties,
        entities,
        operands,
        ..
    } = &retained_without_circle[0].0.definition
    else {
        panic!("a missing circle entity must retain its native radius relation");
    };
    assert_eq!(native_kind, "creo:segtab:radius");
    assert_eq!(native_properties["dimension_ordinal"], "0");
    assert!(entities.is_empty());
    assert_eq!(operands[0].native_field.as_deref(), Some("ext_id"));
    assert_eq!(operands[0].object_index, 42);
    assert_eq!(operands[1].native_field.as_deref(), Some("radius"));
    assert_eq!(operands[1].object_index, 0);
    definition
        .dimensions
        .as_mut()
        .expect("dimension table")
        .rows[0]
        .dimension_type = 4;
    assert_eq!(
        resolved_section_radii(&definition),
        BTreeMap::from([(0, 2.5)])
    );
    let diameter = section_segment_radius_constraints(&definition, &sketch_917);
    assert_eq!(diameter.len(), 1);
    assert_eq!(
        diameter[0].0.definition,
        SketchConstraintDefinition::Diameter {
            entity: SketchEntityId("creo:featdefs:sketch_entity#917:42".to_string()),
            parameter: ParameterId("creo:featdefs:parameter#917:3".to_string()),
        }
    );
    definition
        .segments
        .as_mut()
        .expect("segment table")
        .declared_count = 2;
    assert_eq!(
        resolved_section_radii(&definition),
        BTreeMap::from([(0, 2.5)])
    );
    assert_eq!(
        section_segment_radius_constraints(&definition, &sketch_917)[0]
            .0
            .definition,
        SketchConstraintDefinition::Diameter {
            entity: SketchEntityId("creo:featdefs:sketch_entity#917:42".to_string()),
            parameter: ParameterId("creo:featdefs:parameter#917:3".to_string()),
        }
    );
    definition
        .segments
        .as_mut()
        .expect("segment table")
        .declared_count = 1;
    definition
        .dimensions
        .as_mut()
        .expect("dimension table")
        .rows[0]
        .dimension_type = 2;
    assert!(resolved_section_radii(&definition).is_empty());
    let unresolved_kind = section_segment_radius_constraints(&definition, &sketch_917);
    assert!(matches!(
        unresolved_kind[0].0.definition,
        SketchConstraintDefinition::Native { .. }
    ));
    let segments = definition.segments.as_mut().expect("segment table");
    let circle = segments.circle_rows.remove(0);
    segments
        .opaque_rows
        .push(crate::feature::FeatureOpaqueSegment {
            kind: 10,
            directions: [None; 3],
            point_ids: [None, Some(1)],
            center_id: Some(circle.center_id),
            arc_orientation: Some(0),
            vertical_horizontal: Some(0),
            radius_ref: Some(circle.radius_ref),
            radius2_ref: Some(7),
            external_id: circle.external_id,
            body: Vec::new(),
            offset: circle.offset,
        });
    let retained_slots = section_segment_radius_constraints(&definition, &sketch_917);
    assert_eq!(retained_slots.len(), 2);
    let secondary = retained_slots
        .iter()
        .find(|(constraint, _)| constraint.id.0.ends_with("radius2:42"))
        .expect("secondary radius binding");
    let SketchConstraintDefinition::Native {
        native_kind,
        native_properties,
        entities,
        operands,
        ..
    } = &secondary.0.definition
    else {
        panic!("secondary radius binding must remain native");
    };
    assert_eq!(native_kind, "creo:segtab:radius2");
    assert_eq!(native_properties["dimension_ordinal"], "7");
    assert_eq!(
        entities,
        &[SketchEntityId(
            "creo:featdefs:sketch_entity#917:42".to_string()
        )]
    );
    assert_eq!(operands[0].native_field.as_deref(), Some("ext_id"));
    assert_eq!(operands[0].object_index, 42);
    assert_eq!(operands[1].native_field.as_deref(), Some("radius2"));
    assert_eq!(operands[1].object_index, 7);
    definition
        .segments
        .as_mut()
        .expect("segment table")
        .opaque_rows[0]
        .radius2_ref = None;
    definition
        .segments
        .as_mut()
        .expect("segment table")
        .rows
        .push(crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Arc,
            directions: [None; 3],
            point_ids: [1, 2],
            center_id: Some(7),
            arc_orientation: Some(0),
            vertical_horizontal: None,
            radius_ref: Some(8),
            radius2_ref: Some(9),
            external_id: 43,
            body: Vec::new(),
            offset: 21,
        });
    let typed_slots = section_segment_radius_constraints(&definition, &sketch_917);
    assert!(typed_slots.iter().any(|(constraint, _)| {
        constraint.id.0.ends_with("segtab-radius:43")
            && matches!(
                &constraint.definition,
                SketchConstraintDefinition::Native {
                    native_properties,
                    ..
                } if native_properties["dimension_ordinal"] == "8"
            )
    }));
    assert!(typed_slots.iter().any(|(constraint, _)| {
        constraint.id.0.ends_with("segtab-radius2:43")
            && matches!(
                &constraint.definition,
                SketchConstraintDefinition::Native {
                    native_properties,
                    ..
                } if native_properties["dimension_ordinal"] == "9"
            )
    }));
    definition
        .segments
        .as_mut()
        .expect("segment table")
        .rows
        .clear();
    let segments = definition.segments.as_mut().expect("segment table");
    segments.opaque_rows.clear();
    segments.circle_rows.push(circle);
    definition
        .dimensions
        .as_mut()
        .expect("dimension table")
        .rows[0]
        .dimension_type = 4;
    assert_eq!(
        section_circle_geometry(
            &BTreeMap::from([(7, [1.0, 2.0])]),
            &resolved_section_radii(&definition),
            &definition.segments.as_ref().expect("segments").circle_rows[0],
        ),
        Some(SketchGeometry::Circle {
            center: Point2::new(1.0, 2.0),
            radius: Length(2.5),
        })
    );
    let unresolved_dimension = crate::feature::FeatureDimension {
        value: None,
        value_body: Vec::new(),
        external_id: 4,
        ..dimension.clone()
    };
    let unresolved_table = crate::feature::FeatureDimensionTable {
        rows: vec![unresolved_dimension.clone()],
        ..table.clone()
    };
    assert_eq!(
        resolved_feature_dimension_parameter(&sketch_917, &unresolved_table, 0),
        Some((
            &unresolved_dimension,
            ParameterId("creo:featdefs:parameter#917:4".to_string())
        ))
    );
    let incomplete_table = crate::feature::FeatureDimensionTable {
        declared_count: 2,
        ..unresolved_table
    };
    assert_eq!(
        resolved_feature_dimension_parameter(&sketch_917, &incomplete_table, 0),
        None
    );
    table.rows.push(dimension);
    definition.dimensions = Some(table);
    assert_eq!(
        resolved_feature_dimension_parameter(
            &sketch_917,
            definition.dimensions.as_ref().expect("dimension table"),
            0,
        ),
        None
    );
    assert_eq!(
        resolved_feature_dimension_parameter(
            &sketch_917,
            definition.dimensions.as_ref().expect("dimension table"),
            1,
        ),
        None
    );
}

#[test]
fn dimension_display_preserves_radius_and_diameter_types() {
    assert_eq!(
        feature_dimension_display(0x03),
        Some(DimensionDisplay::Radius)
    );
    assert_eq!(
        feature_dimension_display(0x04),
        Some(DimensionDisplay::Diameter)
    );
    assert_eq!(feature_dimension_display(0x02), None);
    assert_eq!(feature_dimension_display(0x0a), None);
}

#[test]
fn evaluated_sweep_bodies_are_feature_outputs() {
    let mut ir = CadIr::empty(Units::default());
    for id in [
        "creo:feature:extrusion#40:body",
        "creo:feature:revolution#40:body",
        "creo:feature:revolution#41:body",
    ] {
        ir.model.bodies.push(Body {
            id: BodyId(id.to_string()),
            kind: BodyKind::Solid,
            regions: Vec::new(),
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
    }
    ir.model.bodies.push(Body {
        id: BodyId("creo:feature:extrusion#43:body".to_string()),
        kind: BodyKind::Sheet,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    assert_eq!(
        evaluated_sweep_output_bodies(&ir, 40),
        vec![
            BodyId("creo:feature:extrusion#40:body".to_string()),
            BodyId("creo:feature:revolution#40:body".to_string()),
        ]
    );
    assert_eq!(
        evaluated_sweep_body_kind(&ir, "extrusion", 40),
        Some(BodyKind::Solid)
    );
    assert_eq!(
        evaluated_sweep_body_kind(&ir, "revolution", 40),
        Some(BodyKind::Solid)
    );
    assert_eq!(
        evaluated_sweep_body_kind(&ir, "extrusion", 43),
        Some(BodyKind::Sheet)
    );
    assert_eq!(evaluated_sweep_body_kind(&ir, "revolution", 42), None);
}
