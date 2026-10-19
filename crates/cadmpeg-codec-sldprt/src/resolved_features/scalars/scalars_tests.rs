//! Tests for the `scalars` module.

use super::super::names::object_names;
use super::super::{COMPACT_SCALAR_HEADER, NAME_MARKER, SCALAR_HEADER, VALUE_ONLY_SCALAR_HEADER};
use super::named_scalars;
use crate::records::operand_tag::NativeOperandTag;
use crate::records::FeatureInputOperandKind;

fn decoded_names(payload: &[u8]) -> Vec<crate::records::FeatureInputName> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(payload, &arena, &policy)
        .expect("test context");
    object_names(&ctx, payload, "lane").expect("compute the object names")
}

#[test]
fn named_scalar_identity_decodes_record_fields() {
    let mut payload = Vec::new();
    payload.extend_from_slice(NAME_MARKER);
    payload.push(2);
    for unit in "D1".encode_utf16() {
        payload.extend_from_slice(&unit.to_le_bytes());
    }
    payload.extend_from_slice(SCALAR_HEADER);
    payload.extend_from_slice(&0.025f64.to_le_bytes());
    let trailer = payload.len();
    payload.resize(trailer + 7, 0);
    payload[trailer + 3..trailer + 7].copy_from_slice(&42u32.to_le_bytes());
    let names = decoded_names(&payload);
    assert_eq!(names.len(), 1);

    assert_eq!(
        named_scalars(&payload, "lane", &names).unwrap(),
        vec![crate::records::FeatureInputScalar {
            id: "sldprt:feature-input:scalar#lane:32".into(),
            parent: "lane".into(),
            feature_ref: None,
            ordinal: 0,
            offset: 32,
            object_id: 42,
            name: names[0].id.clone(),
            value: cadmpeg_ir::scalar::FiniteReal::new(0.025).unwrap(),
            role: crate::records::FeatureInputScalarRole::Native,
            operands: Vec::new(),
        }]
    );
}

#[test]
fn scalar_trailer_is_relative_to_variable_length_name() {
    let mut payload = Vec::new();
    payload.extend_from_slice(NAME_MARKER);
    payload.push(3);
    for unit in "D10".encode_utf16() {
        payload.extend_from_slice(&unit.to_le_bytes());
    }
    payload.extend_from_slice(SCALAR_HEADER);
    payload.extend_from_slice(&0.025f64.to_le_bytes());
    let trailer = payload.len();
    payload.resize(trailer + 59, 0);
    payload[trailer + 3..trailer + 7].copy_from_slice(&42u32.to_le_bytes());
    payload[trailer + 24..trailer + 29].copy_from_slice(&[0, 0, 0, 2, 0]);
    for (relative, index) in [(35usize, 7u16), (47, 9)] {
        payload[trailer + relative..trailer + relative + 2].copy_from_slice(&[0xd6, 0x80]);
        payload[trailer + relative + 2..trailer + relative + 4]
            .copy_from_slice(&index.to_le_bytes());
        payload[trailer + relative + 4..trailer + relative + 8].fill(0xff);
    }
    let names = decoded_names(&payload);
    let scalars = named_scalars(&payload, "lane", &names).unwrap();
    let [scalar] = scalars.as_slice() else {
        panic!("expected one scalar");
    };
    assert_eq!(scalar.object_id, 42);
    assert_eq!(scalar.role, crate::records::FeatureInputScalarRole::Driving);
    assert_eq!(scalar.entity_indices(), [7, 9]);
}

#[test]
fn compact_scalar_header_ends_at_the_value() {
    let mut payload = Vec::new();
    payload.extend_from_slice(NAME_MARKER);
    payload.push(2);
    for unit in "D1".encode_utf16() {
        payload.extend_from_slice(&unit.to_le_bytes());
    }
    payload.extend_from_slice(COMPACT_SCALAR_HEADER);
    payload.extend_from_slice(&0.0f64.to_le_bytes());
    let trailer = payload.len();
    payload.resize(trailer + 51, 0);
    payload[trailer + 3..trailer + 7].copy_from_slice(&115u32.to_le_bytes());
    payload[trailer + 21..trailer + 27].copy_from_slice(&[1, 0, 0, 0, 2, 0]);
    payload[trailer + 27] = 0;
    payload[trailer + 35..trailer + 37].copy_from_slice(&0x8152u16.to_le_bytes());
    payload[trailer + 37..trailer + 39].copy_from_slice(&7u16.to_le_bytes());
    payload[trailer + 39..trailer + 43].fill(0xff);
    payload[trailer + 43..trailer + 45].copy_from_slice(&0x8152u16.to_le_bytes());
    payload[trailer + 45..trailer + 47].copy_from_slice(&9u16.to_le_bytes());
    payload[trailer + 47..trailer + 51].fill(0xff);

    let names = decoded_names(&payload);
    let scalars = named_scalars(&payload, "lane", &names).unwrap();
    let [scalar] = scalars.as_slice() else {
        panic!("expected one scalar");
    };
    assert_eq!(scalar.value.get(), 0.0);
    assert_eq!(scalar.object_id, 115);
    assert_eq!(scalar.role, crate::records::FeatureInputScalarRole::Driving);
    assert!(scalar.entity_indices().is_empty());
    assert_eq!(
        scalar
            .operands
            .iter()
            .map(|operand| (operand.kind, operand.entity_index))
            .collect::<Vec<_>>(),
        [
            (
                FeatureInputOperandKind::Native(NativeOperandTag::TAG_8152),
                7
            ),
            (
                FeatureInputOperandKind::Native(NativeOperandTag::TAG_8152),
                9
            ),
        ]
    );
}

