// SPDX-License-Identifier: Apache-2.0
use super::transfer_resolved_revolution_breps;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
    SketchGeometryDefinition, SketchId, SketchPlacement, SketchProfiles,
};
use cadmpeg_ir::AnnotationBuilder;

fn definition() -> crate::feature::definitions::FeatureDefinition {
    crate::feature::definitions::FeatureDefinition {
        identity: crate::feature::definitions::DefinitionIdentity::Parsed {
            schema_id: std::num::NonZeroU32::new(40),
            owner_feature_id: Some(40),
        },
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(crate::feature::definitions::test_support::with_points(
            crate::feature::definitions::FeatureVariableTable {
                declared_count: 0,
                entity_ref: None,
                rows: Vec::new(),
                offset: 0,
            },
            vec![
                crate::feature::definitions::FeatureSectionPoint {
                    point_id: 1,
                    u: Some(0.0),
                    v: Some(-1.0),
                },
                crate::feature::definitions::FeatureSectionPoint {
                    point_id: 2,
                    u: Some(0.0),
                    v: Some(1.0),
                },
            ],
        )),
        segments: Some(crate::feature::definitions::FeatureSegmentTable {
            declared_count: 1,
            has_elided_prototype: false,
            entity_ref: None,
            rows: (vec![crate::feature::definitions::FeatureSegment {
                kind: crate::feature::definitions::FeatureSegmentKind::Line([1, 2]),
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
        order_table: Some(crate::feature::definitions::FeatureOrderTable {
            declared_count: 1,
            has_prototype: false,
            entity_ref: None,
            rows: vec![crate::feature::definitions::FeatureOrderRow {
                external_id: 7,
                internal_id: 1,
                bitmask: 0,
                offset: 0,
            }],
            offset: 0,
        }),
        section_3d: Some(crate::feature::definitions::FeatureSection3d {
            sketch_plane_entity_id: None,
            sketch_plane_flip: None,
            reference_planes: crate::feature::definitions::ReferencePlanes::Named(Vec::new()),
            reference_plane_datum_geometry_id: None,
            orientation: crate::feature::definitions::FeatureSectionOrientation::default(),
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
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
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
        .push(crate::feature::operations::FeatureOperation {
            feature_id: 40,
            kind: crate::feature::operations::OperationKind::Revolve,
            name: crate::feature::operations::OperationName::Derived,
            recipe: crate::feature::operations::RecipeResolution::Resolved(
                crate::feature::operations::FeatureRecipe::ProtrudeRevolve,
            ),
            display_state_conflict: false,
            depdb: None,
            offset: 0,
            state_offset: 0,
        });
    scan.features
        .revolution_extents
        .push(crate::feature::rows::FeatureRevolutionExtent {
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
        placement: SketchPlacement::Unresolved {},
        profiles: SketchProfiles::try_from(vec![uses]).expect("profile"),
        native_ref: None,
    });
    let mut losses = Vec::new();
    let count = crate::decode::with_test_decode_ctx(|ctx| {
        transfer_resolved_revolution_breps(
            ctx,
            &scan,
            &mut ir,
            &mut AnnotationBuilder::new(),
            &mut losses,
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
    })
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

fn closed_off_axis_revolution() -> (crate::container::ContainerScan<'static>, CadIr) {
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    scan.features.definitions.push(definition());
    scan.features.section_transforms.push(
        crate::placement::FeatureSectionTransform::new(
            40, Some(40), [0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 0,
        ).expect("section frame"),
    );
    scan.features.operations.push(crate::feature::operations::FeatureOperation {
        feature_id: 40,
        kind: crate::feature::operations::OperationKind::Revolve,
        name: crate::feature::operations::OperationName::Derived,
        recipe: crate::feature::operations::RecipeResolution::Resolved(
            crate::feature::operations::FeatureRecipe::ProtrudeRevolve,
        ),
        display_state_conflict: false,
        depdb: None,
        offset: 0,
        state_offset: 0,
    });
    scan.features.revolution_extents.push(crate::feature::rows::FeatureRevolutionExtent {
        feature_id: 40, offset: 0,
    });
    let sketch_id = SketchId::mint("creo:model:sketch#40".to_string()).expect("sketch id");
    let mut ir = CadIr::empty();
    let mut uses = Vec::new();
    for (index, (start, end)) in [
        ([1.0, 0.0], [2.0, 0.0]),
        ([2.0, 0.0], [2.0, 1.0]),
        ([2.0, 1.0], [1.0, 1.0]),
        ([1.0, 1.0], [1.0, 0.0]),
    ].into_iter().enumerate() {
        let id = SketchEntityId::mint(format!("creo:featdefs:sketch_entity#40:{index}"))
            .expect("entity id");
        ir.model.sketch_entities.push(SketchEntity::new(
            id.clone(), sketch_id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(start[0], start[1]),
                end: Point2::new(end[0], end[1]),
            }).expect("line"),
        ));
        uses.push(SketchEntityUse { entity: id, reversed: false });
    }
    ir.model.sketches.push(Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved {},
        profiles: SketchProfiles::try_from(vec![uses]).expect("profile"),
        native_ref: None,
    });
    (scan, ir)
}

#[test]
fn closed_off_axis_revolution_reaches_brep_admission() {
    let (scan, mut ir) = closed_off_axis_revolution();
    let mut losses = Vec::new();
    let count = crate::decode::with_test_decode_ctx(|ctx| {
        transfer_resolved_revolution_breps(
            ctx, &scan, &mut ir, &mut AnnotationBuilder::new(), &mut losses,
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
    }).expect("service profile admits the closed revolution");
    assert_eq!(count, 1);
    assert_eq!(ir.model.bodies.len(), 1);
    assert!(losses.is_empty());
}

fn revolution_refuses_at_collection_boundary(operation: &'static str) {
    let mut last_refusal = None;
    for limit in 0..4096 {
        let (scan, mut ir) = closed_off_axis_revolution();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is admitted");
        let mut losses = Vec::new();
        let result = transfer_resolved_revolution_breps(
            &ctx, &scan, &mut ir, &mut AnnotationBuilder::new(), &mut losses,
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        );
        match result {
            Err(cadmpeg_core::CodecError::ResourceLimit(resource))
                if resource.dimension == ResourceDimension::CollectionItems
                    && resource.operation == operation => return,
            Err(cadmpeg_core::CodecError::ResourceLimit(resource)) => {
                last_refusal = Some((limit, resource.dimension, resource.operation));
            }
            Err(error) => panic!("unexpected revolution error at collection limit {limit}: {error:?}"),
            Ok(_) => panic!("revolution succeeded before the named {operation} refusal"),
        }
    }
    panic!("the named {operation} boundary was not reached; last refusal: {last_refusal:?}");
}

macro_rules! revolution_collection_limit_test {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            revolution_refuses_at_collection_boundary($operation);
        }
    };
}

revolution_collection_limit_test!(revolution_vertex_curves_refuse_limit, "creo revolution vertex curves");
revolution_collection_limit_test!(revolution_surface_geometries_refuse_limit, "creo revolution surface geometries");
revolution_collection_limit_test!(revolution_segment_boundaries_refuse_limit, "creo revolution segment boundaries");
revolution_collection_limit_test!(revolution_boundary_rows_refuse_limit, "creo revolution boundary rows");
revolution_collection_limit_test!(revolution_face_senses_refuse_limit, "creo revolution face senses");
revolution_collection_limit_test!(revolution_profile_edges_refuse_limit, "creo revolution profile edges");
revolution_collection_limit_test!(revolution_ring_coedges_refuse_limit, "creo revolution ring coedges");
revolution_collection_limit_test!(revolution_ring_validation_nodes_refuse_limit, "creo revolution loop validation nodes");
revolution_collection_limit_test!(revolution_loops_refuse_limit, "creo model revolution loops");
revolution_collection_limit_test!(revolution_coedge_pcurves_refuse_limit, "creo revolution coedge pcurves");
revolution_collection_limit_test!(revolution_face_loop_ids_refuse_limit, "creo revolution face loop IDs");
revolution_collection_limit_test!(revolution_shell_face_ids_refuse_limit, "creo revolution shell face IDs");
revolution_collection_limit_test!(revolution_shells_refuse_limit, "creo model revolution shells");
revolution_collection_limit_test!(revolution_region_shell_ids_refuse_limit, "creo revolution region shell IDs");
revolution_collection_limit_test!(revolution_regions_refuse_limit, "creo model revolution regions");
revolution_collection_limit_test!(revolution_body_region_ids_refuse_limit, "creo revolution body region IDs");
