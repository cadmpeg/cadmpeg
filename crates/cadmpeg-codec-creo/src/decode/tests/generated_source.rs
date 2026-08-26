// SPDX-License-Identifier: Apache-2.0
//! Tests: generated source.

use super::{
    class_911_surface_row, simple_drilled_recipe_surface_rows, simple_drilled_recipe_table,
};
use crate::decode::analytic::rowless_round_face_orientations;
use crate::decode::coverage::{
    constraint_kind_breakdown, curve_transfer_coverage, design_constraint_transfer_coverage,
    surface_transfer_coverage,
};
use crate::decode::feature_history::{
    analytic_surface_id_for_feature, generated_surface_id_for_feature,
    ordered_analytic_surface_id_for_feature, ordered_family_surface_bindings_for_feature,
    schema_feature_definition, section_entity_is_generated_profile,
    section_generated_profile_surface_kinds,
};
use crate::decode::holes::{
    clipped_drilled_hole_placement_from_cone_points, counterbore_axis_placement_from_sources,
    counterbore_cylinder_sources, counterbore_dimension_values, counterbore_directed_span,
    counterbore_envelope_dimension_values, counterbore_placement_from_corner_envelopes,
    counterbore_source_patch_geometries, counterbore_support_axis_placement,
    counterbore_unenveloped_dimension_values, dimension_pair_matches_envelope_spans,
    drilled_hole_placement_from_corner_envelopes, paired_corner_envelope_axis_spans,
    simple_drilled_axis_placement_from_frames, simple_drilled_hole_dimension_values,
    simple_drilled_hole_recipe, stepped_hole_form, ExtrusionSpan, SimpleDrilledDimensionFamily,
};
use crate::decode::sketch::approximately_equal;
use crate::decode::sketch_transfer::{
    normalize_section_incidence_curve_family_evidence, sketch_constraint_loci_compatible,
    SectionEntityIncidenceFamily,
};
use crate::decode::surfaces::rowless_round_cylinder_pairs;
use crate::decode::sweep::{
    circular_pcurve, extruded_nurbs_surface, extrusion_cap_pcurve, extrusion_profile_signed_area,
    extrusion_side_uvs, ordered_extrusion_profiles, oriented_arc_parameterization,
    oriented_full_turn_angles, point_on_profile_arc, profile_arc, resolved_sketch_profiles,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    Angle, FeatureDefinition as IrFeatureDefinition, HoleForm, HoleKind, Length, Termination,
};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, ProceduralSurface, ProceduralSurfaceDefinition, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralSurfaceId, SurfaceId};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchEntity,
    SketchEntityId, SketchEntityUse, SketchGeometry, SketchId, SketchLocus,
};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::SourceObjectAssociation;
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn generated_source_ids_bind_carriers_independently_of_table_position() {
    let table = crate::feature::FeatureEntityTable {
        feature_id: Some(17),
        table_class_id: 80,
        entry_ids: vec![42, 41, 43],
        entries: vec![
            crate::feature::FeatureEntityTableEntry {
                entity_id: 42,
                class_id: 200,
                source_entity_id: Some(10),
                related_entity_id: None,
                related_entity_state: None,
                prefixed: false,
                offset: 0,
                end_offset: 0,
            },
            crate::feature::FeatureEntityTableEntry {
                entity_id: 41,
                class_id: 200,
                source_entity_id: Some(8),
                related_entity_id: None,
                related_entity_state: None,
                prefixed: false,
                offset: 0,
                end_offset: 0,
            },
            crate::feature::FeatureEntityTableEntry {
                entity_id: 43,
                class_id: 200,
                source_entity_id: Some(9),
                related_entity_id: None,
                related_entity_state: None,
                prefixed: false,
                offset: 0,
                end_offset: 0,
            },
        ],
        surface_ids: vec![41, 42, 43],
        non_surface_entity_ids: Vec::new(),
        offset: 0,
    };
    let order = crate::feature::FeatureOrderTable {
        declared_count: 2,
        has_prototype: false,
        entity_ref: Some(3),
        rows: vec![
            crate::feature::FeatureOrderRow {
                external_id: 8,
                internal_id: 1,
                bitmask: 0,
                offset: 0,
            },
            crate::feature::FeatureOrderRow {
                external_id: 9,
                internal_id: 2,
                bitmask: 0,
                offset: 0,
            },
        ],
        offset: 0,
    };
    let row = |id, kind: crate::surface::SurfaceKind| crate::surface::SurfaceRow {
        id,
        type_byte: kind.canonical_type_byte(),
        kind,
        feature_id: 17,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 0,
    };
    let rows = vec![
        row(41, crate::surface::SurfaceKind::Cylinder),
        row(42, crate::surface::SurfaceKind::Cone),
        row(43, crate::surface::SurfaceKind::TorusOrSphere),
    ];
    let cylinder = SurfaceGeometry::Cylinder {
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 2.0,
    };
    let cone = SurfaceGeometry::Cone {
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 1.0,
        ratio: 1.0,
        half_angle: 0.5,
    };
    assert_eq!(
        analytic_surface_id_for_feature(&rows, std::slice::from_ref(&table), 17, 10, &cone,),
        Some(42)
    );
    assert_eq!(
        ordered_analytic_surface_id_for_feature(
            &rows,
            std::slice::from_ref(&table),
            17,
            &order,
            10,
            &cone,
        ),
        None
    );
    assert_eq!(
        analytic_surface_id_for_feature(&rows, std::slice::from_ref(&table), 17, 10, &cylinder,),
        None
    );
    assert_eq!(
        ordered_analytic_surface_id_for_feature(
            &rows,
            std::slice::from_ref(&table),
            17,
            &order,
            8,
            &cylinder,
        ),
        Some(41)
    );
    assert_eq!(
        ordered_analytic_surface_id_for_feature(
            &rows,
            std::slice::from_ref(&table),
            17,
            &order,
            9,
            &cylinder,
        ),
        None
    );
    let mut first_table = table.clone();
    first_table.entry_ids = vec![41];
    first_table.entries = vec![table.entries[1].clone()];
    first_table.surface_ids = vec![41];
    let mut second_table = table.clone();
    second_table.entry_ids = vec![43];
    second_table.entries = vec![table.entries[2].clone()];
    second_table.surface_ids = vec![43];
    assert_eq!(
        generated_surface_id_for_feature(&[first_table.clone(), second_table], 17, 9),
        Some(43)
    );
    first_table.entries[0].source_entity_id = Some(9);
    assert_eq!(
        generated_surface_id_for_feature(&[first_table, table.clone()], 17, 9),
        None
    );
    let mut wrong_class = table.clone();
    wrong_class.entries[2].class_id = 201;
    assert_eq!(
        generated_surface_id_for_feature(&[wrong_class], 17, 9),
        None
    );
    let torus = SurfaceGeometry::Torus {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 1.0, 0.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        major_radius: 4.0,
        minor_radius: 1.0,
    };
    assert_eq!(
        ordered_analytic_surface_id_for_feature(
            &rows,
            std::slice::from_ref(&table),
            17,
            &order,
            9,
            &torus,
        ),
        Some(43)
    );
    assert_eq!(
        ordered_family_surface_bindings_for_feature(
            &rows,
            17,
            std::slice::from_ref(&table),
            &order,
            [9],
            crate::surface::SurfaceKind::TorusOrSphere,
        ),
        BTreeMap::from([(9, 43)])
    );
    assert_eq!(
        section_generated_profile_surface_kinds(&SketchGeometry::Circle {
            center: Point2::new(1.0, 2.0),
            radius: Length(3.0),
        }),
        Some(&[crate::surface::SurfaceKind::Cylinder][..])
    );
    assert!(section_entity_is_generated_profile(
        true,
        Some(17),
        8,
        &[crate::surface::SurfaceKind::Cylinder],
        std::slice::from_ref(&table),
        &rows,
    ));
    let mut extrusion_rows = rows.clone();
    extrusion_rows[2] = row(43, crate::surface::SurfaceKind::Extrusion);
    assert!(section_entity_is_generated_profile(
        true,
        Some(17),
        9,
        &[
            crate::surface::SurfaceKind::Spline,
            crate::surface::SurfaceKind::Extrusion,
        ],
        std::slice::from_ref(&table),
        &extrusion_rows,
    ));
    assert!(!section_entity_is_generated_profile(
        true,
        Some(17),
        9,
        &[crate::surface::SurfaceKind::Spline],
        std::slice::from_ref(&table),
        &extrusion_rows,
    ));
    assert!(!section_entity_is_generated_profile(
        false,
        Some(17),
        9,
        &[
            crate::surface::SurfaceKind::Spline,
            crate::surface::SurfaceKind::Extrusion,
        ],
        std::slice::from_ref(&table),
        &extrusion_rows,
    ));
    assert!(!section_entity_is_generated_profile(
        true,
        Some(17),
        10,
        &[crate::surface::SurfaceKind::Cylinder],
        &[table],
        &rows,
    ));
}