#[test]
fn value_only_scalar_header_ends_at_the_value() {
    let mut payload = Vec::new();
    payload.extend_from_slice(NAME_MARKER);
    payload.push(2);
    for unit in "D1".encode_utf16() {
        payload.extend_from_slice(&unit.to_le_bytes());
    }
    payload.extend_from_slice(VALUE_ONLY_SCALAR_HEADER);
    payload.extend_from_slice(&0.0f64.to_le_bytes());
    let trailer = payload.len();
    payload.resize(trailer + 24, 0);
    payload[trailer + 3..trailer + 7].copy_from_slice(&132u32.to_le_bytes());

    let names = decoded_names(&payload);
    let scalars = named_scalars(&payload, "lane", &names).unwrap();
    let [scalar] = scalars.as_slice() else {
        panic!("expected one scalar");
    };
    assert_eq!(scalar.value.get(), 0.0);
    assert_eq!(scalar.object_id, 132);
    assert_eq!(scalar.role, crate::records::FeatureInputScalarRole::Native);
    assert!(scalar.operands.is_empty());
}

#[test]
fn legacy_scalar_layout_carries_shifted_role_and_operand() {
    let mut payload = Vec::new();
    payload.extend_from_slice(NAME_MARKER);
    payload.push(2);
    for unit in "D1".encode_utf16() {
        payload.extend_from_slice(&unit.to_le_bytes());
    }
    payload.extend_from_slice(SCALAR_HEADER);
    payload.extend_from_slice(&0.004f64.to_le_bytes());
    let trailer = payload.len();
    payload.resize(trailer + 48, 0);
    payload[trailer + 3..trailer + 7].copy_from_slice(&28u32.to_le_bytes());
    payload[trailer + 24..trailer + 30].copy_from_slice(&[0x0f, 0, 0, 0, 2, 0]);
    payload[trailer + 30] = 0;
    payload[trailer + 36..trailer + 38].copy_from_slice(&[0xcc, 0x80]);
    payload[trailer + 38..trailer + 40].copy_from_slice(&0u16.to_le_bytes());
    payload[trailer + 40..trailer + 44].fill(0xff);

    let names = decoded_names(&payload);
    let scalars = named_scalars(&payload, "lane", &names).unwrap();
    let [scalar] = scalars.as_slice() else {
        panic!("expected one scalar");
    };
    assert_eq!(scalar.role, crate::records::FeatureInputScalarRole::Driving);
    assert_eq!(scalar.operands.len(), 1);
    assert_eq!(
        scalar.operands[0].offset,
        cadmpeg_core::decode::u64_from_index(trailer + 36)
    );
    assert_eq!(
        scalar.operands[0].kind,
        crate::records::FeatureInputOperandKind::Native(NativeOperandTag::TAG_80CC)
    );
    assert_eq!(scalar.operands[0].entity_index, 0);
}

#[test]
fn shifted_value_only_scalar_carries_standard_operand_cells() {
    let mut payload = Vec::new();
    payload.extend_from_slice(NAME_MARKER);
    payload.push(2);
    for unit in "D1".encode_utf16() {
        payload.extend_from_slice(&unit.to_le_bytes());
    }
    payload.extend_from_slice(VALUE_ONLY_SCALAR_HEADER);
    payload.extend_from_slice(&[0; 4]);
    let value_offset = payload.len();
    payload.extend_from_slice(&0.045f64.to_le_bytes());
    let trailer = payload.len();
    payload.resize(trailer + 35 + 2 * 12, 0);
    payload[trailer + 3..trailer + 7].copy_from_slice(&70u32.to_le_bytes());
    payload[trailer + 21..trailer + 27].copy_from_slice(&[1, 0, 0, 0, 2, 0]);
    payload[trailer + 27] = 0;
    for (relative, index) in [(35usize, 0u16), (47, 1)] {
        payload[trailer + relative..trailer + relative + 2]
            .copy_from_slice(&0x81b2u16.to_le_bytes());
        payload[trailer + relative + 2..trailer + relative + 4]
            .copy_from_slice(&index.to_le_bytes());
        payload[trailer + relative + 4..trailer + relative + 8].fill(0xff);
    }

    let names = decoded_names(&payload);
    let scalars = named_scalars(&payload, "lane", &names).unwrap();
    let [scalar] = scalars.as_slice() else {
        panic!("expected one scalar");
    };
    assert_eq!(
        scalar.offset,
        cadmpeg_core::decode::u64_from_index(value_offset)
    );
    assert_eq!(scalar.object_id, 70);
    assert_eq!(scalar.role, crate::records::FeatureInputScalarRole::Driving);
    assert!(scalar.entity_indices().is_empty());
    assert_eq!(
        scalar
            .operands
            .iter()
            .map(|operand| (operand.kind, operand.entity_index))
            .collect::<Vec<_>>(),
        [
            (
                FeatureInputOperandKind::Native(NativeOperandTag::TAG_81B2),
                0
            ),
            (
                FeatureInputOperandKind::Native(NativeOperandTag::TAG_81B2),
                1
            ),
        ]
    );
}

