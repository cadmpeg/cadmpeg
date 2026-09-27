// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    Feature, FeatureDefinition as IrFeatureDefinition, FeatureOperation as IrFeatureOperation,
};

use super::super::link::{
    link_feature_sketch_history, ordered_family_surface_bindings_for_feature, profile_segment_ids,
    section_entity_is_generated_profile,
};

fn section_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    scan.features
        .definitions
        .push(crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(7),
                owner_feature_id: None,
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: Some(crate::feature::definitions::FeatureSection3d {
                sketch_plane_entity_id: None,
                sketch_plane_flip: None,
                reference_planes: crate::feature::definitions::ReferencePlanes::Named(Vec::new()),
                reference_plane_datum_geometry_id: None,
                orientation: crate::feature::definitions::FeatureSectionOrientation::default(),
                dimension_ids: Vec::new(),
                offset: 20,
            }),
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 10,
        });
    scan.features.section_transforms.push(
        crate::placement::FeatureSectionTransform::new(
            7,
            Some(2),
            [0.0; 3],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            20,
        )
        .expect("valid section frame"),
    );
    scan
}

fn feature(id: &str) -> Feature {
    Feature {
        id: cadmpeg_ir::features::FeatureId::mint(id).expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            IrFeatureDefinition::Operation(IrFeatureOperation::Native {
                kind: "test".into(),
                parameters: BTreeMap::new(),
            }),
        ),
        native_ref: None,
    }
}

fn link_service(scan: &crate::container::ContainerScan<'_>, ir: &mut CadIr) {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    link_feature_sketch_history(&ctx, scan, ir).expect("service history link");
}

#[test]
fn history_link_rejects_duplicate_owner_feature_ids() {
    let scan = section_scan();
    let mut ir = CadIr::empty();
    ir.model.features.extend([
        feature("creo:model:feature#2"),
        feature("creo:model:feature#2"),
        feature("creo:model:sketch_feature#7"),
    ]);

    link_service(&scan, &mut ir);

    assert!(ir.model.features[..2]
        .iter()
        .all(|feature| feature.dependencies.is_empty()));
}

#[test]
fn history_link_rejects_duplicate_sketch_feature_ids() {
    let scan = section_scan();
    let mut ir = CadIr::empty();
    ir.model.features.extend([
        feature("creo:model:feature#2"),
        feature("creo:model:sketch_feature#7"),
        feature("creo:model:sketch_feature#7"),
    ]);

    link_service(&scan, &mut ir);

    assert!(ir.model.features[0].dependencies.is_empty());
}