#[test]
fn paired_cylinder_sources_and_planar_support_identify_counterbore_form() {
    let entry = |entity_id, class_id, source_entity_id| crate::feature::FeatureEntityTableEntry {
        entity_id,
        class_id,
        source_entity_id,
        related_entity_id: None,
        related_entity_state: None,
        prefixed: false,
        offset: 0,
        end_offset: 0,
    };
    let entries = vec![
        entry(21, 204, None),
        entry(22, 203, None),
        entry(11, 200, Some(4)),
        entry(13, 200, Some(6)),
        entry(15, 200, Some(7)),
        entry(23, 204, None),
        entry(24, 203, None),
        entry(12, 200, Some(4)),
        entry(14, 200, Some(6)),
        entry(16, 200, Some(7)),
        entry(31, 204, None),
        entry(32, 203, None),
        entry(33, 200, Some(4)),
        entry(34, 200, Some(6)),
        entry(35, 200, Some(7)),
    ];
    let table = crate::feature::FeatureEntityTable {
        feature_id: Some(9),
        table_class_id: 29,
        entry_ids: entries.iter().map(|entry| entry.entity_id).collect(),
        entries,
        surface_ids: vec![11, 12, 13, 15, 16],
        non_surface_entity_ids: vec![21, 22, 23, 24, 14, 31, 32, 33, 34, 35],
        offset: 0,
    };
    let row = |id, kind: crate::surface::SurfaceKind| crate::surface::SurfaceRow {
        id,
        type_byte: kind.canonical_type_byte(),
        kind,
        feature_id: 9,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 0,
    };
    let mut rows = vec![
        row(11, crate::surface::SurfaceKind::Cylinder),
        row(12, crate::surface::SurfaceKind::Cylinder),
        row(13, crate::surface::SurfaceKind::Plane),
        row(15, crate::surface::SurfaceKind::Cylinder),
        row(16, crate::surface::SurfaceKind::Cylinder),
    ];

    assert_eq!(
        stepped_hole_form(9, std::slice::from_ref(&table), &rows),
        Some(HoleForm::Counterbore)
    );
    assert_eq!(
        stepped_hole_form(9, &[table.clone(), table.clone()], &rows),
        None
    );

    rows[4].kind = crate::surface::SurfaceKind::Cone;
    assert_eq!(
        stepped_hole_form(9, std::slice::from_ref(&table), &rows),
        None
    );
}

#[test]
fn paired_cone_and_cylinder_sources_identify_simple_drilled_recipe() {
    let mut table = simple_drilled_recipe_table(9);
    let mut rows = simple_drilled_recipe_surface_rows(9);

    assert_eq!(
        simple_drilled_hole_recipe(9, std::slice::from_ref(&table), &rows)
            .map(|recipe| recipe.dimension_family),
        Some(SimpleDrilledDimensionFamily::ExternalId2Depth)
    );
    assert!(simple_drilled_hole_recipe(9, &[table.clone(), table.clone()], &rows,).is_none());

    let mut extended = table.clone();
    let mut extra = extended.entries[3].clone();
    extra.entity_id = 26;
    extra.source_entity_id = Some(5);
    extended.entry_ids.insert(7, extra.entity_id);
    extended.non_surface_entity_ids.push(extra.entity_id);
    extended.entries.insert(7, extra.clone());
    extra.entity_id = 27;
    extended.entry_ids.insert(14, extra.entity_id);
    extended.non_surface_entity_ids.push(extra.entity_id);
    extended.entries.insert(14, extra);
    assert_eq!(
        simple_drilled_hole_recipe(9, std::slice::from_ref(&extended), &rows)
            .map(|recipe| recipe.dimension_family),
        Some(SimpleDrilledDimensionFamily::ExternalId4Depth)
    );
    let mut unknown_family = extended;
    let mut extra = unknown_family.entries[7].clone();
    extra.entity_id = 28;
    extra.source_entity_id = Some(6);
    unknown_family.entry_ids.insert(8, extra.entity_id);
    unknown_family.non_surface_entity_ids.push(extra.entity_id);
    unknown_family.entries.insert(8, extra.clone());
    extra.entity_id = 29;
    unknown_family.entry_ids.insert(16, extra.entity_id);
    unknown_family.non_surface_entity_ids.push(extra.entity_id);
    unknown_family.entries.insert(16, extra);
    assert!(simple_drilled_hole_recipe(9, std::slice::from_ref(&unknown_family), &rows).is_none());

    let mut bottom = table.entries[2].clone();
    bottom.entity_id = 20;
    bottom.source_entity_id = Some(0);
    table.entry_ids.insert(2, bottom.entity_id);
    table.non_surface_entity_ids.push(bottom.entity_id);
    table.entries.insert(2, bottom.clone());
    assert!(simple_drilled_hole_recipe(9, std::slice::from_ref(&table), &rows).is_some());
    bottom.entity_id = 25;
    table.entry_ids.insert(3, bottom.entity_id);
    table.non_surface_entity_ids.push(bottom.entity_id);
    table.entries.insert(3, bottom);
    assert!(simple_drilled_hole_recipe(9, std::slice::from_ref(&table), &rows).is_none());

    let table = simple_drilled_recipe_table(9);
    rows[1].kind = crate::surface::SurfaceKind::Cylinder;
    assert!(simple_drilled_hole_recipe(9, &[table], &rows).is_none());
}

#[test]
fn simple_drilled_dimensions_require_complete_agreeing_tables() {
    let table = |radius: f64, angle: f64, depth: f64| crate::feature::FeatureDimensionTable {
        declared_count: 3,
        entity_ref: Some(88),
        rows: [
            (2, radius, 0, crate::feature::DimensionUnit::Millimeters),
            (10, angle, 1, crate::feature::DimensionUnit::Radians),
            (2, depth, 2, crate::feature::DimensionUnit::Millimeters),
        ]
        .into_iter()
        .map(
            |(dimension_type, value, external_id, value_unit)| crate::feature::FeatureDimension {
                dimension_type,
                value: Some(value),
                value_body: Vec::new(),
                unresolved_value_token: None,
                value_unit,
                direction_byte: 0,
                auxiliary_value: Some(0.0),
                auxiliary_body: Vec::new(),
                external_id,
                references: None,
                offset: 0,
            },
        )
        .collect(),
        offset: 0,
    };
    let angle = 118.0_f64.to_radians();
    let id2 = SimpleDrilledDimensionFamily::ExternalId2Depth;
    let first = table(4.2, angle, -25.0);
    let second = table(4.2, angle, -25.0);

    assert_eq!(
        simple_drilled_hole_dimension_values([&first, &second].into_iter(), None, id2),
        Some((8.4, angle, 25.0))
    );
    let conflicting = table(5.0, angle, -25.0);
    assert_eq!(
        simple_drilled_hole_dimension_values([&first, &conflicting].into_iter(), None, id2),
        None
    );
    assert_eq!(
        simple_drilled_hole_dimension_values(
            [&first, &conflicting].into_iter(),
            Some([[Some(8.4), None], [Some(25.0), None], [Some(100.0), None],]),
            id2,
        ),
        Some((8.4, angle, 25.0))
    );
    assert_eq!(
        simple_drilled_hole_dimension_values(
            [&first, &conflicting].into_iter(),
            Some([[Some(12.0), None], [Some(30.0), None], [Some(100.0), None],]),
            id2,
        ),
        None
    );
    let mut other_layout = table(5.0, angle, -30.0);
    other_layout.rows[2].external_id = 4;
    assert_eq!(
        simple_drilled_hole_dimension_values([&first, &other_layout].into_iter(), None, id2),
        Some((8.4, angle, 25.0))
    );
    assert_eq!(
        simple_drilled_hole_dimension_values(
            [&first, &other_layout].into_iter(),
            None,
            SimpleDrilledDimensionFamily::ExternalId4Depth,
        ),
        Some((10.0, angle, 30.0))
    );
    assert_eq!(
        simple_drilled_hole_dimension_values(
            [&first, &other_layout].into_iter(),
            Some([[Some(8.4), None], [Some(25.0), None], [Some(100.0), None],]),
            id2,
        ),
        Some((8.4, angle, 25.0))
    );
    let invalid_angle = table(4.2, std::f64::consts::PI, -25.0);
    assert_eq!(
        simple_drilled_hole_dimension_values([&invalid_angle].into_iter(), None, id2),
        None
    );
    assert_eq!(
        simple_drilled_hole_dimension_values(
            [&first, &invalid_angle].into_iter(),
            Some([[Some(8.4), None], [Some(25.0), None], [Some(100.0), None],]),
            id2,
        ),
        None
    );
    let invalid_other_angle = table(5.0, std::f64::consts::PI, -25.0);
    assert_eq!(
        simple_drilled_hole_dimension_values(
            [&first, &invalid_other_angle].into_iter(),
            Some([[Some(8.4), None], [Some(25.0), None], [Some(100.0), None],]),
            id2,
        ),
        Some((8.4, angle, 25.0))
    );
    let adjacent_diameter = table(0.125, angle, -0.5);
    assert_eq!(
        simple_drilled_hole_dimension_values(
            [&adjacent_diameter].into_iter(),
            Some([[Some(6.375), None], [Some(0.5), None], [None, Some(0.25)],]),
            id2,
        ),
        Some((0.25, angle, 0.5))
    );
}

