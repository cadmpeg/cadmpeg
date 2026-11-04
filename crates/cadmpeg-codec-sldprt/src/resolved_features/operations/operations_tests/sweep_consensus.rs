//! Every owned lane must corroborate a sweep operation.

use super::*;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, SweepMode};

fn sweep_history() -> FeatureHistory {
    FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![Feature {
            id: "native-sweep".into(),
            parent: "history".into(),
            xml_tag: "Feature".into(),
            tree_parent: None,
            source_id: FeatureSource::from_value(41),
            ordinal: 0,
            name: "Sweep".into(),
            kind: "Sweep".into(),
            input_class: Some("moSweep_c".into()),
            suppressed: false,
            parameters: BTreeMap::new(),
            dimension_properties: BTreeMap::new(),
            properties: BTreeMap::new(),
            text: None,
            content: Vec::new(),
        }],
    }
}

fn sweep_lane(code: u32) -> FeatureInputLane {
    let class_offset = 32usize;
    let class = "moSweep_c";
    let name_offset = class_offset + 6 + class.len();
    let mut payload = vec![0; 100];
    payload[20..24].copy_from_slice(&code.to_le_bytes());
    payload[class_offset..class_offset + 4].copy_from_slice(&[0xff, 0xff, 1, 0]);
    payload[class_offset + 4..class_offset + 6].copy_from_slice(
        &u16::try_from(class.len())
            .expect("short authored class")
            .to_le_bytes(),
    );
    payload[class_offset + 6..name_offset].copy_from_slice(class.as_bytes());
    FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload.into(),
        classes: vec![FeatureInputClass {
            id: "class".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 32,
            name: class.into(),
        }],
        names: vec![FeatureInputName {
            id: "name".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: cadmpeg_core::decode::u64_from_index(name_offset),
            object_id: ObjectId::from_value(41),
            value: "Sweep".into(),
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
    }
}

fn projected_mode(lanes: &[FeatureInputLane]) -> SweepMode {
    let histories = [sweep_history()];
    let mut features = crate::history::project::project_features(&histories)
        .expect("authored sweep history projects");
    super::super::bind_sweep_operations(&mut features, &histories, lanes, Some(8));
    let FeatureDefinition::Operation(FeatureOperation::Sweep { shape, .. }) =
        features[0].evaluation.definition()
    else {
        panic!("history must remain a sweep");
    };
    shape.mode()
}

#[test]
fn an_owned_unknown_word_cannot_corroborate_join_in_either_lane_order() {
    for code in [0, 1, 17, 22_209, u32::MAX] {
        for lanes in [
            vec![sweep_lane(15), sweep_lane(code)],
            vec![sweep_lane(code), sweep_lane(15)],
        ] {
            assert!(matches!(projected_mode(&lanes), SweepMode::Unresolved {}));
        }
    }
    assert!(matches!(
        projected_mode(&[sweep_lane(15), sweep_lane(15)]),
        SweepMode::Solid {
            op: cadmpeg_ir::features::SolidSweepOperation::Join
        }
    ));
}

#[test]
fn malformed_or_ambiguous_owned_lanes_cannot_disappear_from_consensus() {
    let mut malformed = sweep_lane(15);
    malformed.native_payload[24] = 7;
    let mut ambiguous = sweep_lane(15);
    ambiguous.names.push(ambiguous.names[0].clone());
    for bad in [malformed, ambiguous] {
        for lanes in [vec![sweep_lane(15), bad.clone()], vec![bad, sweep_lane(15)]] {
            assert!(matches!(projected_mode(&lanes), SweepMode::Unresolved {}));
        }
    }
}

#[test]
fn a_lane_without_an_owner_does_not_veto_an_independent_sweep() {
    let mut unrelated = sweep_lane(17);
    unrelated.names[0].object_id = ObjectId::from_value(73);
    unrelated.names[0].value = "Other".into();
    assert!(matches!(
        projected_mode(&[sweep_lane(15), unrelated]),
        SweepMode::Solid {
            op: cadmpeg_ir::features::SolidSweepOperation::Join
        }
    ));
}
