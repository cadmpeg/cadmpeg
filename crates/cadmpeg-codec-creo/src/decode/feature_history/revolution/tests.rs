// SPDX-License-Identifier: Apache-2.0

use super::super::transfer_resolved_revolution_surfaces;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, NurbsCurve};
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::AnnotationBuilder;

fn saved_spline_definition() -> crate::feature::FeatureDefinition {
    crate::feature::FeatureDefinition {
        id: 40,
        owner_feature_id: Some(40),
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(crate::feature::FeatureVariableTable {
            declared_count: 0,
            entity_ref: None,
            rows: Vec::new(),
            points: vec![
                crate::feature::FeatureSectionPoint {
                    point_id: 1,
                    u: Some(0.0),
                    v: Some(-1.0),
                },
                crate::feature::FeatureSectionPoint {
                    point_id: 2,
                    u: Some(0.0),
                    v: Some(1.0),
                },
            ],
            offset: 0,
        }),
        segments: Some(crate::feature::FeatureSegmentTable {
            declared_count: 1,
            has_elided_prototype: false,
            entity_ref: None,
            rows: vec![crate::feature::FeatureSegment {
                kind: crate::feature::FeatureSegmentKind::Line,
                directions: [None; 3],
                point_ids: [1, 2],
                center_id: None,
                arc_orientation: None,
                vertical_horizontal: None,
                radius_ref: None,
                radius2_ref: None,
                external_id: 99,
                body: Vec::new(),
                offset: 0,
            }],
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
        order_table: Some(crate::feature::FeatureOrderTable {
            declared_count: 1,
            has_prototype: false,
            entity_ref: None,
            rows: vec![crate::feature::FeatureOrderRow {
                external_id: 7,
                internal_id: 1,
                bitmask: 0,
                offset: 0,
            }],
            offset: 0,
        }),
        section_3d: Some(crate::feature::FeatureSection3d {
            sketch_plane_entity_id: None,
            sketch_plane_flip: None,
            reference_plane_entity_ids: Vec::new(),
            reference_plane_rows: Vec::new(),
            reference_plane_datum_geometry_id: None,
            orientation: crate::feature::FeatureSectionOrientation::default(),
            dimension_ids: Vec::new(),
            offset: 0,
        }),
        dimensions: None,
        relations: None,
        saved_section: Some(crate::feature::FeatureSavedSection {
            entities: vec![crate::feature::FeatureSavedEntity::Spline(
                crate::feature::FeatureSavedSpline {
                    entity_id: Some(1),
                    declared_point_count: Some(2),
                    interpolation_points: vec![[2.0, 0.0, 0.0], [2.0, 0.0, 1.0]],
                    interpolation_points_body: Vec::new(),
                    endpoint_tangents: Some([[0.0, 0.0, 1.0], [0.0, 0.0, 1.0]]),
                    endpoint_tangents_body: None,
                    parameters: Some(vec![0.0, 1.0]),
                    parameters_body: None,
                    offset: 0,
                },
            )],
            offset: 0,
        }),
        offset: 0,
    }
}

fn saved_spline_curve() -> Curve {
    Curve {
        id: CurveId::mint("creo:featdefs:saved_spline_curve#40:1".to_string())
            .expect("identity grammar"),
        geometry: CurveGeometry::Nurbs(
            NurbsCurve::new(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![Point3::new(2.0, 0.0, 0.0), Point3::new(2.0, 0.0, 1.0)],
                None,
                false,
            )
            .expect("valid saved-spline curve"),
        ),
        source_object: None,
    }
}

fn transfer_with_curve_count(curve_count: usize) -> (usize, CadIr) {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features.definitions.push(saved_spline_definition());
    scan.features
        .section_transforms
        .push(crate::placement::FeatureSectionTransform {
            definition_id: 40,
            feature_id: Some(40),
            origin: [0.0; 3],
            u_axis: [1.0, 0.0, 0.0],
            v_axis: [0.0, 1.0, 0.0],
            normal: [0.0, 0.0, 1.0],
            offset: 0,
        });
    scan.features
        .operations
        .push(crate::feature::FeatureOperation {
            feature_id: 40,
            kind: crate::feature::OperationKind::Revolve,
            name: crate::feature::OperationName::Recipe,
            recipe: Some(crate::feature::FeatureRecipe::ProtrudeRevolve),
            recipe_conflict: false,
            display_state_conflict: false,
            depdb: None,
            offset: 0,
            state_offset: 0,
        });
    scan.features
        .revolution_extents
        .push(crate::feature::FeatureRevolutionExtent {
            feature_id: 40,
            kind: crate::feature::FeatureRevolutionExtentKind::FullTurn,
            offset: 0,
        });
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 20,
        type_byte: crate::surface::SurfaceKind::Spline.canonical_type_byte(),
        kind: crate::surface::SurfaceKind::Spline,
        feature_id: 40,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 0,
    });
    scan.features
        .entity_tables
        .push(crate::feature::FeatureEntityTable {
            feature_id: Some(40),
            table_class_id: 29,
            entry_ids: vec![20],
            entries: vec![crate::feature::FeatureEntityTableEntry {
                entity_id: 20,
                class_id: 200,
                source_entity_id: Some(7),
                related_entity_id: None,
                related_entity_state: None,
                prefixed: false,
                offset: 0,
                end_offset: 0,
            }],
            surface_ids: vec![20],
            non_surface_entity_ids: Vec::new(),
            offset: 0,
        });

    let mut ir = CadIr::empty();
    ir.model
        .curves
        .extend((0..curve_count).map(|_| saved_spline_curve()));
    let transferred =
        transfer_resolved_revolution_surfaces(&scan, &mut ir, &mut AnnotationBuilder::new());
    (transferred, ir)
}

#[test]
fn saved_spline_revolution_rejects_duplicate_model_curve_ids() {
    let (transferred, ir) = transfer_with_curve_count(1);
    assert_eq!(transferred, 1);
    assert_eq!(ir.model.surfaces.len(), 1);
    assert_eq!(ir.model.procedural_surfaces.len(), 1);

    let (transferred, ir) = transfer_with_curve_count(2);
    assert_eq!(transferred, 0);
    assert!(ir.model.surfaces.is_empty());
    assert!(ir.model.procedural_surfaces.is_empty());
}