#[test]
fn paired_corner_envelopes_expose_dimension_candidate_spans() {
    assert_eq!(
        paired_corner_envelope_axis_spans(
            [[0.0, 0.0, -10.0], [25.0, 8.38, 100.0]],
            [[0.0, 0.0, 100.0], [25.0, 8.38, 180.0]],
        ),
        Some([[Some(25.0), None], [Some(8.38), None], [None, Some(190.0)],])
    );
    assert_eq!(
        paired_corner_envelope_axis_spans(
            [[0.0, 0.0, 0.0], [6.375, 0.5, 0.125]],
            [[0.0, 0.0, 0.125], [6.375, 0.5, 0.25]],
        ),
        Some([[Some(6.375), None], [Some(0.5), None], [None, Some(0.25)],])
    );
    assert_eq!(
        paired_corner_envelope_axis_spans(
            [[1.9375, 0.75, 0.6875], [2.5625, 1.25, 0.0]],
            [[1.9375, 0.75, 0.0], [2.5625, 1.25, 1.3125]],
        ),
        Some([[Some(0.625), None], [Some(0.5), None], [None, Some(0.625)],])
    );
    assert_eq!(
        paired_corner_envelope_axis_spans(
            [[0.0, -15.0, 0.0], [10.0, 20.0, 30.0]],
            [[0.0, 20.0, 0.0], [10.0, -25.0, 30.0]],
        ),
        Some([[Some(10.0), None], [None, Some(10.0)], [Some(30.0), None],])
    );
    assert_eq!(
        paired_corner_envelope_axis_spans(
            [[0.0, 0.0, 0.0], [1.0, 2.0, 3.0]],
            [[0.0, 4.0, 0.0], [1.0, 6.0, 3.0]],
        ),
        Some([[Some(1.0), None], [None, None], [Some(3.0), None],])
    );
    assert_eq!(
        paired_corner_envelope_axis_spans(
            [[0.0, 0.0, 0.0], [0.0, 2.0, 3.0]],
            [[0.0, 0.0, 0.0], [0.0, 2.0, 3.0]],
        ),
        Some([[None, None], [Some(2.0), None], [Some(3.0), None],])
    );
    assert!(!dimension_pair_matches_envelope_spans(
        4.0,
        5.0,
        [[Some(4.0), Some(5.0)], [Some(2.0), None], [Some(3.0), None]],
    ));
}

#[test]
fn complementary_drilled_hole_envelopes_define_axis_placement() {
    let corners = [
        [[-10.0, 0.0, -10.0], [10.0, 45.0, 0.0]],
        [[-10.0, 0.0, 0.0], [10.0, 45.0, 10.0]],
    ];
    assert_eq!(
        drilled_hole_placement_from_corner_envelopes(corners, 20.0, 45.0),
        Some((Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 1.0, 0.0)))
    );

    let reversed = corners.map(|[first, second]| [second, first]);
    assert_eq!(
        drilled_hole_placement_from_corner_envelopes(reversed, 20.0, 45.0),
        Some((Point3::new(0.0, 45.0, 0.0), Vector3::new(0.0, -1.0, 0.0)))
    );
    assert!(drilled_hole_placement_from_corner_envelopes(corners, 10.0, 45.0).is_none());
    let mut opposed = corners;
    opposed[1].reverse();
    assert!(drilled_hole_placement_from_corner_envelopes(opposed, 20.0, 45.0).is_none());
    let mut diagonal_quadrants = corners;
    diagonal_quadrants[1][0][0] = 0.0;
    assert!(drilled_hole_placement_from_corner_envelopes(diagonal_quadrants, 20.0, 45.0).is_none());
}

#[test]
fn one_sided_drilled_hole_envelopes_define_the_missing_radial_coordinate() {
    let corners = [
        [[-5.0, -15.0, -30.0], [5.0, 20.0, 0.0]],
        [[-5.0, -25.0, -30.0], [5.0, 20.0, 0.0]],
    ];
    assert_eq!(
        drilled_hole_placement_from_corner_envelopes(corners, 10.0, 30.0),
        Some((Point3::new(0.0, -20.0, -30.0), Vector3::new(0.0, 0.0, 1.0)))
    );
    let common_lower_bound = [
        [[-5.0, -20.0, -30.0], [5.0, 25.0, 0.0]],
        [[-5.0, -20.0, -30.0], [5.0, 15.0, 0.0]],
    ];
    assert_eq!(
        drilled_hole_placement_from_corner_envelopes(common_lower_bound, 10.0, 30.0),
        Some((Point3::new(0.0, 20.0, -30.0), Vector3::new(0.0, 0.0, 1.0)))
    );

    let wrong_diameter = [corners[0], [[-5.0, -26.0, -30.0], [5.0, 20.0, 0.0]]];
    assert!(drilled_hole_placement_from_corner_envelopes(wrong_diameter, 10.0, 30.0).is_none());
    let no_common_bound = [corners[0], [[-5.0, -25.0, -30.0], [5.0, 19.0, 0.0]]];
    assert!(drilled_hole_placement_from_corner_envelopes(no_common_bound, 10.0, 30.0).is_none());
}

#[test]
fn drill_tip_cone_points_define_a_clipped_radial_coordinate() {
    let corners = [
        [[0.461_241_074, 749.0, -25.0], [-144.0, 755.0, 0.0]],
        [[0.461_241_074, 755.0, -25.0], [-144.0, 761.0, 0.0]],
    ];
    let cone_points = [[-144.0, 755.0, -25.0], [-144.0, 761.0, -25.0]];
    assert_eq!(
        clipped_drilled_hole_placement_from_cone_points(corners, cone_points, 12.0, 25.0),
        Some((
            Point3::new(-144.0, 755.0, -25.0),
            Vector3::new(0.0, 0.0, 1.0),
        ))
    );

    let mut wrong_entry = cone_points;
    wrong_entry[1][2] = 0.0;
    assert!(
        clipped_drilled_hole_placement_from_cone_points(corners, wrong_entry, 12.0, 25.0,)
            .is_none()
    );
    let mut wrong_radial_corner = cone_points;
    wrong_radial_corner[0][0] = -143.0;
    assert!(clipped_drilled_hole_placement_from_cone_points(
        corners,
        wrong_radial_corner,
        12.0,
        25.0,
    )
    .is_none());
    assert!(
        clipped_drilled_hole_placement_from_cone_points(corners, cone_points, 10.0, 25.0,)
            .is_none()
    );
}

