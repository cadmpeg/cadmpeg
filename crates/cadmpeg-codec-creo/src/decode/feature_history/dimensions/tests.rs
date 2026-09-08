// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{Feature, FeatureDefinition as IrFeatureDefinition, ParameterId};
use cadmpeg_ir::AnnotationBuilder;

use super::super::{planned_feature_dimension_parameter_ids, transfer_feature_dimensions};

#[test]
fn dimension_transfer_rejects_duplicate_owner_feature_ids() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features.rows.push(crate::feature::FeatureRow {
        feature_id: 40,
        root_schema_class: Some(crate::feature::schema::SchemaClass::Section),
        stream_offset: 0,
        body: vec![0; 20].try_into().expect("row body"),
        body_offset: 0,
        offset: 0,
    });
    scan.features
        .definitions
        .push(crate::feature::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(917),
                owner_feature_id: Some(40),
            },
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
                    value: crate::feature::definitions::DimensionValue::Resolved(5.0),
                    value_body: Vec::new(),
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

    assert_eq!(
        planned_feature_dimension_parameter_ids(&scan),
        BTreeSet::from([
            ParameterId::mint("creo:featdefs:parameter#917:3".to_string())
                .expect("identity grammar")
        ])
    );

    let mut ir = CadIr::empty();
    for ordinal in 0..2 {
        ir.model.features.push(Feature::new(
            cadmpeg_ir::features::FeatureId::mint("creo:model:feature#40")
                .expect("identity grammar"),
            ordinal,
            IrFeatureDefinition::Native {
                kind: "test".into(),
                parameters: BTreeMap::new(),
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