#[test]
fn history_link_refuses_dependency_vector_before_growth() {
    let scan = section_scan();
    let mut ir = CadIr::empty();
    ir.model.features.extend([
        feature("creo:model:feature#2"),
        feature("creo:model:sketch_feature#7"),
    ]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");

    let error = link_feature_sketch_history(&ctx, &scan, &mut ir)
        .expect_err("one sketch dependency exceeds the limit");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo sketch history dependencies"));
    assert!(ir.model.features[0].dependencies.is_empty());
}

#[test]
fn history_link_preserves_service_profile_dependency() {
    let scan = section_scan();
    let mut ir = CadIr::empty();
    ir.model.features.extend([
        feature("creo:model:feature#2"),
        feature("creo:model:sketch_feature#7"),
    ]);
    let sketch_feature = ir.model.features[1].id.clone();

    link_service(&scan, &mut ir);

    assert_eq!(ir.model.features[0].dependencies.as_slice(), &[sketch_feature]);
}

#[test]
fn rowless_generated_profile_requires_a_framed_side_table() {
    let entry =
        |entity_id, class_id, source_entity_id| crate::feature::entity::FeatureEntityTableEntry {
            payload: crate::feature::entity::entry_payload(class_id, source_entity_id, None, None),

            entity_id,
            prefixed: false,
            offset: 0,
            end_offset: 0,
        };
    let table = crate::feature::entity::FeatureEntityTable::new(
        7,
        29,
        vec![
            entry(29, 204, None),
            entry(30, 203, None),
            entry(31, 200, Some(11)),
            entry(32, 200, Some(13)),
        ],
        &std::collections::BTreeSet::new(),
        0,
    )
    .with_surface_ids([29, 30, 32]);
    let row = |id| crate::surface::SurfaceRow {
        id,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 7,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    };
    let rows = vec![row(29), row(30), row(32)];
    assert!(section_entity_is_generated_profile(
        true,
        Some(7),
        11,
        &[crate::surface::SurfaceKind::Plane],
        std::slice::from_ref(&table),
        &rows,
    ));

    let mut malformed = table;
    malformed.entries.pop();
    assert!(!section_entity_is_generated_profile(
        true,
        Some(7),
        11,
        &[crate::surface::SurfaceKind::Plane],
        std::slice::from_ref(&malformed),
        &rows,
    ));
}

#[test]
fn rowless_generated_profile_rejects_duplicate_entity_ids() {
    let entry = |entity_id, class_id, source_entity_id| {
        crate::feature::entity::FeatureEntityTableEntry {
            payload: crate::feature::entity::entry_payload(class_id, source_entity_id, None, None),
            entity_id,
            prefixed: false,
            offset: 0,
            end_offset: 0,
        }
    };
    let mut table = crate::feature::entity::FeatureEntityTable::new(
        7,
        29,
        vec![
            entry(29, 204, None),
            entry(30, 203, None),
            entry(31, 200, Some(11)),
            entry(32, 200, Some(13)),
        ],
        &std::collections::BTreeSet::new(),
        0,
    )
    .with_surface_ids([29, 30, 32]);
    let row = |id| crate::surface::SurfaceRow {
        id,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 7,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    };
    let rows = [row(29), row(30), row(32)];
    assert!(section_entity_is_generated_profile(
        true,
        Some(7),
        11,
        &[crate::surface::SurfaceKind::Plane],
        std::slice::from_ref(&table),
        &rows,
    ));
    table.entries[0].entity_id = 30;
    let table = table.with_surface_ids([30, 32]);
    assert!(!section_entity_is_generated_profile(
        true,
        Some(7),
        11,
        &[crate::surface::SurfaceKind::Plane],
        &[table],
        &rows,
    ));
}

fn ordered_binding_fixture() -> (
    crate::feature::entity::FeatureEntityTable,
    crate::feature::definitions::FeatureOrderTable,
    [crate::surface::SurfaceRow; 1],
) {
    let table = crate::feature::entity::FeatureEntityTable::new(
        17,
        100,
        vec![crate::feature::entity::FeatureEntityTableEntry {
            entity_id: 43,
            payload: crate::feature::entity::entry_payload(200, Some(9), None, None),
            prefixed: false,
            offset: 0,
            end_offset: 0,
        }],
        &std::collections::BTreeSet::new(),
        0,
    )
    .with_surface_ids([43]);
    let order = crate::feature::definitions::FeatureOrderTable {
        declared_count: 1,
        has_prototype: false,
        entity_ref: Some(3),
        rows: vec![crate::feature::definitions::FeatureOrderRow {
            external_id: 9,
            internal_id: 1,
            bitmask: 0,
            offset: 0,
        }],
        offset: 0,
    };
    let rows = [crate::surface::SurfaceRow {
        id: 43,
        kind: crate::surface::SurfaceKind::TorusOrSphere,
        feature_id: 17,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    }];
    (table, order, rows)
}

fn ordered_binding_limit_error(limit: u64, operation: &'static str) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let (table, order, rows) = ordered_binding_fixture();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let error = ordered_family_surface_bindings_for_feature(
        &ctx,
        &rows,
        17,
        &[table],
        &order,
        [9],
        crate::surface::SurfaceKind::TorusOrSphere,
    )
    .expect_err("one generated surface binding exceeds the collection limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == operation), "{error:?}");
}

#[test]
fn ordered_generated_surface_ids_refuse_collection_limit() {
    ordered_binding_limit_error(0, "creo bound generated surface IDs");
}

#[test]
fn ordered_generated_surface_bindings_refuse_collection_limit() {
    ordered_binding_limit_error(1, "creo ordered generated surface bindings");
}

#[test]
fn ordered_generated_surface_binding_keeps_identity_under_service_policy() {
    let (table, order, rows) = ordered_binding_fixture();
    let bindings = crate::decode::with_test_decode_ctx(|ctx| {
        ordered_family_surface_bindings_for_feature(
            ctx,
            &rows,
            17,
            &[table],
            &order,
            [9],
            crate::surface::SurfaceKind::TorusOrSphere,
        )
    })
    .expect("service profile admits generated surface binding");
    assert_eq!(bindings, BTreeMap::from([(9, 43)]));
}

#[test]
fn profile_segment_ids_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let segment = crate::feature::definitions::FeatureSegment {
        kind: crate::feature::definitions::FeatureSegmentKind::Line([1, 2]),
        directions: [None; 3],
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 9,
        body: Vec::new(),
        offset: 0,
    };
    let profiles = [vec![cadmpeg_ir::sketches::SketchEntityUse {
        entity: cadmpeg_ir::sketches::SketchEntityId::mint(
            "creo:featdefs:sketch_entity#2:9".to_string(),
        )
        .expect("profile entity identity"),
        reversed: false,
    }]];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let error = profile_segment_ids(&ctx, 2, &[&segment], &profiles)
        .expect_err("one matched profile ID exceeds the collection limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == "creo profile segment ID nodes"), "{error:?}");
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| profile_segment_ids(ctx, 2, &[&segment], &profiles))
            .expect("service profile admits one profile ID"),
        std::collections::BTreeSet::from([9]),
    );
}