#[test]
fn class_911_simple_drilled_recipe_transfers_dimension_tuple() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features
        .entity_tables
        .push(simple_drilled_recipe_table(9));
    scan.surfaces
        .rows
        .extend(simple_drilled_recipe_surface_rows(9));
    let drill_point_angle = 118.0_f64.to_radians();
    let dimension =
        |dimension_type, external_id, value, value_unit| crate::feature::FeatureDimension {
            dimension_type,
            value: Some(value),
            value_body: Vec::new(),
            unresolved_value_token: None,
            value_unit,
            direction_byte: 0,
            auxiliary_value: Some(0.0),
            auxiliary_body: Vec::new(),
            external_id,
            references: None,
            offset: 0,
        };
    scan.features
        .definitions
        .push(crate::feature::FeatureDefinition {
            id: 911,
            owner_feature_id: None,
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: Some(crate::feature::FeatureDimensionTable {
                declared_count: 3,
                entity_ref: Some(88),
                rows: vec![
                    dimension(2, 0, 4.2, crate::feature::DimensionUnit::Millimeters),
                    dimension(
                        10,
                        1,
                        drill_point_angle,
                        crate::feature::DimensionUnit::Radians,
                    ),
                    dimension(2, 2, -25.0, crate::feature::DimensionUnit::Millimeters),
                ],
                offset: 0,
            }),
            relations: None,
            saved_section: None,
            offset: 0,
        });

    assert!(matches!(
        schema_feature_definition(
            &scan,
            &CadIr::empty(Units::default()),
            9,
            911,
            "Hole"
        ),
        IrFeatureDefinition::Hole {
            kind: HoleKind::SimpleDrilled {
                drill_point_angle: Angle(angle),
            },
            diameter: Some(Length(8.4)),
            extent: Some(Termination::Blind {
                length: Length(25.0),
            }),
            bottom: None,
            ..
        } if approximately_equal(angle, drill_point_angle)
    ));

    let compact_entry =
        |entity_id, class_id, source_entity_id| crate::feature::FeatureEntityTableEntry {
            entity_id,
            class_id,
            source_entity_id,
            related_entity_id: None,
            related_entity_state: None,
            prefixed: false,
            offset: 0,
            end_offset: 0,
        };
    scan.features
        .entity_tables
        .push(crate::feature::FeatureEntityTable {
            feature_id: Some(9),
            table_class_id: 29,
            entry_ids: vec![21, 22, 23, 24],
            entries: vec![
                compact_entry(21, 204, None),
                compact_entry(22, 203, None),
                compact_entry(23, 200, Some(0)),
                compact_entry(24, 200, None),
            ],
            surface_ids: vec![24],
            non_surface_entity_ids: Vec::new(),
            offset: 0,
        });
    scan.surfaces.rows.push(class_911_surface_row(
        9,
        24,
        crate::surface::SurfaceKind::Cylinder,
    ));
    assert!(matches!(
        schema_feature_definition(&scan, &CadIr::empty(Units::default()), 9, 911, "Hole"),
        IrFeatureDefinition::Hole {
            kind: HoleKind::Simple,
            diameter: None,
            extent: None,
            ..
        }
    ));
}

#[test]
fn counterbore_sources_require_materialized_table_membership() {
    let entry = |entity_id, source_entity_id| crate::feature::FeatureEntityTableEntry {
        entity_id,
        class_id: 200,
        source_entity_id: Some(source_entity_id),
        related_entity_id: None,
        related_entity_state: None,
        prefixed: false,
        offset: 0,
        end_offset: 0,
    };
    let entries = vec![entry(11, 4), entry(12, 4), entry(15, 7), entry(16, 7)];
    let table = crate::feature::FeatureEntityTable {
        feature_id: Some(9),
        table_class_id: 29,
        entry_ids: entries.iter().map(|entry| entry.entity_id).collect(),
        entries,
        surface_ids: vec![11, 15, 16],
        non_surface_entity_ids: vec![12],
        offset: 0,
    };
    let row = |id| crate::surface::SurfaceRow {
        id,
        type_byte: crate::surface::SurfaceKind::Cylinder.canonical_type_byte(),
        kind: crate::surface::SurfaceKind::Cylinder,
        feature_id: 9,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 0,
    };
    let mut scan = crate::container::scan_bytes(Vec::new());
    let duplicate_productive_table = table.clone();
    scan.features.entity_tables.push(table);
    scan.features
        .entity_tables
        .push(crate::feature::FeatureEntityTable {
            feature_id: Some(9),
            table_class_id: 29,
            entry_ids: vec![99],
            entries: vec![crate::feature::FeatureEntityTableEntry {
                entity_id: 99,
                class_id: 0,
                source_entity_id: None,
                related_entity_id: None,
                related_entity_state: None,
                prefixed: true,
                offset: 1,
                end_offset: 2,
            }],
            surface_ids: Vec::new(),
            non_surface_entity_ids: vec![99],
            offset: 1,
        });
    scan.surfaces
        .rows
        .extend([row(11), row(12), row(15), row(16)]);

    assert_eq!(
        counterbore_cylinder_sources(&scan, 9),
        Some(vec![vec![15, 16]])
    );
    scan.features.entity_tables.push(duplicate_productive_table);
    assert!(counterbore_cylinder_sources(&scan, 9).is_none());
}

#[test]
fn counterbore_dimensions_require_complete_agreeing_radius_anchored_tables() {
    let table = |depth: f64| crate::feature::FeatureDimensionTable {
        declared_count: 4,
        entity_ref: Some(88),
        rows: [
            (2, 0.098, 0),
            (2, 0.463_628_944_932_919_5, 1),
            (1, depth, 2),
            (2, 0.3125, 3),
        ]
        .into_iter()
        .map(
            |(dimension_type, value, external_id)| crate::feature::FeatureDimension {
                dimension_type,
                value: Some(value),
                value_body: Vec::new(),
                unresolved_value_token: None,
                value_unit: crate::feature::DimensionUnit::Millimeters,
                direction_byte: 0,
                auxiliary_value: Some(0.0),
                auxiliary_body: Vec::new(),
                external_id,
                references: None,
                offset: 0,
            },
        )
        .collect(),
        offset: 0,
    };
    let first = table(0.15);
    let second = table(0.15);

    assert_eq!(
        counterbore_dimension_values([&first, &second].into_iter(), &[0.3125]),
        Some((0.196, 0.625, 0.15))
    );
    assert_eq!(
        counterbore_dimension_values([&first].into_iter(), &[0.25]),
        None
    );
    let conflicting = table(0.2);
    assert_eq!(
        counterbore_dimension_values([&first, &conflicting].into_iter(), &[0.3125]),
        None
    );
}

