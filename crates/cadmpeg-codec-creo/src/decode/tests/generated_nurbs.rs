// SPDX-License-Identifier: Apache-2.0
//! Tests: generated nurbs.

use super::section_axis_line_carrier;
use crate::decode::feature_history::{
    agreed_feature_affected_ids, agreed_feature_geometry_ids, agreed_feature_replay_edge_ids,
    agreed_feature_replay_geometry_ids,
};
use crate::decode::sketch::{
    intersect_section_line_arc, intersect_section_lines, intersect_tangent_section_arcs,
    resolved_section_coordinates, resolved_section_points, resolved_section_radii,
    resolved_section_scalar_values, section_axis_reference_line_geometry, section_line_geometry,
    section_point_geometry,
};
use crate::decode::sketch_transfer::{
    current_feature_operation, current_feature_recipe, current_feature_recipe_parent,
    feature_is_first_material_operation, first_material_feature_by_definition_order,
    reconcile_constraint_entity_references, reconcile_constraint_parameter_reference,
    resolved_feature_schema_class_from_classes, row_feature_schema_classes,
    section_equation_same_coordinate_constraints, unique_feature_revolution_extent_kind,
};
use crate::decode::sweep::{generated_nurbs_translation_extent, nurbs_translation_span};
use crate::decode::uniqueness::{
    unique_feature_section_transform, unique_owned_feature_definition,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{Angle, ExtrudeExtent, ExtrudeSide, Length, ParameterId, Termination};
use cadmpeg_ir::geometry::{NurbsSurface, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::{
    SketchConstraintDefinition, SketchEntityId, SketchGeometry, SketchLocus,
};
use cadmpeg_ir::units::Units;
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn generated_nurbs_translations_define_a_blind_extrusion() {
    let translated_surface = |last_z| NurbsSurface {
        u_degree: 2,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 3,
        v_count: 2,
        control_points: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 0.0, 2.0),
            Point3::new(1.0, 1.0, 0.0),
            Point3::new(1.0, 1.0, 2.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, last_z),
        ],
        weights: None,
        u_periodic: false,
        v_periodic: false,
    };
    let span = nurbs_translation_span(&translated_surface(2.0)).expect("translation");
    assert_eq!(span.vector, [0.0, 0.0, 2.0]);
    assert_eq!(
        span.starts,
        vec![[0.0, 0.0, 0.0], [1.0, 1.0, 0.0], [2.0, 0.0, 0.0]]
    );
    assert!(nurbs_translation_span(&translated_surface(3.0)).is_none());

    let row = |id, kind: crate::surface::SurfaceKind| crate::surface::SurfaceRow {
        id,
        type_byte: kind.canonical_type_byte(),
        kind,
        feature_id: 7,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: id as usize,
    };
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.surfaces.rows.extend([
        row(31, crate::surface::SurfaceKind::Extrusion),
        row(32, crate::surface::SurfaceKind::Plane),
        row(33, crate::surface::SurfaceKind::Plane),
        row(34, crate::surface::SurfaceKind::Extrusion),
        row(35, crate::surface::SurfaceKind::Plane),
    ]);
    let mut ir = CadIr::empty(Units::default());
    ir.model.surfaces.extend([
        Surface {
            id: SurfaceId("creo:visibgeom:surface#31".to_string()),
            geometry: SurfaceGeometry::Nurbs(translated_surface(2.0)),
            source_object: None,
        },
        Surface {
            id: SurfaceId("creo:visibgeom:surface#32".to_string()),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
        Surface {
            id: SurfaceId("creo:visibgeom:surface#33".to_string()),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 2.0),
                normal: Vector3::new(0.0, 0.0, -1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
        Surface {
            id: SurfaceId("creo:visibgeom:surface#34".to_string()),
            geometry: SurfaceGeometry::Unknown { record: None },
            source_object: None,
        },
        Surface {
            id: SurfaceId("creo:visibgeom:surface#35".to_string()),
            geometry: SurfaceGeometry::Unknown { record: None },
            source_object: None,
        },
    ]);
    assert_eq!(
        generated_nurbs_translation_extent(&scan, &ir, 7, None),
        Some((
            ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(2.0),
                    },
                    draft: None,
                    offset: None,
                },
            },
            [0.0, 0.0, 1.0],
        ))
    );

    let mut ambiguous = translated_surface(2.0);
    ambiguous.u_degree = 1;
    ambiguous.u_count = 2;
    ambiguous.u_knots = vec![0.0, 0.0, 1.0, 1.0];
    ambiguous.control_points.truncate(4);
    assert!(nurbs_translation_span(&ambiguous).is_none());
}

