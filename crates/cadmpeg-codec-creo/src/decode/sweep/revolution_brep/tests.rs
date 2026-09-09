// SPDX-License-Identifier: Apache-2.0
use super::*;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
    SketchGeometryDefinition, SketchId, SketchPlacement, SketchProfiles,
};

fn definition() -> crate::feature::FeatureDefinition {
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
        saved_section: None,
        offset: 0,
    }
}

#[test]
fn axis_endpoint_with_offset_neighbor_reports_boundary_rejection() {
    const JOIN_OFFSET: f64 = 5.0e-10;
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features.definitions.push(definition());
    scan.features.section_transforms.push(
        crate::placement::FeatureSectionTransform::new(
            40,
            Some(40),
            [0.0; 3],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            0,
        )
        .expect("section frame"),
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
    let sketch_id = SketchId::mint("creo:model:sketch#40".to_string()).expect("sketch id");
    let mut ir = CadIr::empty();
    let mut uses = Vec::new();
    for (index, (start, end)) in [
        ([1.0, 0.0], [0.0, 0.0]),
        ([JOIN_OFFSET, 0.0], [1.0, 1.0]),
        ([1.0, 1.0], [1.0, 0.0]),
    ]
    .into_iter()
    .enumerate()
    {
        let id = SketchEntityId::mint(format!("creo:featdefs:sketch_entity#40:{index}"))
            .expect("entity id");
        ir.model.sketch_entities.push(SketchEntity::new(
            id.clone(),
            sketch_id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(start[0], start[1]),
                end: Point2::new(end[0], end[1]),
            })
            .expect("line"),
        ));
        uses.push(SketchEntityUse {
            entity: id,
            reversed: false,
        });
    }
    ir.model.sketches.push(Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved,
        profiles: SketchProfiles::try_from(vec![uses]).expect("profile"),
        native_ref: None,
    });
    let mut losses = Vec::new();
    let count = transfer_resolved_revolution_breps(
        &scan,
        &mut ir,
        &mut AnnotationBuilder::new(),
        &mut losses,
    )
    .expect("transfer");
    assert_eq!(count, 0);
    assert!(ir.model.faces.is_empty());
    assert!(ir.model.bodies.is_empty());
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::CreoLossCode::BrepTransferIncomplete.kind()
    );
    assert!(losses[0].message.contains("boundary pcurve"));
}