#[test]
fn counterbore_envelope_family_accepts_signed_depth_and_optional_drill_angle() {
    let table = |counterbore_depth: f64| crate::feature::FeatureDimensionTable {
        declared_count: 5,
        entity_ref: Some(88),
        rows: [
            (
                0,
                1,
                counterbore_depth,
                crate::feature::DimensionUnit::Millimeters,
            ),
            (1, 2, 20.0, crate::feature::DimensionUnit::Millimeters),
            (
                2,
                10,
                118.0_f64.to_radians(),
                crate::feature::DimensionUnit::Radians,
            ),
            (3, 2, 60.0, crate::feature::DimensionUnit::Millimeters),
            (4, 2, -295.661, crate::feature::DimensionUnit::Millimeters),
        ]
        .into_iter()
        .map(
            |(external_id, dimension_type, value, value_unit)| crate::feature::FeatureDimension {
                dimension_type,
                value: Some(value),
                value_body: Vec::new(),
                unresolved_value_token: None,
                value_unit,
                direction_byte: 0,
                auxiliary_value: Some(0.0),
                auxiliary_body: Vec::new(),
                external_id,
                references: None,
                offset: 0,
            },
        )
        .collect(),
        offset: 0,
    };
    let bore_spans = [[Some(40.0), None], [Some(49.0), None], [None, Some(40.0)]];
    let counterbore_spans = [[Some(120.0), None], [Some(8.0), None], [None, Some(120.0)]];
    let first = table(8.0);
    assert_eq!(
        counterbore_envelope_dimension_values(
            std::iter::once(&first),
            &[Some(bore_spans), Some(counterbore_spans)],
        ),
        Some((40.0, 120.0, 8.0))
    );
    let signed_depth = table(-8.0);
    assert_eq!(
        counterbore_envelope_dimension_values(
            std::iter::once(&signed_depth),
            &[Some(bore_spans), Some(counterbore_spans)],
        ),
        Some((40.0, 120.0, 8.0))
    );
    assert_eq!(
        counterbore_unenveloped_dimension_values([&signed_depth, &signed_depth].into_iter()),
        Some((40.0, 120.0, 8.0))
    );
    assert_eq!(
        counterbore_envelope_dimension_values(
            std::iter::once(&first),
            &[Some(counterbore_spans), Some(bore_spans)],
        ),
        Some((40.0, 120.0, 8.0))
    );
    let mut without_drill_angle = table(8.0);
    without_drill_angle.declared_count = 4;
    without_drill_angle.rows.retain(|row| row.external_id != 2);
    assert_eq!(
        counterbore_envelope_dimension_values(
            std::iter::once(&without_drill_angle),
            &[Some(bore_spans), Some(counterbore_spans)],
        ),
        Some((40.0, 120.0, 8.0))
    );
    let mut shifted_four_row = without_drill_angle.clone();
    for row in &mut shifted_four_row.rows {
        if row.external_id >= 3 {
            row.external_id -= 1;
        }
    }
    assert_eq!(
        counterbore_envelope_dimension_values(
            std::iter::once(&shifted_four_row),
            &[None, Some(counterbore_spans)],
        ),
        Some((40.0, 120.0, 8.0))
    );
    let one_sided_counterbore_spans = paired_corner_envelope_axis_spans(
        [[0.0, 0.0, 68.0], [120.0, 8.0, 0.0]],
        [[0.0, 0.0, 0.0], [120.0, 8.0, 188.0]],
    )
    .expect("finite one-sided envelope pair");
    assert_eq!(
        counterbore_envelope_dimension_values(
            std::iter::once(&shifted_four_row),
            &[None, Some(one_sided_counterbore_spans)],
        ),
        Some((40.0, 120.0, 8.0))
    );
    assert_eq!(
        counterbore_envelope_dimension_values(
            std::iter::once(&shifted_four_row),
            &[Some(bore_spans), None],
        ),
        Some((40.0, 120.0, 8.0))
    );
    let dual_role_spans = [
        [Some(40.0), Some(120.0)],
        [Some(40.0), Some(120.0)],
        [Some(8.0), None],
    ];
    assert!(counterbore_envelope_dimension_values(
        std::iter::once(&shifted_four_row),
        &[Some(dual_role_spans), None],
    )
    .is_none());
    let mut invalid_drill_angle = table(8.0);
    invalid_drill_angle
        .rows
        .iter_mut()
        .find(|row| row.external_id == 2)
        .expect("the five-row test table has a drill-angle row")
        .value = Some(std::f64::consts::PI);
    assert!(counterbore_envelope_dimension_values(
        std::iter::once(&invalid_drill_angle),
        &[Some(bore_spans), Some(counterbore_spans)],
    )
    .is_none());
    let conflicting = table(9.0);
    assert_eq!(
        counterbore_envelope_dimension_values(
            [&first, &conflicting].into_iter(),
            &[Some(bore_spans), Some(counterbore_spans)],
        ),
        Some((40.0, 120.0, 8.0))
    );
    assert!(counterbore_envelope_dimension_values(
        std::iter::once(&conflicting),
        &[Some(bore_spans), Some(counterbore_spans)],
    )
    .is_none());
    assert!(
        counterbore_unenveloped_dimension_values([&signed_depth, &conflicting].into_iter())
            .is_none()
    );
    let mut other_layout = table(8.0);
    other_layout.rows[0].dimension_type = 2;
    assert!(
        counterbore_unenveloped_dimension_values([&signed_depth, &other_layout].into_iter())
            .is_none()
    );
}

