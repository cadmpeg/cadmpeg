// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{Feature, FeatureDefinition as IrFeatureDefinition};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::AnnotationBuilder;

use super::super::transfer_feature_dimensions;

#[test]
fn dimension_transfer_rejects_duplicate_owner_feature_ids() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features.rows.push(crate::feature::FeatureRow {
        feature_id: 40,
        header: [0xeb, 0x04],
        root_schema_class: Some(926),
        stream_offset: 0,
        body: vec![0; 20],
        body_offset: 0,
        offset: 0,
    });
    scan.features
        .definitions
        .push(crate::feature::FeatureDefinition {
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
            dimensions: Some(crate::feature::FeatureDimensionTable {
                declared_count: 1,
                entity_ref: None,
                rows: vec![crate::feature::FeatureDimension {
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
                }],
                offset: 9,
            }),
            relations: None,
            saved_section: None,
            offset: 8,
        });

    let mut ir = CadIr::empty(Units::default());
    for ordinal in 0..2 {
        ir.model.features.push(Feature::new(
            "creo:model:feature#40".into(),
            ordinal,
            IrFeatureDefinition::Native {
                kind: "test".to_string(),
                parameters: BTreeMap::new(),
                properties: BTreeMap::new(),
            },
        ));
    }

    let (transferred, _) =
        transfer_feature_dimensions(&scan, &mut ir, &mut AnnotationBuilder::new());

    assert_eq!(transferred, 1);
    assert!(ir
        .model
        .features
        .iter()
        .all(|feature| feature.source_content.is_empty()));
}
