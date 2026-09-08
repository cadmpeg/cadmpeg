// SPDX-License-Identifier: Apache-2.0

use super::super::transfer_resolved_revolution_surfaces;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, NurbsCurve};
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::AnnotationBuilder;

fn saved_spline_definition() -> crate::feature::FeatureDefinition {
    crate::feature::FeatureDefinition {
        identity: crate::feature::definitions::DefinitionIdentity::Parsed {
            schema_id: std::num::NonZeroU32::new(40),
            owner_feature_id: Some(40),
        },
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(crate::feature::definitions::test_support::with_points(
            crate::feature::FeatureVariableTable {
                declared_count: 0,
                entity_ref: None,
                rows: Vec::new(),
                offset: 0,
            },
            vec![
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
        )),
        segments: Some(crate::feature::FeatureSegmentTable {
            declared_count: 1,
            has_elided_prototype: false,
            entity_ref: None,
            rows: (vec![crate::feature::FeatureSegment {
                kind: crate::feature::FeatureSegmentKind::Line([1, 2]),
                directions: [None; 3],
                center_id: None,
                arc_orientation: None,
                vertical_horizontal: None,
                radius_ref: None,
                radius2_ref: None,
                external_id: 99,
                body: Vec::new(),
                offset: 0,
            }])
            .into_iter()
            .map(crate::feature::segment_rows::SegmentRow::Ordinary)
            .collect(),
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
            reference_planes: crate::feature::definitions::ReferencePlanes::Named(Vec::new()),
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
                    endpoint_tangents: Some(crate::feature::definitions::DecodedField {
                        value: [[0.0, 0.0, 1.0], [0.0, 0.0, 1.0]],
                        body: Vec::new(),
                    }),
                    parameters: Some(crate::feature::definitions::DecodedField {
                        value: vec![0.0, 1.0],
                        body: Vec::new(),
                    }),
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
    scan.features.section_transforms.push(
        crate::placement::FeatureSectionTransform::new(
            40,
            Some(40),
            [0.0; 3],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            0,
        )
        .expect("valid section frame"),
    );
    scan.features
        .operations
        .push(crate::feature::FeatureOperation {
            feature_id: 40,
            kind: crate::feature::OperationKind::Revolve,
            name: crate::feature::operations::OperationName::Derived,
            recipe: crate::feature::RecipeResolution::Resolved(
                crate::feature::FeatureRecipe::ProtrudeRevolve,
            ),
            display_state_conflict: false,
            depdb: None,
            offset: 0,
            state_offset: 0,
        });
    scan.features
        .revolution_extents
        .push(crate::feature::FeatureRevolutionExtent {
            feature_id: 40,
            offset: 0,
        });
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 20,
        kind: crate::surface::SurfaceKind::Spline,
        feature_id: 40,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    });
    scan.features.entity_tables.push(
        crate::feature::FeatureEntityTable {
            feature_id: 40,
            table_class_id: 29,
            entries: vec![crate::feature::FeatureEntityTableEntry {
                entity_id: 20,
                class_id: 200,
                payload: crate::feature::entry_payload(200, Some(7), None, None),
                prefixed: false,
                offset: 0,
                end_offset: 0,
                is_surface: false,
            }],
            offset: 0,
        }
        .with_surface_ids([20]),
    );

    let mut ir = CadIr::empty();
    ir.model
        .curves
        .extend((0..curve_count).map(|_| saved_spline_curve()));
    let transferred =
        transfer_resolved_revolution_surfaces(&scan, &mut ir, &mut AnnotationBuilder::new())
            .expect("valid source object identity");
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