#[test]
fn counterbore_bore_patches_inherit_the_unique_larger_cylinder_frame() {
    let carrier = SurfaceGeometry::Cylinder {
        origin: Point3::new(1.0, 2.0, 3.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 0.3125,
    };
    let mut existing = BTreeMap::from([(30, carrier.clone()), (31, carrier.clone())]);
    let sources = vec![vec![10, 11], vec![30, 31]];

    let patches = counterbore_source_patch_geometries(&sources, &existing, 0.196, 0.625)
        .expect("coaxial patches");

    assert_eq!(patches.len(), 4);
    assert!(patches
        .iter()
        .filter(|(id, _)| *id < 30)
        .all(|(_, geometry)| {
            matches!(geometry, SurfaceGeometry::Cylinder { origin, axis, radius, .. }
                if *origin == Point3::new(1.0, 2.0, 3.0)
                    && *axis == Vector3::new(0.0, 0.0, 1.0)
                    && (*radius - 0.098).abs() < 1.0e-12)
        }));
    assert_eq!(
        counterbore_axis_placement_from_sources(&sources, &existing, 0.625),
        Some(cadmpeg_ir::features::HolePlacement::Axis {
            origin: Point3::new(1.0, 2.0, 3.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
        })
    );
    let mut conflicting_patch = existing.clone();
    let SurfaceGeometry::Cylinder { radius, .. } =
        conflicting_patch.get_mut(&31).expect("second patch")
    else {
        unreachable!()
    };
    *radius = 0.25;
    assert_eq!(
        counterbore_axis_placement_from_sources(&sources, &conflicting_patch, 0.625),
        None
    );
    existing.insert(10, carrier);
    assert_eq!(
        counterbore_source_patch_geometries(&sources, &existing, 0.196, 0.625),
        None
    );
    let duplicate = existing[&30].clone();
    existing.insert(11, duplicate);
    assert_eq!(
        counterbore_axis_placement_from_sources(&sources, &existing, 0.625),
        None
    );
}

#[test]
fn counterbore_step_support_supplies_only_its_unoriented_normal_axis() {
    let table = crate::feature::FeatureEntityTable {
        feature_id: Some(9),
        table_class_id: 29,
        entry_ids: Vec::new(),
        entries: Vec::new(),
        surface_ids: vec![11, 13, 15],
        non_surface_entity_ids: Vec::new(),
        offset: 0,
    };
    let rows = [
        class_911_surface_row(9, 11, crate::surface::SurfaceKind::Cylinder),
        class_911_surface_row(9, 13, crate::surface::SurfaceKind::Plane),
        class_911_surface_row(9, 15, crate::surface::SurfaceKind::Cylinder),
    ];
    let frame = crate::surface::PlaneLocalSystem {
        surface_id: 13,
        body: Vec::new(),
        slots: vec![Some(0.0); 12],
        origin: Some([2.0, 3.0, 4.0]),
        u_axis: Some([0.0, 0.0, 1.0]),
        normal: Some([0.0, -2.0, 0.0]),
        classification: crate::surface::LocalSystemClassification::Simple,
        row_offset: 0,
        offset: 0,
    };

    assert_eq!(
        counterbore_support_axis_placement(9, &table, &rows, std::slice::from_ref(&frame)),
        Some(cadmpeg_ir::features::HolePlacement::Axis {
            origin: Point3::new(2.0, 3.0, 4.0),
            axis: Vector3::new(0.0, -1.0, 0.0),
        })
    );
    assert!(
        counterbore_support_axis_placement(10, &table, &rows, std::slice::from_ref(&frame),)
            .is_none()
    );
    let mut incomplete = frame.clone();
    incomplete.normal = None;
    assert!(counterbore_support_axis_placement(
        9,
        &table,
        &rows,
        std::slice::from_ref(&incomplete),
    )
    .is_none());
    assert!(
        counterbore_support_axis_placement(9, &table, &rows, &[frame.clone(), frame]).is_none()
    );
}

#[test]
fn simple_drilled_axis_accepts_only_coaxial_dimension_matched_carriers() {
    let frame = |origin, axis, radius| crate::surface::PositionalCylinderFrame {
        origin,
        axis,
        ref_direction: [0.0, 1.0, 0.0],
        radius,
        length: None,
    };
    let first = frame([2.0, -3.0, 4.0], [1.0, 0.0, 0.0], 0.25);
    let shifted = frame([7.0, -3.0, 4.0], [-1.0, 0.0, 0.0], 0.25);

    assert_eq!(
        simple_drilled_axis_placement_from_frames(&[first, shifted], 0.5),
        Some(cadmpeg_ir::features::HolePlacement::Axis {
            origin: Point3::new(2.0, -3.0, 4.0),
            axis: Vector3::new(1.0, 0.0, 0.0),
        })
    );
    assert!(simple_drilled_axis_placement_from_frames(&[], 0.5).is_none());
    assert!(simple_drilled_axis_placement_from_frames(
        &[first, frame([2.0, -2.9, 4.0], [1.0, 0.0, 0.0], 0.25)],
        0.5,
    )
    .is_none());
    assert!(simple_drilled_axis_placement_from_frames(
        &[frame([2.0, -3.0, 4.0], [1.0, 0.0, 0.0], 0.3)],
        0.5,
    )
    .is_none());
    assert!(simple_drilled_axis_placement_from_frames(
        &[frame([2.0, -3.0, 4.0], [1.0, 0.0, 0.0], f64::NAN,)],
        0.5,
    )
    .is_none());
}

#[test]
fn counterbore_boundary_circles_define_the_directed_full_span() {
    let counterbore = (65, Point3::new(0.0, 2.625, -1.0), [0.0, 0.0, 1.0]);
    let bore = (61, Point3::new(0.0, 2.625, 0.0), [0.0, 0.0, -1.0]);
    assert_eq!(
        counterbore_directed_span(counterbore, bore, 0.15),
        Some((
            65,
            Point3::new(0.0, 2.625, -1.0),
            Vector3::new(0.0, 0.0, 1.0),
            Termination::Blind {
                length: Length(1.0),
            },
        ))
    );
    assert!(counterbore_directed_span(counterbore, bore, 1.1).is_none());
    assert!(counterbore_directed_span(
        counterbore,
        (61, Point3::new(0.1, 2.625, 0.0), [0.0, 0.0, -1.0]),
        0.15,
    )
    .is_none());
    assert!(counterbore_directed_span(
        counterbore,
        (61, Point3::new(0.0, 2.625, 0.0), [1.0, 0.0, 0.0]),
        0.15,
    )
    .is_none());
}

#[test]
fn counterbore_corner_envelopes_define_the_directed_stepped_span() {
    let bore = [
        [[-20.0, -32.0, -160.0], [20.0, 17.0, -140.0]],
        [[-20.0, -32.0, -140.0], [20.0, 17.0, -120.0]],
    ];
    let counterbore = [
        [[-60.0, -40.0, -200.0], [60.0, -32.0, -140.0]],
        [[-60.0, -40.0, -140.0], [60.0, -32.0, -80.0]],
    ];
    let expected = Some((
        Point3::new(0.0, -40.0, -140.0),
        Vector3::new(0.0, 1.0, 0.0),
        Termination::Blind {
            length: Length(57.0),
        },
    ));
    assert_eq!(
        counterbore_placement_from_corner_envelopes(&[bore, counterbore], 40.0, 120.0, 8.0),
        expected
    );
    assert_eq!(
        counterbore_placement_from_corner_envelopes(&[counterbore, bore], 40.0, 120.0, 8.0),
        expected
    );

    let reverse_bore = [
        [[225.0, 174.0, -211.0], [257.0, 226.0, -185.0]],
        [[225.0, 174.0, -185.0], [257.0, 226.0, -159.0]],
    ];
    let reverse_counterbore = [
        [[257.0, 150.0, -235.0], [265.0, 250.0, -185.0]],
        [[257.0, 150.0, -185.0], [265.0, 250.0, -135.0]],
    ];
    assert_eq!(
        counterbore_placement_from_corner_envelopes(
            &[reverse_bore, reverse_counterbore],
            52.0,
            100.0,
            8.0,
        ),
        Some((
            Point3::new(265.0, 200.0, -185.0),
            Vector3::new(-1.0, 0.0, 0.0),
            Termination::Blind {
                length: Length(40.0),
            },
        ))
    );

    let mut separated_counterbore = counterbore;
    for patch in &mut separated_counterbore {
        patch[0][1] -= 1.0;
        patch[1][1] -= 1.0;
    }
    assert!(counterbore_placement_from_corner_envelopes(
        &[bore, separated_counterbore],
        40.0,
        120.0,
        8.0,
    )
    .is_none());
    assert!(
        counterbore_placement_from_corner_envelopes(&[bore, counterbore], 40.0, 120.0, 9.0,)
            .is_none()
    );
}

#[test]
fn surface_coverage_separates_transferred_unique_rows_from_ambiguous_ids() {
    let row = |id, kind: crate::surface::SurfaceKind| crate::surface::SurfaceRow {
        id,
        type_byte: kind.canonical_type_byte(),
        kind,
        feature_id: 17,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 0,
    };
    let rows = vec![
        row(41, crate::surface::SurfaceKind::Plane),
        row(42, crate::surface::SurfaceKind::Cylinder),
        row(44, crate::surface::SurfaceKind::Extrusion),
        row(43, crate::surface::SurfaceKind::Cone),
        row(43, crate::surface::SurfaceKind::Cone),
    ];
    let plane = |id: &str, native_id: u32| Surface {
        id: SurfaceId(id.to_string()),
        geometry: SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: Some(SourceObjectAssociation {
            format: "creo".to_string(),
            object_id: format!("VisibGeom:{native_id}"),
            name: None,
            color: None,
            visible: None,
            layer: None,
            instance_path: Vec::new(),
        }),
    };
    let surfaces = vec![
        plane("derived-id-independent-of-native-id", 41),
        plane("wrong-family", 42),
        plane("extrusion-carrier", 44),
    ];
    let procedural_surfaces = vec![ProceduralSurface {
        id: ProceduralSurfaceId("extrusion-construction".to_string()),
        surface: SurfaceId("extrusion-carrier".to_string()),
        definition: ProceduralSurfaceDefinition::Extrusion {
            directrix: CurveId("directrix".to_string()),
            parameter_interval: None,
            direction: Vector3::new(0.0, 0.0, 1.0),
            native_position: None,
            revision_form: None,
        },
        cache_fit_tolerance: None,
        record_bounds: None,
    }];

    let coverage = surface_transfer_coverage(&rows, &surfaces, &procedural_surfaces);

    assert_eq!(coverage.unique_rows, 3);
    assert_eq!(coverage.transferred_rows, 2);
    assert_eq!(coverage.ambiguous_rows, 2);
    assert_eq!(coverage.by_family["plane"], (1, 1));
    assert_eq!(coverage.by_family["cylinder"], (1, 0));
    assert_eq!(coverage.by_family["cone"], (0, 0));
    assert_eq!(coverage.by_family["extrusion"], (1, 1));
}

#[test]
fn curve_coverage_excludes_unknown_carriers_and_ambiguous_ids() {
    let row = |id, type_byte| crate::curve::CurveTopologyRow {
        id,
        type_byte,
        feature_id: 17,
        directions: [0x01, 0xf6],
        faces: [1, 2],
        next_edges: [id, id],
        offset: 0,
    };
    let rows = vec![row(41, 0x05), row(42, 0x13), row(43, 0x05), row(43, 0x05)];
    let source = |native_id| SourceObjectAssociation {
        format: "creo".to_string(),
        object_id: format!("VisibGeom:{native_id}"),
        name: None,
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    };
    let curves = vec![
        Curve {
            id: CurveId("typed".to_string()),
            geometry: CurveGeometry::Line {
                origin: Point3::new(0.0, 0.0, 0.0),
                direction: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: Some(source(41)),
        },
        Curve {
            id: CurveId("opaque".to_string()),
            geometry: CurveGeometry::Unknown { record: None },
            source_object: Some(source(42)),
        },
    ];

    let coverage = curve_transfer_coverage(&rows, &curves);

    assert_eq!(coverage.unique_rows, 2);
    assert_eq!(coverage.transferred_rows, 1);
    assert_eq!(coverage.ambiguous_rows, 2);
    assert_eq!(coverage.by_type[&0x05], (1, 1));
    assert_eq!(coverage.by_type[&0x13], (1, 0));
}

#[test]
fn design_constraint_coverage_separates_typed_and_native_constraints() {
    let sketch = SketchId("sketch".to_string());
    let constraint = |id: &str, definition| SketchConstraint {
        id: SketchConstraintId(id.to_string()),
        sketch: sketch.clone(),
        definition,
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: None,
    };
    let entity = SketchEntityId("entity".to_string());
    let mut constraints = vec![
        constraint(
            "sketch:relation:1",
            SketchConstraintDefinition::Fixed {
                entity: entity.clone(),
            },
        ),
        constraint(
            "sketch:relation:2",
            SketchConstraintDefinition::Native {
                native_kind: "creo:relation:9".to_string(),
                entities: vec![entity.clone()],
                parameter: None,
                operands: Vec::new(),
                native_state: None,
                native_flags: None,
                native_properties: std::collections::BTreeMap::new(),
            },
        ),
        constraint(
            "sketch:skamp:3",
            SketchConstraintDefinition::Fixed { entity },
        ),
    ];
    constraints[0].active = Some(true);
    constraints[1].active = Some(true);
    constraints[2].active = Some(false);

    let coverage =
        design_constraint_transfer_coverage(&constraints, ":relation:", "creo:relation:");

    assert_eq!(coverage.transferred, 2);
    assert_eq!(coverage.native, 1);
    assert_eq!(coverage.typed(), 1);
    assert_eq!(coverage.active, 2);
    assert_eq!(coverage.active_native, 1);
    assert_eq!(coverage.active_typed(), 1);
    assert_eq!(coverage.native_by_kind, BTreeMap::from([(9, 1)]));
    assert_eq!(coverage.active_native_by_kind, BTreeMap::from([(9, 1)]));
    assert_eq!(
        constraint_kind_breakdown(
            &BTreeMap::from([
                (
                    "active_native_feature_relation_type_1_constraint_count".to_string(),
                    2,
                ),
                (
                    "active_native_feature_relation_type_9_constraint_count".to_string(),
                    1,
                ),
                (
                    "transferred_native_feature_relation_type_9_constraint_count".to_string(),
                    4,
                ),
            ]),
            "active_native_feature_relation_type_",
        ),
        "type 1=2, type 9=1"
    );
}

#[test]
fn native_curve_families_accept_only_their_defined_loci() {
    let point = SketchEntityId("point".to_string());
    let bounded = SketchEntityId("bounded".to_string());
    let line = SketchEntityId("line".to_string());
    let reference_line = SketchEntityId("reference_line".to_string());
    let circle = SketchEntityId("circle".to_string());
    let geometry = BTreeMap::from([
        (
            point.clone(),
            SketchGeometry::Native {
                native_kind: "point".to_string(),
            },
        ),
        (
            bounded.clone(),
            SketchGeometry::Native {
                native_kind: "bounded_curve".to_string(),
            },
        ),
        (
            line.clone(),
            SketchGeometry::Native {
                native_kind: "line".to_string(),
            },
        ),
        (
            reference_line.clone(),
            SketchGeometry::Native {
                native_kind: "reference_line".to_string(),
            },
        ),
        (
            circle.clone(),
            SketchGeometry::Native {
                native_kind: "circle".to_string(),
            },
        ),
    ]);
    let compatible = SketchConstraintDefinition::CoincidentLoci {
        loci: vec![
            SketchLocus::Entity(point),
            SketchLocus::Start(bounded),
            SketchLocus::Center(circle.clone()),
        ],
    };
    assert!(sketch_constraint_loci_compatible(&compatible, &geometry));
    let incompatible = SketchConstraintDefinition::CoincidentLoci {
        loci: vec![SketchLocus::Start(line), SketchLocus::Start(circle)],
    };
    assert!(!sketch_constraint_loci_compatible(&incompatible, &geometry));
    let centered_midpoint = SketchConstraintDefinition::Midpoint {
        point: SketchLocus::Center(SketchEntityId("line".to_string())),
        entity: SketchEntityId("bounded".to_string()),
    };
    assert!(sketch_constraint_loci_compatible(
        &centered_midpoint,
        &geometry
    ));
    let incompatible_midpoint = SketchConstraintDefinition::Midpoint {
        point: SketchLocus::Center(reference_line),
        entity: SketchEntityId("bounded".to_string()),
    };
    assert!(!sketch_constraint_loci_compatible(
        &incompatible_midpoint,
        &geometry
    ));
}

#[test]
fn incidence_family_lattice_narrows_endpoint_evidence() {
    let mut line = BTreeSet::from([
        SectionEntityIncidenceFamily::BoundedCurve,
        SectionEntityIncidenceFamily::Line,
    ]);
    normalize_section_incidence_curve_family_evidence(&mut line);
    assert_eq!(line, BTreeSet::from([SectionEntityIncidenceFamily::Line]));

    let mut arc = BTreeSet::from([
        SectionEntityIncidenceFamily::BoundedCurve,
        SectionEntityIncidenceFamily::Circular,
    ]);
    normalize_section_incidence_curve_family_evidence(&mut arc);
    assert_eq!(arc, BTreeSet::from([SectionEntityIncidenceFamily::Arc]));

    let mut conflicting = BTreeSet::from([
        SectionEntityIncidenceFamily::Line,
        SectionEntityIncidenceFamily::Circular,
    ]);
    normalize_section_incidence_curve_family_evidence(&mut conflicting);
    assert_eq!(conflicting.len(), 2);
}

#[test]
fn rowless_round_cylinder_requires_the_four_entry_sibling_layout() {
    let row = |id, kind: crate::surface::SurfaceKind| crate::surface::SurfaceRow {
        id,
        type_byte: kind.canonical_type_byte(),
        kind,
        feature_id: 23,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 0,
    };
    let mut rows = vec![
        row(10, crate::surface::SurfaceKind::Plane),
        row(11, crate::surface::SurfaceKind::Plane),
        row(13, crate::surface::SurfaceKind::Cylinder),
    ];
    let table = crate::feature::FeatureEntityTable {
        feature_id: Some(23),
        table_class_id: 80,
        entry_ids: vec![10, 11, 12, 13],
        entries: Vec::new(),
        surface_ids: vec![10, 11, 13],
        non_surface_entity_ids: vec![12],
        offset: 47,
    };
    assert_eq!(
        rowless_round_cylinder_pairs(&BTreeSet::from([23]), std::slice::from_ref(&table), &rows,),
        vec![(12, 13, 47)]
    );
    assert!(
        rowless_round_cylinder_pairs(&BTreeSet::new(), std::slice::from_ref(&table), &rows,)
            .is_empty()
    );
    rows[2].reversed = true;
    assert_eq!(
        rowless_round_face_orientations(
            &BTreeSet::from([23]),
            std::slice::from_ref(&table),
            &rows,
            &BTreeSet::from([12]),
        ),
        BTreeMap::from([(12, true)])
    );
    assert!(rowless_round_face_orientations(
        &BTreeSet::from([23]),
        std::slice::from_ref(&table),
        &rows,
        &BTreeSet::new(),
    )
    .is_empty());
    let mut materialized_rowless = rows;
    materialized_rowless.push(row(12, crate::surface::SurfaceKind::Cylinder));
    assert!(
        rowless_round_cylinder_pairs(&BTreeSet::from([23]), &[table], &materialized_rowless,)
            .is_empty()
    );
}

#[test]
fn spline_extrusion_preserves_directrix_basis_and_weights() {
    let directrix = NurbsCurve {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        control_points: vec![
            Point3::new(1.0, 2.0, 3.0),
            Point3::new(4.0, 5.0, 6.0),
            Point3::new(7.0, 8.0, 9.0),
        ],
        weights: Some(vec![1.0, 0.5, 1.0]),
        periodic: false,
    };
    let surface =
        extruded_nurbs_surface(&directrix, [0.0, 0.0, 4.0]).expect("valid extrusion surface");

    assert_eq!((surface.u_degree, surface.v_degree), (2, 1));
    assert_eq!((surface.u_count, surface.v_count), (3, 2));
    assert_eq!(surface.u_knots, directrix.knots);
    assert_eq!(surface.v_knots, [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(
        surface.control_points,
        [
            Point3::new(1.0, 2.0, 3.0),
            Point3::new(1.0, 2.0, 7.0),
            Point3::new(4.0, 5.0, 6.0),
            Point3::new(4.0, 5.0, 10.0),
            Point3::new(7.0, 8.0, 9.0),
            Point3::new(7.0, 8.0, 13.0),
        ]
    );
    assert_eq!(surface.weights, Some(vec![1.0, 1.0, 0.5, 0.5, 1.0, 1.0]));
}

#[test]
fn reversed_arc_uses_opposite_axis_and_canonical_increasing_domain() {
    let (axis_sign, range) = oriented_arc_parameterization(
        true,
        -std::f64::consts::FRAC_PI_2,
        std::f64::consts::FRAC_PI_2,
    );

    assert_eq!(axis_sign, -1.0);
    assert_eq!(
        range,
        [
            3.0 * std::f64::consts::FRAC_PI_2,
            5.0 * std::f64::consts::FRAC_PI_2
        ]
    );
}

#[test]
fn extrusion_arc_pcurve_is_exact_in_both_directions() {
    for (start, end, expected_middle) in [
        (0.0, std::f64::consts::PI, Point2::new(2.0, 5.0)),
        (std::f64::consts::PI, 0.0, Point2::new(2.0, 5.0)),
    ] {
        let pcurve = circular_pcurve([2.0, 2.0], 3.0, start, end);
        let first = cadmpeg_ir::eval::pcurve_uv(&pcurve, 0.0).expect("first endpoint");
        let middle = cadmpeg_ir::eval::pcurve_uv(&pcurve, 0.5).expect("arc midpoint");
        let last = cadmpeg_ir::eval::pcurve_uv(&pcurve, 1.0).expect("last endpoint");
        assert!((first.u - (2.0 + 3.0 * start.cos())).abs() < 1.0e-12);
        assert!((first.v - (2.0 + 3.0 * start.sin())).abs() < 1.0e-12);
        assert!((middle.u - expected_middle.u).abs() < 1.0e-12);
        assert!((middle.v - expected_middle.v).abs() < 1.0e-12);
        assert!((last.u - (2.0 + 3.0 * end.cos())).abs() < 1.0e-12);
        assert!((last.v - (2.0 + 3.0 * end.sin())).abs() < 1.0e-12);
    }
}

#[test]
fn extrusion_profile_area_includes_oriented_arc_sector() {
    let arc = SketchGeometry::Arc {
        center: Point2::new(0.0, 0.0),
        radius: Length(1.0),
        start_angle: Angle(0.0),
        end_angle: Angle(std::f64::consts::PI),
    };
    let line = SketchGeometry::Line {
        start: Point2::new(-1.0, 0.0),
        end: Point2::new(1.0, 0.0),
    };
    let counterclockwise = vec![
        (arc.clone(), false, [1.0, 0.0], [-1.0, 0.0]),
        (line.clone(), false, [-1.0, 0.0], [1.0, 0.0]),
    ];
    let clockwise = vec![
        (arc, true, [-1.0, 0.0], [1.0, 0.0]),
        (line, true, [1.0, 0.0], [-1.0, 0.0]),
    ];
    assert!(
        (extrusion_profile_signed_area(&counterclockwise).expect("positive area")
            - std::f64::consts::FRAC_PI_2)
            .abs()
            < 1.0e-12
    );
    assert!(
        (extrusion_profile_signed_area(&clockwise).expect("negative area")
            + std::f64::consts::FRAC_PI_2)
            .abs()
            < 1.0e-12
    );
}

#[test]
fn full_turn_arc_remains_a_closed_extrusion_profile() {
    let profile = vec![(
        SketchGeometry::Arc {
            center: Point2::new(0.0, 0.0),
            radius: Length(2.0),
            start_angle: Angle(0.0),
            end_angle: Angle(std::f64::consts::TAU),
        },
        false,
        [2.0, 0.0],
        [2.0, 0.0],
    )];
    let (profiles, area) = ordered_extrusion_profiles(vec![profile.clone()])
        .expect("a full-turn arc is a closed profile");
    assert_eq!(profiles, vec![profile]);
    assert!((area - 4.0 * std::f64::consts::PI).abs() < 1.0e-12);
    assert_eq!(
        oriented_arc_parameterization(false, 0.0, std::f64::consts::TAU).1,
        [0.0, std::f64::consts::TAU]
    );
    assert_eq!(
        oriented_arc_parameterization(true, 0.0, std::f64::consts::TAU).1,
        [0.0, std::f64::consts::TAU]
    );
}

#[test]
fn circle_remains_a_closed_extrusion_profile() {
    let sketch_id = SketchId("creo:model:sketch#circle".to_string());
    let entity_id = SketchEntityId("creo:model:sketch_entity#circle".to_string());
    let circle = SketchGeometry::Circle {
        center: Point2::new(1.0, -2.0),
        radius: Length(3.0),
    };
    let seam = [4.0, -2.0];
    let mut ir = CadIr::empty(Units::default());
    ir.model.sketches.push(Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Unresolved,
        profiles: vec![vec![SketchEntityUse {
            entity: entity_id.clone(),
            reversed: false,
        }]],
        native_ref: None,
    });
    ir.model.sketch_entities.push(SketchEntity {
        id: entity_id,
        sketch: sketch_id.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: circle.clone(),
    });

    let profiles = resolved_sketch_profiles(&ir, &sketch_id, 1).expect("one circle profile");
    assert_eq!(profiles, vec![vec![(circle.clone(), false, seam, seam)]]);
    let (ordered, area) = ordered_extrusion_profiles(profiles.clone()).expect("closed circle");
    assert_eq!(ordered, profiles);
    assert!((area - 9.0 * std::f64::consts::PI).abs() < 1.0e-12);

    for reversed in [false, true] {
        let pcurve = extrusion_cap_pcurve(&circle, reversed, seam, seam);
        let first = cadmpeg_ir::eval::pcurve_uv(&pcurve, 0.0).expect("circle seam");
        let middle = cadmpeg_ir::eval::pcurve_uv(&pcurve, 0.5).expect("circle midpoint");
        let last = cadmpeg_ir::eval::pcurve_uv(&pcurve, 1.0).expect("circle seam");
        assert!((first.u - seam[0]).abs() < 1.0e-12);
        assert!((first.v - seam[1]).abs() < 1.0e-12);
        assert!((middle.u - (1.0 - 3.0)).abs() < 1.0e-12);
        assert!((middle.v + 2.0).abs() < 1.0e-12);
        assert!((last.u - seam[0]).abs() < 1.0e-12);
        assert!((last.v - seam[1]).abs() < 1.0e-12);
        assert_eq!(
            extrusion_side_uvs(
                &circle,
                reversed,
                seam,
                seam,
                ExtrusionSpan {
                    lower: -1.0,
                    upper: 2.0,
                },
            )[0],
            [
                [oriented_full_turn_angles(reversed)[0], -1.0],
                [oriented_full_turn_angles(reversed)[1], -1.0],
            ]
        );
        assert_eq!(
            profile_arc(&(circle.clone(), reversed, seam, seam)),
            Some((
                [1.0, -2.0],
                3.0,
                0.0,
                if reversed {
                    -std::f64::consts::TAU
                } else {
                    std::f64::consts::TAU
                },
            ))
        );
    }
    assert!(point_on_profile_arc(
        seam,
        profile_arc(&(circle, false, seam, seam)).expect("circle arc"),
        1.0e-9,
    ));
    assert_eq!(
        oriented_full_turn_angles(false),
        [0.0, std::f64::consts::TAU]
    );
    assert_eq!(
        oriented_full_turn_angles(true),
        [std::f64::consts::TAU, 0.0]
    );
}