#[test]
fn indexed_feature_names_preserve_precedence_and_ambiguity() {
    use super::FeatureObjectNames;
    use crate::records::{FeatureInputName, ObjectId};

    let mut names = (0u32..1000)
        .map(|index| FeatureInputName { feature_ref: None,
            id: format!("name#{index}"),
            parent: "lane".into(),
            ordinal: index, sequence: index,
            offset: Some(u64::from(index)),
            object_id: ObjectId::from_value(index + 1),
            value: format!("F{index}"),
        })
        .collect::<Vec<_>>();
    names.push(names[0].clone());
    let indexed = FeatureObjectNames::new(&names);
    assert!(indexed.get(Some(1), "F1").is_none());
    assert!(indexed.get(None, "F0").is_none());
    assert_eq!(indexed.get(Some(1001), "F1").unwrap().id, "name#1");
    for index in 1u32..1000 {
        let name = indexed.get(Some(index + 1), "F0").unwrap();
        assert_eq!(name.id, format!("name#{index}"));
    }
}

#[test]
fn indexed_feature_objects_match_the_linear_scans() {
    use super::{feature_object_name, FeatureObjectNames, FeatureObjectStarts};
    use crate::records::{
        Feature, FeatureHistory, FeatureInputLane, FeatureInputName, FeatureSource, ObjectId,
    };
    use std::collections::BTreeMap;

    const PAYLOAD_LEN: usize = 2_100;
    let mut state = 0x2545_f491_u64;
    let mut next = |bound: u64| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        u32::try_from((state >> 33) % bound).unwrap()
    };
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: vec![0; PAYLOAD_LEN].into(),
        classes: Vec::new(),
        names: (0u32..400)
            .map(|index| FeatureInputName { feature_ref: None,
                id: format!("name#{index}"),
                parent: "lane".into(),
                ordinal: index, sequence: index,
                offset: Some(u64::from(next(2_000))),
                object_id: if next(4) == 0 {
                    None
                } else {
                    ObjectId::from_value(next(600))
                },
                value: format!("F{}", next(400)),
            })
            .collect(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };
    let mut features = (0u32..300)
        .map(|index| Feature {
            id: format!("feature#{index}"),
            parent: "history".into(),
            xml_tag: "Feature".into(),
            tree_parent: None,
            source_id: match next(4) {
                0 => None,
                1 => Some(FeatureSource::Reserved),
                _ => FeatureSource::from_value(next(800)),
            },
            ordinal: index,
            name: format!("F{}", next(500)),
            kind: "Feature".into(),
            input_class: None,
            suppressed: false,
            parameters: BTreeMap::new(),
            dimension_properties: BTreeMap::new(),
            properties: BTreeMap::new(),
            text: None,
            content: Vec::new(),
        })
        .collect::<Vec<_>>();
    let history = |id: &str, features| FeatureHistory {
        id: id.into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features,
    };
    let second = features.split_off(150);
    let histories = [history("first", features), history("second", second)];

    let names = FeatureObjectNames::new(&lane.names);
    let starts = FeatureObjectStarts::new(&histories, &names);
    let mut found = 0;
    for feature in histories.iter().flat_map(|history| &history.features) {
        let linear = feature_object_name(feature, &lane).map(|name| name.id.as_str());
        assert_eq!(names.of(feature).map(|name| name.id.as_str()), linear);
        found += usize::from(linear.is_some());
    }
    assert!(found > 0 && found < 300);
    let probes = (0..200).map(|_| u64::from(next(2_100))).collect::<Vec<_>>();
    for start in lane.names.iter().map(|name| name.offset).chain(probes) {
        let linear = histories
            .iter()
            .flat_map(|history| &history.features)
            .filter_map(|feature| feature_object_name(feature, &lane))
            .filter(|candidate| candidate.offset > start)
            .map(|candidate| candidate.offset)
            .min()
            .and_then(|offset| usize::try_from(offset).ok())
            .unwrap_or(PAYLOAD_LEN);
        assert_eq!(starts.end(start, PAYLOAD_LEN), linear);
    }
}
