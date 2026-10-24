// SPDX-License-Identifier: Apache-2.0

use crate::test_support::blend_history::primary_partition;
use crate::test_support::test_bytes::zlib_compress;
use crate::test_support::test_om::composed_feature_history_section;
use crate::test_support::test_prt::prt_with_named_payloads;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::edge_treatments::RadiusSpec;
use cadmpeg_ir::features::{EdgeSelection, FaceSelection, FeatureDefinition, FeatureOperation};
use std::io::Cursor;

#[test]
fn historical_blends_share_a_body_without_inheriting_its_carriers() {
    let operations = ["BLEND", "BLEND", "FACE_BLEND", "BLEND"]
        .into_iter()
        .enumerate()
        .map(|(index, name)| {
            let group = 49 + u8::try_from(index).unwrap();
            (
                [0xff; 4],
                name,
                vec![
                    0x01, 0x02, 0x11, group, 0x97, 0x75, 0x01, 0x02, 0x10, 0x41, 0xff,
                ],
            )
        })
        .collect::<Vec<_>>();
    let borrowed = operations
        .iter()
        .map(|(inputs, name, frame)| (inputs.as_slice(), *name, frame.clone()))
        .collect::<Vec<_>>();
    let history = composed_feature_history_section(&borrowed);
    let mut partition = primary_partition();

    for (identifier, definition, name, fields) in [
        (691_u16, 692_u16, "UG2/CMOD_FEAT_ATTRIBUTE", &[0_u8][..]),
        (701, 702, "UG2/CMOD_FACR_ATTRIBUTE", &[0, 1][..]),
    ] {
        partition.extend_from_slice(&79_u16.to_be_bytes());
        partition.extend_from_slice(&u32::try_from(name.len()).unwrap().to_be_bytes());
        partition.extend_from_slice(&identifier.to_be_bytes());
        partition.extend_from_slice(name.as_bytes());
        partition.extend_from_slice(&80_u16.to_be_bytes());
        partition.extend_from_slice(&u32::try_from(fields.len()).unwrap().to_be_bytes());
        for reference in [definition, 1, identifier] {
            partition.extend_from_slice(&reference.to_be_bytes());
        }
        partition.extend_from_slice(&9000_u32.to_be_bytes());
        partition.extend_from_slice(&[0; 8]);
        partition.extend_from_slice(&1_u16.to_be_bytes());
        partition.extend_from_slice(&[0; 14]);
        partition.extend_from_slice(fields);
    }
    for (index, key) in [709_u32, 624, 464, 565].into_iter().enumerate() {
        let index = u16::try_from(index).unwrap();
        let group = 800 + index;
        let attribute = 500 + index;
        partition.extend_from_slice(&90_u16.to_be_bytes());
        partition.extend_from_slice(&group.to_be_bytes());
        partition.extend_from_slice(&u32::from(49 + index).to_be_bytes());
        for reference in [attribute, 1, 1, 1] {
            partition.extend_from_slice(&reference.to_be_bytes());
        }
        partition.push(4);
        partition.extend_from_slice(&1_u16.to_be_bytes());
        partition.extend_from_slice(&81_u16.to_be_bytes());
        partition.extend_from_slice(&1_u32.to_be_bytes());
        partition.extend_from_slice(&attribute.to_be_bytes());
        partition.extend_from_slice(&u32::from(attribute).to_be_bytes());
        for reference in [692, group, 1, 1, 1, 1, 600 + index] {
            partition.extend_from_slice(&reference.to_be_bytes());
        }
        partition.extend_from_slice(&82_u16.to_be_bytes());
        partition.extend_from_slice(&1_u32.to_be_bytes());
        partition.extend_from_slice(&(600 + index).to_be_bytes());
        partition.extend_from_slice(&key.to_be_bytes());
    }
    partition.extend_from_slice(&81_u16.to_be_bytes());
    partition.extend_from_slice(&2_u32.to_be_bytes());
    partition.extend_from_slice(&710_u16.to_be_bytes());
    partition.extend_from_slice(&710_u32.to_be_bytes());
    for reference in [702_u16, 4, 1, 1, 1, 1, 711, 712] {
        partition.extend_from_slice(&reference.to_be_bytes());
    }
    for (identity, value) in [(711_u16, 709_u32), (712, 1)] {
        partition.extend_from_slice(&82_u16.to_be_bytes());
        partition.extend_from_slice(&1_u32.to_be_bytes());
        partition.extend_from_slice(&identity.to_be_bytes());
        partition.extend_from_slice(&value.to_be_bytes());
    }
    partition.extend_from_slice(&[0, 1, 0, 1]);
    let compressed = zlib_compress(&partition);
    let index_byte_len = 48_u32;
    let wrapper_offset = index_byte_len + u32::try_from(history.len()).unwrap();
    let mut payload = [
        index_byte_len,
        9,
        11,
        1,
        1,
        index_byte_len,
        wrapper_offset,
        0,
        17,
        65,
        19,
        0,
    ]
    .into_iter()
    .flat_map(u32::to_le_bytes)
    .collect::<Vec<_>>();
    payload.extend(history);
    payload.extend_from_slice(&0x8000_0000_u32.to_le_bytes());
    payload.extend_from_slice(&u32::try_from(compressed.len()).unwrap().to_le_bytes());
    payload.extend(compressed);
    let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)]);
    let result = crate::NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("four historical body writers");
    let ir = result.ir();
    assert_eq!(ir.model.bodies.len(), 1);
    let initial_body = ir
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "nx:feature-history:feature#initial-bodies")
        .expect("one retained-history body input");
    assert_eq!(
        initial_body.evaluation.outputs(),
        &[ir.model.bodies[0].id.clone()]
    );
    let historical = ir
        .model
        .features
        .iter()
        .filter(|feature| feature.id != initial_body.id)
        .collect::<Vec<_>>();
    assert_eq!(historical.len(), 4);
    assert!(!ir.model.procedural_surfaces.is_empty());
    for feature in historical {
        assert_eq!(
            feature.evaluation.outputs(),
            initial_body.evaluation.outputs()
        );
        assert!(!feature
            .source_properties
            .keys()
            .any(|key| key.as_str().starts_with("blend_result_surface.")));
        match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) => {
                assert!(matches!(groups.as_slice(), [group]
                    if matches!(group.edges, EdgeSelection::Unresolved)
                        && matches!(group.radius, RadiusSpec::Unresolved { form: None })));
            }
            FeatureDefinition::Operation(FeatureOperation::FaceBlend { operands, radius }) => {
                assert!(matches!(operands.first_faces(), FaceSelection::Unresolved));
                assert!(matches!(operands.second_faces(), FaceSelection::Unresolved));
                assert!(matches!(radius, RadiusSpec::Unresolved { form: None }));
            }
            other => panic!("body-writing blend family: {other:?}"),
        }
    }
    let native = ir.native.namespace("nx").unwrap();
    let arenas = native.arenas();
    assert_eq!(arenas["feature_body_write_group_partition_uses"].len(), 4);
    let integers = serde_json::to_value(&arenas["parasolid_entity_52_integer_records"]).unwrap();
    for key in [709, 624, 464, 565] {
        assert!(integers
            .as_array()
            .unwrap()
            .iter()
            .any(|record| record["values"] == serde_json::json!([key])));
    }
    let entities = serde_json::to_value(&arenas["parasolid_entity_51_records"]).unwrap();
    assert!(entities
        .as_array()
        .unwrap()
        .iter()
        .any(|record| record["definition_xmt"] == 702
            && record["leading_references"][0] == 4
            && record["trailing_references"] == serde_json::json!([711, 712])));
    assert!(!arenas["parasolid_topology_attribute_class_uses"].is_empty());
    assert!(!arenas["parasolid_entity_51_numeric_uses"].is_empty());
}