#[test]
fn equation_function_two_joins_coordinate_rows_by_position() {
    let definition = crate::feature::FeatureDefinition {
        id: 40,
        owner_feature_id: None,
        body: b"eqtn_arr\0\xf2\xf8\x02\xf7\x80\x9f\xfb\xe2\
                \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2\
                \x01\x02\xf8\x02\x00\x01\xf6\xe2"
            .to_vec(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(crate::feature::FeatureVariableTable {
            declared_count: 2,
            entity_ref: None,
            rows: vec![
                crate::feature::FeatureVariableRow {
                    variable_type: 1,
                    key: 7,
                    value: Some(4.0),
                    value_body: Vec::new(),
                    guess: Some(4.0),
                    guess_body: Vec::new(),
                    guess_dimension_driven: false,
                    known: Some(0),
                    homogeneity: Some(1),
                    uvar_id: Some(10),
                    dimension_driven: false,
                    offset: 0,
                },
                crate::feature::FeatureVariableRow {
                    variable_type: 1,
                    key: 8,
                    value: None,
                    value_body: Vec::new(),
                    guess: None,
                    guess_body: Vec::new(),
                    guess_dimension_driven: true,
                    known: Some(0),
                    homogeneity: Some(1),
                    uvar_id: Some(20),
                    dimension_driven: true,
                    offset: 0,
                },
            ],
            points: Vec::new(),
            offset: 0,
        }),
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 0,
    };

    assert_eq!(
        resolved_section_coordinates(&definition).get(&8),
        Some(&[Some(4.0), None])
    );
}

#[test]
fn equation_function_two_propagates_non_coordinate_scalar_components() {
    let row = |key, value, dimension_driven| crate::feature::FeatureVariableRow {
        variable_type: 6,
        key,
        value,
        value_body: Vec::new(),
        guess: value,
        guess_body: Vec::new(),
        guess_dimension_driven: dimension_driven,
        known: Some(0),
        homogeneity: Some(0),
        uvar_id: None,
        dimension_driven,
        offset: 0,
    };
    let definition = |middle_value, last_value| crate::feature::FeatureDefinition {
        id: 40,
        owner_feature_id: None,
        body: b"eqtn_arr\0\xf2\xf8\x03\xf7\x80\x9f\xfb\xe2\
                \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2\
                \x01\x02\xf8\x02\x00\x01\xf6\xe2\
                \x02\x02\xf8\x02\x01\x02\xf6\xe2"
            .to_vec(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(crate::feature::FeatureVariableTable {
            declared_count: 3,
            entity_ref: None,
            rows: vec![
                row(10, None, true),
                row(11, middle_value, middle_value.is_none()),
                row(12, last_value, last_value.is_none()),
            ],
            points: Vec::new(),
            offset: 0,
        }),
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 0,
    };

    let resolved = resolved_section_scalar_values(&definition(None, Some(2.5)));
    assert_eq!(resolved.get(&(6, 10)), Some(&2.5));
    assert_eq!(resolved.get(&(6, 11)), Some(&2.5));
    assert_eq!(resolved.get(&(6, 12)), Some(&2.5));

    let conflicting = resolved_section_scalar_values(&definition(Some(2.5), Some(3.5)));
    assert!(!conflicting.contains_key(&(6, 10)));
    assert!(!conflicting.contains_key(&(6, 11)));
}

#[test]
fn equation_function_five_propagates_direct_type_six_equality() {
    let row = |variable_type, key, value, dimension_driven| crate::feature::FeatureVariableRow {
        variable_type,
        key,
        value,
        value_body: Vec::new(),
        guess: value,
        guess_body: Vec::new(),
        guess_dimension_driven: dimension_driven,
        known: Some(0),
        homogeneity: Some(0),
        uvar_id: None,
        dimension_driven,
        offset: 0,
    };
    let definition =
        |first_value, second_value, selector_value| crate::feature::FeatureDefinition {
            id: 40,
            owner_feature_id: None,
            body: b"eqtn_arr\0\xf2\xf8\x02\xf7\x80\x9f\xfb\xe2\
                    \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2\
                    \x01\x05\xf8\x03\x00\x01\x02\xf6\xe2"
                .to_vec(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: Some(crate::feature::FeatureVariableTable {
                declared_count: 3,
                entity_ref: None,
                rows: vec![
                    row(6, 10, first_value, first_value.is_none()),
                    row(6, 11, second_value, second_value.is_none()),
                    row(5, 0, selector_value, false),
                ],
                points: Vec::new(),
                offset: 0,
            }),
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 0,
        };

    let resolved = resolved_section_scalar_values(&definition(None, Some(2.5), Some(0.0)));
    assert_eq!(resolved.get(&(6, 10)), Some(&2.5));
    assert_eq!(resolved.get(&(6, 11)), Some(&2.5));

    let conflicting = resolved_section_scalar_values(&definition(Some(2.5), Some(3.5), Some(0.0)));
    assert!(!conflicting.contains_key(&(6, 10)));
    assert!(!conflicting.contains_key(&(6, 11)));
    assert!(
        !resolved_section_scalar_values(&definition(None, Some(2.5), None)).contains_key(&(6, 10))
    );
    assert!(
        !resolved_section_scalar_values(&definition(None, Some(2.5), Some(1.0)))
            .contains_key(&(6, 10))
    );
}

#[test]
fn equation_function_two_propagates_radius_components() {
    let row = |key, value| crate::feature::FeatureVariableRow {
        variable_type: 3,
        key,
        value,
        value_body: Vec::new(),
        guess: value,
        guess_body: Vec::new(),
        guess_dimension_driven: value.is_none(),
        known: Some(0),
        homogeneity: Some(1),
        uvar_id: None,
        dimension_driven: value.is_none(),
        offset: 0,
    };
    let definition = |first_value, second_value| crate::feature::FeatureDefinition {
        id: 40,
        owner_feature_id: None,
        body: b"eqtn_arr\0\xf2\xf8\x02\xf7\x80\x9f\xfb\xe2\
                \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2\
                \x01\x02\xf8\x02\x00\x01\xf6\xe2"
            .to_vec(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(crate::feature::FeatureVariableTable {
            declared_count: 2,
            entity_ref: None,
            rows: vec![row(42, first_value), row(43, second_value)],
            points: Vec::new(),
            offset: 0,
        }),
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 0,
    };

    assert_eq!(
        resolved_section_radii(&definition(None, Some(2.5))),
        BTreeMap::from([(42, 2.5), (43, 2.5)])
    );
    assert!(resolved_section_radii(&definition(Some(2.5), Some(3.5))).is_empty());
    assert!(resolved_section_radii(&definition(Some(0.0), Some(2.5))).is_empty());
}

#[test]
fn equation_function_two_binds_radius_row_to_dimension_row() {
    let definition = crate::feature::FeatureDefinition {
        id: 40,
        owner_feature_id: None,
        body: b"eqtn_arr\0\xf2\xf8\x02\xf7\x80\x9f\xfb\xe2\
                \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2\
                \x01\x02\xf8\x02\x00\x01\xf6\xe2"
            .to_vec(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(crate::feature::FeatureVariableTable {
            declared_count: 2,
            entity_ref: None,
            rows: vec![
                crate::feature::FeatureVariableRow {
                    variable_type: 3,
                    key: 42,
                    value: None,
                    value_body: Vec::new(),
                    guess: None,
                    guess_body: Vec::new(),
                    guess_dimension_driven: true,
                    known: Some(0),
                    homogeneity: Some(1),
                    uvar_id: Some(7),
                    dimension_driven: true,
                    offset: 0,
                },
                crate::feature::FeatureVariableRow {
                    variable_type: 0,
                    key: 0,
                    value: Some(5.0),
                    value_body: Vec::new(),
                    guess: Some(5.0),
                    guess_body: Vec::new(),
                    guess_dimension_driven: false,
                    known: Some(0),
                    homogeneity: Some(0),
                    uvar_id: None,
                    dimension_driven: false,
                    offset: 0,
                },
            ],
            points: Vec::new(),
            offset: 0,
        }),
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: Some(crate::feature::FeatureDimensionTable {
            declared_count: 1,
            entity_ref: None,
            rows: vec![crate::feature::FeatureDimension {
                dimension_type: 3,
                value: Some(5.0),
                value_body: Vec::new(),
                unresolved_value_token: None,
                value_unit: crate::feature::DimensionUnit::Millimeters,
                direction_byte: 0,
                auxiliary_value: None,
                auxiliary_body: Vec::new(),
                external_id: 100,
                references: None,
                offset: 0,
            }],
            offset: 0,
        }),
        relations: None,
        saved_section: None,
        offset: 0,
    };

    assert_eq!(
        resolved_section_radii(&definition),
        BTreeMap::from([(42, 5.0)])
    );

    let mut dimension_driven = definition.clone();
    let dimension_scalar = &mut dimension_driven.variables.as_mut().expect("variables").rows[1];
    dimension_scalar.value = None;
    dimension_scalar.guess = None;
    dimension_scalar.guess_dimension_driven = true;
    dimension_scalar.dimension_driven = true;
    assert_eq!(
        resolved_section_radii(&dimension_driven),
        BTreeMap::from([(42, 5.0)])
    );
    assert_eq!(
        resolved_section_scalar_values(&dimension_driven).get(&(0, 0)),
        Some(&5.0)
    );

    let mut missing_inline = definition.clone();
    let missing_scalar = &mut missing_inline.variables.as_mut().expect("variables").rows[1];
    missing_scalar.value = None;
    missing_scalar.guess = None;
    assert!(resolved_section_radii(&missing_inline).is_empty());

    let mut mismatched = definition;
    mismatched
        .dimensions
        .as_mut()
        .expect("dimension table")
        .rows[0]
        .value = Some(6.0);
    assert!(resolved_section_radii(&mismatched).is_empty());
}

#[test]
fn equation_function_forty_two_transfers_midpoint_coordinates_and_scalar() {
    let row = |variable_type, key, value| crate::feature::FeatureVariableRow {
        variable_type,
        key,
        value,
        value_body: Vec::new(),
        guess: value,
        guess_body: Vec::new(),
        guess_dimension_driven: value.is_none(),
        known: Some(0),
        homogeneity: Some(1),
        uvar_id: None,
        dimension_driven: value.is_none(),
        offset: 0,
    };
    let definition = |first, second, midpoint| crate::feature::FeatureDefinition {
        id: 40,
        owner_feature_id: None,
        body: b"eqtn_arr\0\xf2\xf8\x02\xf7\x80\x9f\xfb\xe2\
                \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2\
                \x01\x2a\xf8\x03\x00\x01\x02\xf6\xe2"
            .to_vec(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(crate::feature::FeatureVariableTable {
            declared_count: 3,
            entity_ref: None,
            rows: vec![row(1, 10, first), row(1, 11, second), row(6, 20, midpoint)],
            points: Vec::new(),
            offset: 0,
        }),
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 0,
    };

    assert_eq!(
        resolved_section_coordinates(&definition(Some(2.0), None, Some(5.0))).get(&11),
        Some(&[Some(8.0), None])
    );
    assert_eq!(
        resolved_section_scalar_values(&definition(Some(2.0), Some(8.0), None)).get(&(6, 20)),
        Some(&5.0)
    );

    let conflicting = definition(Some(2.0), Some(9.0), Some(5.0));
    assert!(!resolved_section_scalar_values(&conflicting).contains_key(&(6, 20)));
}

#[test]
fn equation_function_thirty_one_transfers_point_coordinates_and_scalars() {
    let row = |variable_type, key, value| crate::feature::FeatureVariableRow {
        variable_type,
        key,
        value,
        value_body: Vec::new(),
        guess: value,
        guess_body: Vec::new(),
        guess_dimension_driven: value.is_none(),
        known: Some(0),
        homogeneity: Some(1),
        uvar_id: None,
        dimension_driven: value.is_none(),
        offset: 0,
    };
    let definition = |u, v, first, second| crate::feature::FeatureDefinition {
        id: 40,
        owner_feature_id: None,
        body: b"eqtn_arr\0\xf2\xf8\x02\xf7\x80\x9f\xfb\xe2\
                \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2\
                \x01\x1f\xf8\x04\x00\x01\x02\x03\xf6\xe2"
            .to_vec(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(crate::feature::FeatureVariableTable {
            declared_count: 4,
            entity_ref: None,
            rows: vec![
                row(1, 10, u),
                row(2, 10, v),
                row(6, 20, first),
                row(6, 21, second),
            ],
            points: Vec::new(),
            offset: 0,
        }),
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 0,
    };

    assert_eq!(
        resolved_section_coordinates(&definition(None, None, Some(3.0), Some(4.0))).get(&10),
        Some(&[Some(3.0), Some(4.0)])
    );
    let partial = definition(None, Some(4.0), Some(3.0), None);
    assert_eq!(
        resolved_section_coordinates(&partial).get(&10),
        Some(&[Some(3.0), Some(4.0)])
    );
    assert_eq!(
        resolved_section_scalar_values(&partial).get(&(6, 21)),
        Some(&4.0)
    );
    let resolved = resolved_section_scalar_values(&definition(Some(3.0), Some(4.0), None, None));
    assert_eq!(resolved.get(&(6, 20)), Some(&3.0));
    assert_eq!(resolved.get(&(6, 21)), Some(&4.0));
}

#[test]
fn equation_function_six_derives_positive_point_distance() {
    let row = |variable_type, key, value| crate::feature::FeatureVariableRow {
        variable_type,
        key,
        value,
        value_body: Vec::new(),
        guess: value,
        guess_body: Vec::new(),
        guess_dimension_driven: value.is_none(),
        known: Some(0),
        homogeneity: Some(1),
        uvar_id: None,
        dimension_driven: value.is_none(),
        offset: 0,
    };
    let definition = |radius| crate::feature::FeatureDefinition {
        id: 40,
        owner_feature_id: None,
        body: b"eqtn_arr\0\xf2\xf8\x02\xf7\x80\x9f\xfb\xe2\
                \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2\
                \x01\x06\xf8\x05\x00\x01\x02\x03\x04\xf6\xe2"
            .to_vec(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(crate::feature::FeatureVariableTable {
            declared_count: 5,
            entity_ref: None,
            rows: vec![
                row(1, 10, Some(0.0)),
                row(2, 10, Some(0.0)),
                row(1, 11, Some(3.0)),
                row(2, 11, Some(4.0)),
                row(3, 20, radius),
            ],
            points: Vec::new(),
            offset: 0,
        }),
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 0,
    };

    assert_eq!(
        resolved_section_scalar_values(&definition(None)).get(&(3, 20)),
        Some(&5.0)
    );
    assert_eq!(
        resolved_section_radii(&definition(None)).get(&20),
        Some(&5.0)
    );
    assert!(!resolved_section_scalar_values(&definition(Some(6.0))).contains_key(&(3, 20)));
    assert_eq!(
        resolved_section_radii(&definition(Some(6.0))).get(&20),
        Some(&6.0)
    );

    let mut stored_without_coordinates = definition(Some(5.0));
    for row in &mut stored_without_coordinates
        .variables
        .as_mut()
        .expect("variables")
        .rows[..4]
    {
        row.value = None;
        row.guess = None;
    }
    assert!(!resolved_section_scalar_values(&stored_without_coordinates).contains_key(&(3, 20)));
}

#[test]
fn equation_function_forty_three_derives_unique_axis_distance_scalar() {
    let row = |variable_type, key, value| crate::feature::FeatureVariableRow {
        variable_type,
        key,
        value,
        value_body: Vec::new(),
        guess: value,
        guess_body: Vec::new(),
        guess_dimension_driven: value.is_none(),
        known: Some(0),
        homogeneity: Some(1),
        uvar_id: None,
        dimension_driven: value.is_none(),
        offset: 0,
    };
    let definition =
        |first: [f64; 2], second: [f64; 2], distance| crate::feature::FeatureDefinition {
            id: 40,
            owner_feature_id: None,
            body: b"eqtn_arr\0\xf2\xf8\x02\xf7\x80\x9f\xfb\xe2\
                    \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2\
                    \x01\x2b\xf8\x08\x00\x01\x02\x03\x04\x05\x06\x07\xf6\xe2"
                .to_vec(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: Some(crate::feature::FeatureVariableTable {
                declared_count: 8,
                entity_ref: None,
                rows: vec![
                    row(1, 10, Some(first[0])),
                    row(2, 10, Some(first[1])),
                    row(1, 11, Some(second[0])),
                    row(2, 11, Some(second[1])),
                    row(4, 2, Some(0.0)),
                    row(5, 0, Some(0.0)),
                    row(0, 20, distance),
                    row(5, 1, Some(0.0)),
                ],
                points: Vec::new(),
                offset: 0,
            }),
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 0,
        };

    assert_eq!(
        resolved_section_scalar_values(&definition([0.0, 0.0], [3.0, 0.0], None)).get(&(0, 20)),
        Some(&3.0)
    );
    assert_eq!(
        resolved_section_scalar_values(&definition([0.0, 0.0], [3.0, 4.0], Some(4.0)))
            .get(&(0, 20)),
        Some(&4.0)
    );
    assert!(
        !resolved_section_scalar_values(&definition([0.0, 0.0], [3.0, 4.0], None))
            .contains_key(&(0, 20))
    );
    assert!(
        !resolved_section_scalar_values(&definition([0.0, 0.0], [3.0, 4.0], Some(5.0)))
            .contains_key(&(0, 20))
    );
    assert!(
        !resolved_section_scalar_values(&definition([0.0, 0.0], [3.0, 3.0], Some(3.0)))
            .contains_key(&(0, 20))
    );

    let mut invalid_auxiliary = definition([0.0, 0.0], [3.0, 0.0], None);
    invalid_auxiliary
        .variables
        .as_mut()
        .expect("variables")
        .rows[5]
        .value = Some(1.0);
    assert!(!resolved_section_scalar_values(&invalid_auxiliary).contains_key(&(0, 20)));
}

#[test]
fn equation_function_sixteen_derives_direct_angle_difference() {
    let row = |variable_type, key, value| crate::feature::FeatureVariableRow {
        variable_type,
        key,
        value,
        value_body: Vec::new(),
        guess: value,
        guess_body: Vec::new(),
        guess_dimension_driven: value.is_none(),
        known: Some(0),
        homogeneity: Some(1),
        uvar_id: None,
        dimension_driven: value.is_none(),
        offset: 0,
    };
    let definition = |first, second, difference, selector| crate::feature::FeatureDefinition {
        id: 40,
        owner_feature_id: None,
        body: b"eqtn_arr\0\xf2\xf8\x02\xf7\x80\x9f\xfb\xe2\
                \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2\
                \x01\x10\xf8\x04\x00\x01\x02\x03\xf6\xe2"
            .to_vec(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(crate::feature::FeatureVariableTable {
            declared_count: 4,
            entity_ref: None,
            rows: vec![
                row(4, 10, first),
                row(4, 11, second),
                row(0, 20, difference),
                row(5, 0, selector),
            ],
            points: Vec::new(),
            offset: 0,
        }),
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 0,
    };

    assert_eq!(
        resolved_section_scalar_values(&definition(Some(2.5), Some(1.0), None, Some(0.0)))
            .get(&(0, 20)),
        Some(&1.5)
    );
    assert_eq!(
        resolved_section_scalar_values(&definition(Some(2.5), Some(1.0), Some(1.5), Some(0.0),))
            .get(&(0, 20)),
        Some(&1.5)
    );
    assert!(!resolved_section_scalar_values(&definition(
        Some(2.5),
        Some(1.0),
        Some(1.0),
        Some(0.0),
    ))
    .contains_key(&(0, 20)));
    assert!(
        !resolved_section_scalar_values(&definition(Some(2.5), Some(1.0), None, Some(1.0)))
            .contains_key(&(0, 20))
    );
    assert!(
        !resolved_section_scalar_values(&definition(Some(1.0), Some(2.5), None, Some(0.0)))
            .contains_key(&(0, 20))
    );
    assert!(
        !resolved_section_scalar_values(&definition(Some(4.0), Some(0.0), None, Some(0.0)))
            .contains_key(&(0, 20))
    );
}

#[test]
fn equation_function_zero_solves_radial_endpoint_and_opaque_scalars() {
    let variable = |variable_type, key, value| crate::feature::FeatureVariableRow {
        variable_type,
        key,
        value,
        value_body: Vec::new(),
        guess: value,
        guess_body: Vec::new(),
        guess_dimension_driven: false,
        known: Some(0),
        homogeneity: Some(1),
        uvar_id: None,
        dimension_driven: false,
        offset: 0,
    };
    let definition = |second: [Option<f64>; 2], radius: Option<f64>, angle: Option<f64>| {
        crate::feature::FeatureDefinition {
            id: 40,
            owner_feature_id: None,
            body: b"eqtn_arr\0\xf2\xf8\x02\xf7\x80\x9f\xfb\xe2\
                \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2\
                \x01\x00\xf8\x06\x00\x01\x02\x03\x04\x05\xf6\xe2"
                .to_vec(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: Some(crate::feature::FeatureVariableTable {
                declared_count: 6,
                entity_ref: None,
                rows: vec![
                    variable(1, 1, Some(0.0)),
                    variable(2, 1, Some(0.0)),
                    variable(1, 2, second[0]),
                    variable(2, 2, second[1]),
                    variable(3, 9, radius),
                    variable(6, 10, angle),
                ],
                points: Vec::new(),
                offset: 0,
            }),
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 0,
        }
    };

    let solved = definition([None, None], Some(2.0), Some(std::f64::consts::FRAC_PI_2));
    let solved_point = resolved_section_points(&solved)
        .get(&2)
        .copied()
        .expect("point");
    assert!(solved_point[0].abs() <= 1.0e-12);
    assert!((solved_point[1] - 2.0).abs() <= 1.0e-12);
    assert_eq!(
        resolved_section_scalar_values(&solved).get(&(3, 9)),
        Some(&2.0)
    );
    assert_eq!(
        resolved_section_scalar_values(&solved).get(&(6, 10)),
        Some(&std::f64::consts::FRAC_PI_2)
    );

    let derived_angle = definition([Some(0.0), Some(2.0)], Some(2.0), None);
    assert_eq!(
        resolved_section_scalar_values(&derived_angle).get(&(6, 10)),
        Some(&std::f64::consts::FRAC_PI_2)
    );

    let derived_radius = definition(
        [Some(0.0), Some(2.0)],
        None,
        Some(std::f64::consts::FRAC_PI_2),
    );
    assert_eq!(resolved_section_radii(&derived_radius).get(&9), Some(&2.0));
}

#[test]
fn equation_function_three_solves_unique_unsigned_coordinate_distance() {
    let variable =
        |variable_type, key, value, dimension_driven| crate::feature::FeatureVariableRow {
            variable_type,
            key,
            value,
            value_body: Vec::new(),
            guess: value,
            guess_body: Vec::new(),
            guess_dimension_driven: dimension_driven,
            known: Some(0),
            homogeneity: Some(1),
            uvar_id: None,
            dimension_driven,
            offset: 0,
        };
    let dimension = |value| crate::feature::FeatureDimension {
        dimension_type: 1,
        value: Some(value),
        value_body: Vec::new(),
        unresolved_value_token: None,
        value_unit: crate::feature::DimensionUnit::Millimeters,
        direction_byte: 0,
        auxiliary_value: None,
        auxiliary_body: Vec::new(),
        external_id: 0,
        references: None,
        offset: 0,
    };
    let definition = crate::feature::FeatureDefinition {
        id: 40,
        owner_feature_id: None,
        body: b"eqtn_arr\0\xf2\xf8\x03\xf7\x80\x9f\xfb\xe2\
                \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2\
                \x01\x03\xf8\x03\x00\x01\x03\xf6\xe2\
                \x02\x03\xf8\x03\x01\x02\x04\xf6\xe2"
            .to_vec(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(crate::feature::FeatureVariableTable {
            declared_count: 5,
            entity_ref: None,
            rows: vec![
                variable(1, 1, Some(0.0), false),
                variable(1, 2, None, true),
                variable(1, 3, Some(10.0), false),
                variable(0, 0, Some(5.0), false),
                variable(0, 1, Some(5.0), false),
            ],
            points: Vec::new(),
            offset: 0,
        }),
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: Some(crate::feature::FeatureDimensionTable {
            declared_count: 2,
            entity_ref: None,
            rows: vec![dimension(5.0), dimension(5.0)],
            offset: 0,
        }),
        relations: None,
        saved_section: None,
        offset: 0,
    };

    assert_eq!(
        resolved_section_coordinates(&definition).get(&2),
        Some(&[Some(5.0), None])
    );

    let mut dimension_driven = definition.clone();
    let dimension_scalar = &mut dimension_driven.variables.as_mut().expect("variables").rows[3];
    dimension_scalar.value = None;
    dimension_scalar.guess = None;
    dimension_scalar.guess_dimension_driven = true;
    dimension_scalar.dimension_driven = true;
    assert_eq!(
        resolved_section_coordinates(&dimension_driven).get(&2),
        Some(&[Some(5.0), None])
    );
    assert_eq!(
        resolved_section_scalar_values(&dimension_driven).get(&(0, 0)),
        Some(&5.0)
    );

    let mut missing_inline = definition.clone();
    let missing_scalar = &mut missing_inline.variables.as_mut().expect("variables").rows[3];
    missing_scalar.value = None;
    missing_scalar.guess = None;
    assert!(!resolved_section_scalar_values(&missing_inline).contains_key(&(0, 0)));

    let equation_id = crate::feature::equation_table(&definition.body, 0, definition.body.len())
        .expect("equation table")
        .rows
        .iter()
        .find(|equation| equation.function_id == 3)
        .expect("function-three equation")
        .equation_id;
    let mut disabled_equation = definition.clone();
    disabled_equation.relations = Some(crate::feature::FeatureRelationTable {
        declared_count: 1,
        entity_ref: None,
        rows: Vec::new(),
        skamps: vec![crate::feature::FeatureSkamp {
            id: 900,
            kind: 0,
            flags: 0,
            status: 0,
            items: Vec::new(),
            offset: 900,
        }],
        skamp_header: Some(crate::feature::FeatureSolverTableHeader {
            declared_count: 1,
            entity_ref: 901,
            offset: 900,
        }),
        triples: vec![crate::feature::FeatureRelationTriple {
            relation_id: None,
            equation_id: Some(equation_id),
            skamp_id: Some(900),
            offset: 902,
        }],
        triples_header: Some(crate::feature::FeatureSolverTableHeader {
            declared_count: 1,
            entity_ref: 903,
            offset: 902,
        }),
        offset: 899,
    });
    assert!(!resolved_section_coordinates(&disabled_equation).contains_key(&2));

    let mut mismatched = definition;
    mismatched
        .dimensions
        .as_mut()
        .expect("dimension table")
        .rows[0]
        .value = Some(6.0);
    assert!(!resolved_section_coordinates(&mismatched).contains_key(&2));
}

#[test]
fn equation_function_thirteen_transfers_zero_auxiliary_same_coordinate() {
    let row = |variable_type, key, value| crate::feature::FeatureVariableRow {
        variable_type,
        key,
        value,
        value_body: Vec::new(),
        guess: value,
        guess_body: Vec::new(),
        guess_dimension_driven: false,
        known: Some(0),
        homogeneity: Some(1),
        uvar_id: None,
        dimension_driven: false,
        offset: 0,
    };
    let line = |external_id, point_ids| crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Line,
        directions: [None; 3],
        point_ids,
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id,
        body: Vec::new(),
        offset: 0,
    };
    let definition = crate::feature::FeatureDefinition {
        id: 40,
        owner_feature_id: None,
        body: b"eqtn_arr\0\xf2\xf8\x02\xf7\x80\x9f\xfb\xe2\
                \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2\
                \x01\x0d\xf8\x03\x00\x01\x02\xf6\xe2"
            .to_vec(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(crate::feature::FeatureVariableTable {
            declared_count: 3,
            entity_ref: None,
            rows: vec![row(2, 1, Some(4.5)), row(2, 2, None), row(7, 3, Some(0.0))],
            points: Vec::new(),
            offset: 0,
        }),
        segments: Some(crate::feature::FeatureSegmentTable {
            declared_count: 1,
            has_elided_prototype: false,
            entity_ref: None,
            rows: vec![line(10, [1, 2])],
            circle_rows: Vec::new(),
            point_rows: Vec::new(),
            centered_line_rows: Vec::new(),
            reference_line_rows: Vec::new(),
            bounded_curve_rows: Vec::new(),
            conic_rows: Vec::new(),
            opaque_rows: Vec::new(),
            offset: 0,
        }),
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 0,
    };

    assert_eq!(
        resolved_section_coordinates(&definition).get(&2),
        Some(&[None, Some(4.5)])
    );
    let sketch = cadmpeg_ir::sketches::SketchId("creo:model:sketch#40".into());
    let constraints = section_equation_same_coordinate_constraints(&definition, &sketch);
    assert_eq!(constraints.len(), 1);
    assert_eq!(constraints[0].0.active, Some(true));
    assert_eq!(
        constraints[0].0.definition,
        SketchConstraintDefinition::SameCoordinate {
            first: SketchLocus::Start(SketchEntityId("creo:featdefs:sketch_entity#40:10".into(),)),
            second: SketchLocus::End(SketchEntityId("creo:featdefs:sketch_entity#40:10".into(),)),
            axis: cadmpeg_ir::sketches::SketchCoordinateAxis::V,
        }
    );

    let mut function_two = definition.clone();
    function_two.body = b"eqtn_arr\0\xf2\xf8\x03\xf7\x80\x9f\xfb\xe2\
            \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2\
            \x01\x02\x00\x01\xf6\xe2\
            \x02\x0d\xf8\x03\x02\x03\x04\xf6\xe2"
        .to_vec();
    function_two.variables.as_mut().expect("variables").rows = vec![
        row(1, 1, None),
        row(1, 2, None),
        row(2, 1, Some(4.5)),
        row(2, 2, None),
        row(7, 3, Some(0.0)),
    ];
    function_two
        .variables
        .as_mut()
        .expect("variables")
        .declared_count = 5;
    let function_two_constraints =
        section_equation_same_coordinate_constraints(&function_two, &sketch);
    assert_eq!(function_two_constraints.len(), 2);
    assert_eq!(
        function_two_constraints[0].0.definition,
        SketchConstraintDefinition::SameCoordinate {
            first: SketchLocus::Start(SketchEntityId("creo:featdefs:sketch_entity#40:10".into(),)),
            second: SketchLocus::End(SketchEntityId("creo:featdefs:sketch_entity#40:10".into(),)),
            axis: cadmpeg_ir::sketches::SketchCoordinateAxis::U,
        }
    );

    let mut nonzero_auxiliary = definition;
    nonzero_auxiliary
        .variables
        .as_mut()
        .expect("variables")
        .rows[2]
        .value = Some(1.0);
    assert_eq!(
        resolved_section_coordinates(&nonzero_auxiliary).get(&2),
        None
    );
}

#[test]
fn equation_function_thirty_three_solves_unique_equal_line_length_coordinate() {
    let row = |variable_type, key, value| crate::feature::FeatureVariableRow {
        variable_type,
        key,
        value,
        value_body: Vec::new(),
        guess: value,
        guess_body: Vec::new(),
        guess_dimension_driven: false,
        known: Some(0),
        homogeneity: Some(1),
        uvar_id: None,
        dimension_driven: false,
        offset: 0,
    };
    let definition = crate::feature::FeatureDefinition {
        id: 40,
        owner_feature_id: None,
        body: b"eqtn_arr\0\xf2\xf8\x02\xf7\x80\x9f\xfb\xe2\
                \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2\
                \x01\x21\xf8\x09\x00\x01\x02\x03\x04\x05\x06\x07\x08\xf6\xe2"
            .to_vec(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(crate::feature::FeatureVariableTable {
            declared_count: 9,
            entity_ref: None,
            rows: vec![
                row(1, 1, Some(0.0)),
                row(2, 1, Some(0.0)),
                row(1, 2, Some(0.0)),
                row(2, 2, Some(4.0)),
                row(1, 3, Some(0.0)),
                row(2, 3, Some(0.0)),
                row(1, 4, None),
                row(2, 4, Some(4.0)),
                row(7, 5, Some(0.0)),
            ],
            points: Vec::new(),
            offset: 0,
        }),
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 0,
    };

    assert_eq!(
        resolved_section_coordinates(&definition).get(&4),
        Some(&[Some(0.0), Some(4.0)])
    );

    let mut ambiguous = definition.clone();
    ambiguous.variables.as_mut().expect("variables").rows[7].value = Some(0.0);
    assert_eq!(
        resolved_section_coordinates(&ambiguous).get(&4),
        Some(&[None, Some(0.0)])
    );

    let mut nonzero_auxiliary = definition;
    nonzero_auxiliary
        .variables
        .as_mut()
        .expect("variables")
        .rows[8]
        .value = Some(1.0);
    assert_eq!(
        resolved_section_coordinates(&nonzero_auxiliary).get(&4),
        Some(&[None, Some(4.0)])
    );
}

#[test]
fn equation_function_thirty_five_solves_point_on_reference_line() {
    let row = |variable_type, key, value| crate::feature::FeatureVariableRow {
        variable_type,
        key,
        value,
        value_body: Vec::new(),
        guess: value,
        guess_body: Vec::new(),
        guess_dimension_driven: false,
        known: Some(0),
        homogeneity: Some(1),
        uvar_id: None,
        dimension_driven: false,
        offset: 0,
    };
    let definition = crate::feature::FeatureDefinition {
        id: 40,
        owner_feature_id: None,
        body: b"eqtn_arr\0\xf2\xf8\x02\xf7\x80\x9f\xfb\xe2\
                \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2\
                \x01\x23\xf8\x09\x00\x01\x02\x03\x04\x05\x06\x07\x08\xf6\xe2"
            .to_vec(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(crate::feature::FeatureVariableTable {
            declared_count: 9,
            entity_ref: None,
            rows: vec![
                row(1, 20, None),
                row(2, 20, Some(165.0)),
                row(1, 18, Some(0.0)),
                row(2, 18, Some(0.0)),
                row(1, 19, Some(0.0)),
                row(2, 19, Some(-100.0)),
                row(4, 2, None),
                row(5, 0, Some(0.0)),
                row(5, 1, Some(0.0)),
            ],
            points: Vec::new(),
            offset: 0,
        }),
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 0,
    };

    assert_eq!(
        resolved_section_coordinates(&definition).get(&20),
        Some(&[Some(0.0), Some(165.0)])
    );
}

#[test]
fn section_line_requires_two_solved_points() {
    let segment = crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Line,
        directions: [None; 3],
        point_ids: [7, 9],
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 12,
        body: Vec::new(),
        offset: 40,
    };
    let mut points = BTreeMap::from([(7, [2.0, 3.0])]);
    assert!(section_line_geometry(&points, &segment).is_none());
    points.insert(9, [5.0, 8.0]);
    assert_eq!(
        section_line_geometry(&points, &segment),
        Some(SketchGeometry::Line {
            start: cadmpeg_ir::math::Point2::new(2.0, 3.0),
            end: cadmpeg_ir::math::Point2::new(5.0, 8.0),
        })
    );
    points.insert(9, [2.0, 3.0]);
    assert!(section_line_geometry(&points, &segment).is_none());
    points.insert(9, [2.0 + 1e-13, 3.0]);
    assert!(section_line_geometry(&points, &segment).is_none());
    points.insert(9, [2.0 + 1.0e-10, 3.0]);
    assert!(section_line_geometry(&points, &segment).is_some());
}

#[test]
fn sketch_constraints_require_every_neutral_reference_to_be_emitted() {
    let first = SketchEntityId("first".to_string());
    let second = SketchEntityId("second".to_string());
    let emitted = BTreeSet::from([first.clone()]);

    let mut horizontal = SketchConstraintDefinition::Horizontal {
        entity: first.clone(),
    };
    assert!(reconcile_constraint_entity_references(
        &mut horizontal,
        &emitted
    ));
    let mut parallel = SketchConstraintDefinition::Parallel {
        first: first.clone(),
        second: second.clone(),
    };
    assert!(!reconcile_constraint_entity_references(
        &mut parallel,
        &emitted
    ));
    let mut distance = SketchConstraintDefinition::DistanceLoci {
        first: SketchLocus::Start(first.clone()),
        second: SketchLocus::Center(second.clone()),
        parameter: ParameterId("distance".to_string()),
    };
    assert!(!reconcile_constraint_entity_references(
        &mut distance,
        &emitted
    ));
    let mut native = SketchConstraintDefinition::Native {
        native_kind: "creo:test".to_string(),
        entities: vec![first.clone(), second],
        parameter: None,
        operands: Vec::new(),
        native_state: None,
        native_flags: None,
        native_properties: std::collections::BTreeMap::new(),
    };
    assert!(reconcile_constraint_entity_references(
        &mut native,
        &emitted
    ));
    assert!(matches!(
        native,
        SketchConstraintDefinition::Native { entities, .. }
            if entities == vec![first]
    ));

    let parameter = ParameterId("distance".to_string());
    let parameters = BTreeSet::from([parameter.clone()]);
    let mut radius = SketchConstraintDefinition::Radius {
        entity: SketchEntityId("first".to_string()),
        parameter: parameter.clone(),
    };
    assert!(reconcile_constraint_parameter_reference(
        &mut radius,
        &parameters
    ));
    let mut missing_distance = SketchConstraintDefinition::Distance {
        entities: Vec::new(),
        parameter: ParameterId("missing".to_string()),
    };
    assert!(!reconcile_constraint_parameter_reference(
        &mut missing_distance,
        &parameters
    ));
    let mut native_parameter = SketchConstraintDefinition::Native {
        native_kind: "creo:test".to_string(),
        entities: Vec::new(),
        parameter: Some(ParameterId("missing".to_string())),
        operands: Vec::new(),
        native_state: None,
        native_flags: None,
        native_properties: std::collections::BTreeMap::new(),
    };
    assert!(reconcile_constraint_parameter_reference(
        &mut native_parameter,
        &parameters
    ));
    assert!(matches!(
        native_parameter,
        SketchConstraintDefinition::Native {
            parameter: None,
            ..
        }
    ));
}

#[test]
fn section_point_uses_its_single_solved_position() {
    let segment = crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Point,
        directions: [None; 3],
        point_ids: [7, 7],
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 4,
        body: Vec::new(),
        offset: 40,
    };
    let points = BTreeMap::from([(7, [2.0, 3.0])]);

    assert_eq!(
        section_point_geometry(&points, &segment),
        Some(SketchGeometry::Point {
            position: cadmpeg_ir::math::Point2::new(2.0, 3.0),
        })
    );
}

#[test]
fn section_axis_line_carrier_uses_equal_decoded_ordinates() {
    let segment = crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Line,
        directions: [Some(0), None, Some(0)],
        point_ids: [7, 9],
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 12,
        body: Vec::new(),
        offset: 40,
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
            points: vec![
                crate::feature::FeatureSectionPoint {
                    point_id: 7,
                    u: Some(2.0),
                    v: None,
                },
                crate::feature::FeatureSectionPoint {
                    point_id: 9,
                    u: Some(2.0),
                    v: Some(8.0),
                },
            ],
            offset: 0,
        }),
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 0,
    };
    assert_eq!(
        section_axis_line_carrier(&definition, &segment),
        Some(SketchGeometry::ReferenceLine {
            origin: cadmpeg_ir::math::Point2::new(2.0, 0.0),
            direction: cadmpeg_ir::math::Point2::new(0.0, 1.0),
        })
    );
    assert_eq!(
        section_axis_reference_line_geometry(
            &definition,
            &resolved_section_coordinates(&definition),
            &segment,
        ),
        Some(SketchGeometry::ReferenceLine {
            origin: cadmpeg_ir::math::Point2::new(2.0, 0.0),
            direction: cadmpeg_ir::math::Point2::new(0.0, 1.0),
        })
    );
    assert_eq!(
        section_axis_reference_line_geometry(
            &definition,
            &BTreeMap::from([(7, [Some(2.0), None]), (9, [None, Some(8.0)]),]),
            &segment,
        ),
        None
    );
    let mut selector_segment = segment.clone();
    selector_segment.directions = [None; 3];
    selector_segment.vertical_horizontal = Some(0);
    let mut selector_definition = definition.clone();
    selector_definition.segments = Some(crate::feature::FeatureSegmentTable {
        declared_count: 1,
        has_elided_prototype: false,
        entity_ref: None,
        rows: vec![selector_segment.clone()],
        circle_rows: Vec::new(),
        point_rows: Vec::new(),
        centered_line_rows: Vec::new(),
        reference_line_rows: Vec::new(),
        bounded_curve_rows: Vec::new(),
        conic_rows: Vec::new(),
        opaque_rows: Vec::new(),
        offset: 0,
    });
    assert_eq!(
        section_axis_reference_line_geometry(
            &selector_definition,
            &BTreeMap::from([(7, [Some(2.0), None]), (9, [Some(2.0), Some(8.0)])]),
            &selector_segment,
        ),
        Some(SketchGeometry::ReferenceLine {
            origin: cadmpeg_ir::math::Point2::new(2.0, 0.0),
            direction: cadmpeg_ir::math::Point2::new(0.0, 1.0),
        })
    );
    assert_eq!(
        unique_owned_feature_definition(std::slice::from_ref(&definition), 6)
            .map(|matched| matched.id),
        Some(5)
    );
    assert!(
        unique_owned_feature_definition(&[definition.clone(), definition.clone()], 6).is_none()
    );
    let operation = crate::feature::FeatureOperation {
        feature_id: 6,
        kind: "Extrude".to_string(),
        display_name_stored: true,
        stored_name: Some("Extrude id 6".to_string()),
        stored_name_bytes: Some(b"Extrude id 6".to_vec()),
        identifier_keyword: Some("id".to_string()),
        stored_name_prefix: None,
        recipe: Some(crate::feature::FeatureRecipe::ProtrudeExtrude),
        recipe_conflict: false,
        display_state_conflict: false,
        root_schema_class: Some(917),
        parent_feature_id: None,
        offset: 10,
        state_offset: 10,
    };
    assert_eq!(
        current_feature_operation(std::slice::from_ref(&operation), 6)
            .and_then(|current| current.root_schema_class),
        Some(917)
    );
    assert!(current_feature_operation(&[operation.clone(), operation.clone()], 6).is_none());
    assert_eq!(
        current_feature_recipe(std::slice::from_ref(&operation), 6),
        Some(crate::feature::FeatureRecipe::ProtrudeExtrude)
    );
    let mut conflicting_recipe = operation.clone();
    conflicting_recipe.recipe = Some(crate::feature::FeatureRecipe::ProtrudeRevolve);
    assert_eq!(
        current_feature_recipe(&[operation.clone(), conflicting_recipe], 6),
        None
    );
    let mut parented_operation = operation.clone();
    parented_operation.parent_feature_id = Some(5);
    assert_eq!(
        current_feature_recipe_parent(std::slice::from_ref(&parented_operation), 6),
        Some(5)
    );
    let mut conflicting_parent = parented_operation.clone();
    conflicting_parent.parent_feature_id = Some(4);
    assert_eq!(
        current_feature_recipe_parent(&[parented_operation, conflicting_parent], 6),
        None
    );
    let row = |schema_class, offset| crate::feature::FeatureRow {
        feature_id: 6,
        header: [0xeb, 0x04],
        root_schema_class: Some(schema_class),
        stream_offset: 0,
        body: Vec::new(),
        body_offset: offset + 1,
        offset,
    };
    assert_eq!(
        resolved_feature_schema_class_from_classes(
            &[],
            row_feature_schema_classes(&[row(917, 20), row(917, 30)], 6),
            6,
        ),
        Some(917)
    );
    assert_eq!(
        resolved_feature_schema_class_from_classes(
            &[],
            row_feature_schema_classes(&[row(913, 20), row(914, 30)], 6),
            6,
        ),
        None
    );
    assert_eq!(
        resolved_feature_schema_class_from_classes(
            std::slice::from_ref(&operation),
            row_feature_schema_classes(&[row(913, 20), row(914, 30)], 6),
            6,
        ),
        Some(917)
    );
    assert_eq!(
        resolved_feature_schema_class_from_classes(
            std::slice::from_ref(&operation),
            row_feature_schema_classes(&[row(913, 20), row(913, 30)], 6),
            6,
        ),
        Some(917)
    );
    assert_eq!(
        row_feature_schema_classes(&[row(913, 20), row(914, 30)], 6),
        BTreeSet::from([913, 914])
    );
    let extent = |feature_id, offset| crate::feature::FeatureRevolutionExtent {
        feature_id,
        kind: crate::feature::FeatureRevolutionExtentKind::FullTurn,
        offset,
    };
    assert_eq!(
        unique_feature_revolution_extent_kind(&[extent(6, 40), extent(6, 50)], 6),
        Some(crate::feature::FeatureRevolutionExtentKind::FullTurn)
    );
    assert_eq!(
        unique_feature_revolution_extent_kind(&[extent(7, 40)], 6),
        None
    );
    let transform = crate::placement::FeatureSectionTransform {
        definition_id: 5,
        feature_id: Some(6),
        origin: [0.0; 3],
        u_axis: [1.0, 0.0, 0.0],
        v_axis: [0.0, 1.0, 0.0],
        normal: [0.0, 0.0, 1.0],
        offset: 40,
    };
    assert_eq!(
        unique_feature_section_transform(std::slice::from_ref(&transform), 5, 40)
            .map(|placed| placed.offset),
        Some(40)
    );
    assert!(
        unique_feature_section_transform(&[transform.clone(), transform.clone()], 5, 40).is_none()
    );
    let repeated_schema = crate::placement::FeatureSectionTransform {
        feature_id: Some(7),
        offset: 50,
        ..transform.clone()
    };
    assert_eq!(
        unique_feature_section_transform(&[transform.clone(), repeated_schema], 5, 40)
            .map(|placed| placed.offset),
        Some(40)
    );
    let competing_definition = crate::placement::FeatureSectionTransform {
        definition_id: 7,
        offset: 50,
        ..transform.clone()
    };
    assert!(unique_feature_section_transform(&[transform, competing_definition], 5, 40).is_none());
    let affected = |ids: &[u32], offset| crate::feature::FeatureAffectedIds {
        feature_id: 6,
        kind: crate::feature::AffectedIdKind::Edges,
        ids: ids.to_vec(),
        offset,
    };
    assert_eq!(
        agreed_feature_affected_ids(
            &[affected(&[7, 8], 60), affected(&[7, 8], 70)],
            6,
            crate::feature::AffectedIdKind::Edges,
        ),
        Some(&[7, 8][..])
    );
    assert_eq!(
        agreed_feature_affected_ids(
            &[affected(&[7, 8], 60), affected(&[8, 7], 70)],
            6,
            crate::feature::AffectedIdKind::Edges,
        ),
        None
    );
    let replay =
        |geometry_ids: &[u32], edge_ids: &[u32], offset| crate::feature::FeatureReplayAffectedIds {
            feature_id: 6,
            geometry_ids: geometry_ids.to_vec(),
            edge_ids: edge_ids.to_vec(),
            geometry_extent: crate::feature::ReplayExtentSource::Explicit,
            edge_extent: crate::feature::ReplayExtentSource::Inherited,
            offset,
        };
    let geometry = |ids: &[u32], offset| crate::feature::FeatureAffectedIds {
        feature_id: 6,
        kind: crate::feature::AffectedIdKind::Geometry,
        ids: ids.to_vec(),
        offset,
    };
    let replay_geometry = replay(&[9], &[7], 80);
    assert_eq!(
        agreed_feature_geometry_ids(&[], std::slice::from_ref(&replay_geometry), 6),
        Some(&[9][..])
    );
    let named_empty = geometry(&[], 60);
    assert_eq!(
        agreed_feature_geometry_ids(
            std::slice::from_ref(&named_empty),
            std::slice::from_ref(&replay_geometry),
            6,
        ),
        Some(&[][..])
    );
    let conflicting_named = [geometry(&[7], 60), geometry(&[8], 70)];
    assert_eq!(
        agreed_feature_geometry_ids(
            &conflicting_named,
            std::slice::from_ref(&replay_geometry),
            6,
        ),
        None
    );
    assert_eq!(
        agreed_feature_replay_geometry_ids(
            &[replay(&[1, 2], &[7], 80), replay(&[1, 2], &[7], 90)],
            6,
        ),
        Some(&[1, 2][..])
    );
    assert_eq!(
        agreed_feature_replay_edge_ids(&[replay(&[1], &[7], 80), replay(&[1], &[], 90)], 6,),
        None
    );
}

#[test]
fn material_base_body_uses_bounded_definition_order() {
    assert!(first_material_feature_by_definition_order(
        10,
        &[(10, 100), (20, 200)]
    ));
    assert!(!first_material_feature_by_definition_order(
        20,
        &[(10, 100), (20, 200)]
    ));
    assert!(!first_material_feature_by_definition_order(
        10,
        &[(10, 100), (10, 100), (20, 200)]
    ));
    assert!(!first_material_feature_by_definition_order(
        10,
        &[(10, 100), (20, 100)]
    ));
    assert!(!first_material_feature_by_definition_order(
        10,
        &[(20, 200)]
    ));
}

#[test]
fn unresolved_material_join_does_not_hide_exact_base_body_candidate() {
    let operation = |feature_id, root_schema_class, recipe| crate::feature::FeatureOperation {
        feature_id,
        kind: "Sweep".to_string(),
        display_name_stored: false,
        stored_name: None,
        stored_name_bytes: None,
        identifier_keyword: None,
        stored_name_prefix: None,
        recipe,
        recipe_conflict: false,
        display_state_conflict: false,
        root_schema_class,
        parent_feature_id: None,
        offset: feature_id as usize,
        state_offset: feature_id as usize,
    };
    let definition = |id, section_offset, offset| crate::feature::FeatureDefinition {
        id,
        owner_feature_id: Some(id),
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: Some(crate::feature::FeatureSection3d {
            sketch_plane_entity_id: None,
            sketch_plane_flip: None,
            reference_plane_entity_ids: Vec::new(),
            reference_plane_rows: Vec::new(),
            reference_plane_datum_geometry_id: None,
            orientation: crate::feature::FeatureSectionOrientation::default(),
            dimension_ids: Vec::new(),
            offset: section_offset,
        }),
        dimensions: None,
        relations: None,
        saved_section: None,
        offset,
    };
    let transform = |definition_id, feature_id, offset| crate::placement::FeatureSectionTransform {
        definition_id,
        feature_id: Some(feature_id),
        origin: [0.0; 3],
        u_axis: [1.0, 0.0, 0.0],
        v_axis: [0.0, 1.0, 0.0],
        normal: [0.0, 0.0, 1.0],
        offset,
    };

    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features.operations.extend([
        operation(
            10,
            Some(917),
            Some(crate::feature::FeatureRecipe::ProtrudeExtrude),
        ),
        operation(20, Some(917), None),
        operation(30, Some(917), None),
    ]);
    scan.features.definitions.push(definition(10, 101, 100));
    scan.features
        .section_transforms
        .extend([transform(10, 10, 101), transform(30, 30, 302)]);

    assert!(feature_is_first_material_operation(&scan, 10));
    assert!(!feature_is_first_material_operation(&scan, 20));
    assert!(!feature_is_first_material_operation(&scan, 30));
}

#[test]
fn intersects_evaluated_section_carriers() {
    let horizontal = SketchGeometry::Line {
        start: cadmpeg_ir::math::Point2::new(-2.0, 1.0),
        end: cadmpeg_ir::math::Point2::new(2.0, 1.0),
    };
    let vertical = SketchGeometry::Line {
        start: cadmpeg_ir::math::Point2::new(0.5, -3.0),
        end: cadmpeg_ir::math::Point2::new(0.5, 3.0),
    };
    assert_eq!(
        intersect_section_lines(&horizontal, &vertical),
        Some([0.5, 1.0])
    );
    let vertical_reference = SketchGeometry::ReferenceLine {
        origin: cadmpeg_ir::math::Point2::new(0.5, 0.0),
        direction: cadmpeg_ir::math::Point2::new(0.0, 1.0),
    };
    assert_eq!(
        intersect_section_lines(&horizontal, &vertical_reference),
        Some([0.5, 1.0])
    );

    let circle_half = SketchGeometry::Arc {
        center: cadmpeg_ir::math::Point2::new(0.0, 0.0),
        radius: Length(2.0),
        start_angle: Angle(0.0),
        end_angle: Angle(std::f64::consts::PI),
    };
    let endpoint_line = SketchGeometry::Line {
        start: cadmpeg_ir::math::Point2::new(2.0, 0.0),
        end: cadmpeg_ir::math::Point2::new(3.0, 1.0),
    };
    let intersection = intersect_section_line_arc(&endpoint_line, &circle_half)
        .expect("line has one endpoint on the arc");
    assert!((intersection[0] - 2.0).abs() <= 1.0e-12);
    assert!(intersection[1].abs() <= 1.0e-12);
    let one_crossing = SketchGeometry::Line {
        start: cadmpeg_ir::math::Point2::new(0.0, 0.0),
        end: cadmpeg_ir::math::Point2::new(3.0, 0.0),
    };
    assert_eq!(
        intersect_section_line_arc(&one_crossing, &circle_half),
        Some([2.0, 0.0])
    );
    let two_crossings = SketchGeometry::Line {
        start: cadmpeg_ir::math::Point2::new(-3.0, 0.0),
        end: cadmpeg_ir::math::Point2::new(3.0, 0.0),
    };
    assert_eq!(
        intersect_section_line_arc(&two_crossings, &circle_half),
        None
    );
    let no_crossing = SketchGeometry::Line {
        start: cadmpeg_ir::math::Point2::new(3.0, 0.0),
        end: cadmpeg_ir::math::Point2::new(4.0, 0.0),
    };
    assert_eq!(intersect_section_line_arc(&no_crossing, &circle_half), None);

    let circle = |center, radius| SketchGeometry::Arc {
        center: cadmpeg_ir::math::Point2::new(center, 0.0),
        radius: Length(radius),
        start_angle: Angle(0.0),
        end_angle: Angle(std::f64::consts::TAU),
    };
    assert_eq!(
        intersect_tangent_section_arcs(&circle(0.0, 2.0), &circle(3.0, 1.0)),
        Some([2.0, 0.0])
    );
    assert_eq!(
        intersect_tangent_section_arcs(&circle(0.0, 3.0), &circle(2.0, 1.0)),
        Some([3.0, 0.0])
    );
    assert_eq!(
        intersect_tangent_section_arcs(&circle(0.0, 2.0), &circle(2.0, 2.0)),
        None
    );
}
