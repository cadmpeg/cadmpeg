// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{Feature, FeatureDefinition as IrFeatureDefinition};
use cadmpeg_ir::units::Units;

use super::super::link_feature_sketch_history;

fn section_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features
        .definitions
        .push(crate::feature::FeatureDefinition {
            id: 7,
            owner_feature_id: None,
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
                offset: 20,
            }),
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 10,
        });
    scan.features
        .section_transforms
        .push(crate::placement::FeatureSectionTransform {
            definition_id: 7,
            feature_id: Some(2),
            origin: [0.0; 3],
            u_axis: [1.0, 0.0, 0.0],
            v_axis: [0.0, 1.0, 0.0],
            normal: [0.0, 0.0, 1.0],
            offset: 20,
        });
    scan
}

fn feature(id: &str) -> Feature {
    Feature::new(
        id.into(),
        0,
        IrFeatureDefinition::Native {
            kind: "test".to_string(),
            parameters: BTreeMap::new(),
            properties: BTreeMap::new(),
        },
    )
}

#[test]
fn history_link_rejects_duplicate_owner_feature_ids() {
    let scan = section_scan();
    let mut ir = CadIr::empty(Units::default());
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
    let mut ir = CadIr::empty(Units::default());
    ir.model.features.extend([
        feature("creo:model:feature#2"),
        feature("creo:model:sketch_feature#7"),
        feature("creo:model:sketch_feature#7"),
    ]);

    link_feature_sketch_history(&scan, &mut ir);

    assert!(ir.model.features[0].dependencies.is_empty());
}
