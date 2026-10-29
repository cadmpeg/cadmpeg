// SPDX-License-Identifier: Apache-2.0
//! Native ownership and precedence regressions.
#![allow(clippy::unwrap_used)]

use super::{body_identity, merge, Choice};
use crate::appearance::AppearanceDefinition;
use crate::layout::visual_component_body_prefix as prefix;
use cadmpeg_ir::ids::FaceId;
use cadmpeg_ir::topology::Color;
use std::collections::BTreeMap;

#[test]
fn body_scalar_requires_the_complete_duplicated_component_identity() {
    let mut bytes = vec![0; prefix::LEN];
    bytes[prefix::CLASS_TOKEN..prefix::CLASS_TOKEN + 2].copy_from_slice(&0x8023_u16.to_le_bytes());
    bytes[prefix::IDENTITY..prefix::IDENTITY + 4].copy_from_slice(&907_u32.to_le_bytes());
    bytes[prefix::IDENTITY_COPY..prefix::IDENTITY_COPY + 4].copy_from_slice(&907_u32.to_le_bytes());
    assert_eq!(body_identity(&bytes), Some(907));
    for at in [prefix::ZERO_PADDING, prefix::IDENTITY_COPY] {
        let mut changed = bytes.clone();
        changed[at] ^= 1;
        assert_eq!(body_identity(&changed), None);
    }
    for n in 0..prefix::LEN {
        assert_eq!(body_identity(&bytes[..n]), None);
    }
    bytes[prefix::IDENTITY..prefix::IDENTITY + 4].fill(0);
    bytes[prefix::IDENTITY_COPY..prefix::IDENTITY_COPY + 4].fill(0);
    assert_eq!(body_identity(&bytes), None);
}

fn choice(rank: u8, rgb: Option<[u8; 3]>) -> Choice {
    Choice {
        rank,
        ordinal: 8,
        definition: rgb.map(|rgb| AppearanceDefinition {
            name: "Scoped test appearance".into(),
            color: Color::from_rgba8(rgb[0], rgb[1], rgb[2], 255),
            source_name: cadmpeg_ir::StreamName::try_from("ThirdPtyStore/VisualStates".to_owned())
                .unwrap(),
            record_offset: 12,
        }),
    }
}

#[test]
fn higher_scope_and_conflicting_equal_scope_remain_unresolved_in_both_orders() {
    let face = FaceId::mint("sldprt:brep:face#17").unwrap();
    for inputs in [
        [choice(1, Some([240, 240, 240])), choice(3, None)],
        [choice(3, None), choice(1, Some([240, 240, 240]))],
        [choice(3, Some([25, 50, 75])), choice(3, Some([75, 50, 25]))],
        [choice(3, Some([75, 50, 25])), choice(3, Some([25, 50, 75]))],
    ] {
        let mut choices = BTreeMap::new();
        for input in inputs {
            merge(&mut choices, face.clone(), input);
        }
        assert_eq!(choices[&face].rank, 3);
        assert!(choices[&face].definition.is_none());
    }
}

#[test]
fn feature_scope_keeps_distinct_four_word_owners_and_requires_unique_history_object() {
    use crate::brep::attrib::NativeIdentity;
    use crate::records::{FeatureHistory, FeatureInputLane, FeatureInputName, ObjectId};
    let feature: crate::records::Feature = serde_json::from_value(serde_json::json!({
        "id":"history:corner", "parent":"history", "source_id":"47", "ordinal":0,
        "name":"Corner", "kind":"Fillet"
    }))
    .unwrap();
    let mut histories = vec![FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![feature.clone()],
    }];
    let mut lanes = vec![FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: vec![0; 4].into(),
        classes: Vec::new(),
        names: vec![FeatureInputName {
            id: "name".into(),
            parent: "lane".into(),
            feature_ref: None,
            ordinal: 0,
            sequence: 0,
            offset: 0,
            object_id: ObjectId::from_value(47),
            value: "Corner".into(),
        }],
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    }];
    let faces: Vec<_> = [17, 23]
        .into_iter()
        .map(|owner| NativeIdentity {
            target: FaceId::mint(format!("sldprt:brep:face#{owner}")).unwrap(),
            values: vec![36, 47, 0x6523_4178, 0],
        })
        .collect();
    let mut model = cadmpeg_ir::document::Model::default();
    model.features.push(serde_json::from_value(serde_json::json!({
        "id":"sldprt:model:feature#47", "ordinal":0, "name":"Corner", "suppressed":false,
        "needs_recompute":false, "definition":{"definition":"stored_geometry"}, "outputs":[], "native_ref":"history:corner"
    })).unwrap());
    let run = |histories: &[FeatureHistory], lanes: &[FeatureInputLane], timestamp| {
        super::feature_faces(
            &super::Inputs {
                model: &model,
                histories,
                lanes,
                faces: &faces,
                bodies: &[],
            },
            47,
            timestamp,
        )
    };
    let selected = run(&histories, &lanes, 0x6523_4178).unwrap();
    assert_eq!(selected.resolved.len(), 2);
    assert!(selected.uncertain.is_empty());
    assert!(run(&histories, &lanes, 0x6523_4179).is_none());
    histories[0].features.push(feature);
    assert!(run(&histories, &lanes, 0x6523_4178).is_none());
    histories[0].features.pop();
    lanes[0].names.clear();
    assert!(run(&histories, &lanes, 0x6523_4178).is_none());
}

fn configuration(id: u32, name: &str, active: bool) -> cadmpeg_ir::features::DesignConfiguration {
    serde_json::from_value(
        serde_json::json!({"id":format!("sldprt:model:configuration#{id}"),
        "ordinal":id,"active":active,"name":name,"bodies":[format!("sldprt:brep:body#{id}")]}),
    )
    .unwrap()
}

fn state_bodies(
    model: &cadmpeg_ir::document::Model,
    keys: &[&str],
) -> Option<std::collections::BTreeSet<cadmpeg_ir::ids::BodyId>> {
    let class = b"visStateKeyData_c";
    let mut bytes = vec![0xff, 0xff, 1, 0];
    bytes.extend(u16::try_from(class.len()).unwrap().to_le_bytes());
    bytes.extend(class);
    for key in keys {
        let units: Vec<_> = key.encode_utf16().collect();
        bytes.extend([0xff, 0xfe, 0xff, u8::try_from(units.len()).unwrap()]);
        bytes.extend(units.into_iter().flat_map(u16::to_le_bytes));
    }
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &bytes,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    super::selected_bodies(&ctx, &bytes, model).unwrap()
}

#[test]
fn display_key_requires_one_active_configuration_and_nonempty_display_state() {
    let mut model = cadmpeg_ir::document::Model::default();
    model.configurations = vec![
        configuration(3, "Variant_A", true),
        configuration(5, "Variant_B", false),
    ];
    assert_eq!(
        state_bodies(&model, &["<Variant_A>_Bright"]).unwrap().len(),
        1
    );
    for key in [
        "<Variant_B>_Bright",
        "<Absent>_Bright",
        "<Variant_A>_",
        "missing-separator",
    ] {
        assert!(state_bodies(&model, &[key]).is_none());
    }
    assert!(state_bodies(&model, &["<Variant_A>_Bright", "<Variant_A>_Dim"]).is_none());
    model.configurations.truncate(1);
    assert!(state_bodies(&model, &["<Localized>_Bright"]).is_none());
    assert!(state_bodies(&model, &["missing-separator"]).is_none());
}
