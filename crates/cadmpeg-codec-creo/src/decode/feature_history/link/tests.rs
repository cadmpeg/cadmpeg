// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{Feature, FeatureDefinition as IrFeatureDefinition};

use super::super::{link_feature_sketch_history, section_entity_is_generated_profile};

fn section_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features
        .definitions
        .push(crate::feature::FeatureDefinition {
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
            section_3d: Some(crate::feature::FeatureSection3d {
                sketch_plane_entity_id: None,
                sketch_plane_flip: None,
                reference_planes: crate::feature::definitions::ReferencePlanes::Named(Vec::new()),
                reference_plane_datum_geometry_id: None,
                orientation: crate::feature::FeatureSectionOrientation::default(),
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
    Feature::new(
        cadmpeg_ir::features::FeatureId::mint(id).expect("identity grammar"),
        0,
        IrFeatureDefinition::Native {
            kind: "test".into(),
            parameters: BTreeMap::new(),
        },
    )
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

    link_feature_sketch_history(&scan, &mut ir);

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

    link_feature_sketch_history(&scan, &mut ir);

    assert!(ir.model.features[0].dependencies.is_empty());
}

#[test]
fn rowless_generated_profile_requires_a_framed_side_table() {
    let entry = |entity_id, class_id, source_entity_id| crate::feature::FeatureEntityTableEntry {
        payload: crate::feature::entry_payload(class_id, source_entity_id, None, None),

        entity_id,
        prefixed: false,
        offset: 0,
        end_offset: 0,
    };
    let table = crate::feature::FeatureEntityTable::new(
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
