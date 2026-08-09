//! Tests for the `operations` module.

use super::{
    class_scoped_extrusion_operation, extrusion_operation, feature_inline_operation,
    feature_inline_operation_fields, feature_operation_code, form_code_padding,
    revolution_operation, FormCodePadding,
};
use crate::records::{
    Feature, FeatureInputClass, FeatureInputClassRole, FeatureInputLane, FeatureInputName,
};
use cadmpeg_ir::features::BooleanOp;
use std::collections::BTreeMap;
#[test]
fn inline_operation_binds_join_and_cut_to_their_family_words() {
    use crate::records::{FeatureInputLane, FeatureInputName};
    use cadmpeg_ir::features::BooleanOp;

    let value = "F";
    let name_offset = 10usize;
    let mut payload = vec![0; 40];
    let trailer = name_offset + 6 + 2;
    payload[trailer + 4] = 0x40;
    payload[trailer + 5] = 1;
    payload[trailer + 7] = 0xc0;
    payload[trailer + 8..trailer + 12].copy_from_slice(&7u32.to_le_bytes());
    payload[trailer + 16..trailer + 19].copy_from_slice(&[0xff, 0xfe, 0xff]);
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: Vec::new(),
        names: Vec::new(),
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
    let name = FeatureInputName {
        id: "name".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: name_offset as u64,
        value: value.into(),
        object_id: Some(7),
    };
    let mut lane = lane;
    lane.native_payload[name_offset - 6..name_offset - 2].copy_from_slice(&1u32.to_le_bytes());
    lane.native_payload[name_offset - 2..name_offset].copy_from_slice(&0x8d9au16.to_le_bytes());
    assert_eq!(
        feature_operation_code(&lane, &name, Some("moICE_c"), None),
        Some(1)
    );
    assert_eq!(
        feature_operation_code(&lane, &name, Some("moExtrusion_c"), None),
        None
    );
    assert_eq!(
        feature_operation_code(&lane, &name, Some("moICE_c"), Some(FormCodePadding::Eight),),
        Some(1)
    );
    assert_eq!(
        feature_inline_operation(&lane, &name),
        Some(BooleanOp::Join)
    );
    // A zero operation byte on an moICE_c object carries no operation.
    lane.native_payload[trailer + 4] = 0xca;
    assert_eq!(feature_inline_operation(&lane, &name), None);
    assert!(feature_inline_operation_fields(&lane, &name).is_some());
    lane.native_payload[trailer + 6] = 2;
    assert_eq!(feature_inline_operation(&lane, &name), Some(BooleanOp::Cut));
    lane.native_payload[trailer + 4] = 0x40;
    assert_eq!(feature_inline_operation(&lane, &name), None);
    lane.native_payload[trailer + 6] = 3;
    assert_eq!(feature_inline_operation_fields(&lane, &name), None);

    lane.native_payload[trailer + 6] = 0;
    lane.native_payload[trailer + 16..trailer + 19].fill(0);
    lane.native_payload.resize(trailer + 40, 0);
    lane.native_payload[trailer + 22..trailer + 24].copy_from_slice(&[1, 0]);
    lane.native_payload[trailer + 24..trailer + 26].copy_from_slice(&0x0185u16.to_le_bytes());
    lane.native_payload[trailer + 38..trailer + 40].copy_from_slice(&0x019fu16.to_le_bytes());
    assert_eq!(
        feature_inline_operation(&lane, &name),
        Some(BooleanOp::Join)
    );
    lane.native_payload[trailer + 38..trailer + 40].fill(0);
    assert_eq!(feature_inline_operation_fields(&lane, &name), None);

    lane.native_payload[trailer + 4] = 0xca;
    lane.native_payload[trailer + 16..trailer + 40].fill(0);
    lane.native_payload[trailer + 18..trailer + 20].copy_from_slice(&[1, 0]);
    lane.native_payload[trailer + 20..trailer + 24].copy_from_slice(&360u32.to_le_bytes());
    lane.native_payload[trailer + 34..trailer + 36].copy_from_slice(&435u16.to_le_bytes());
    assert_eq!(
        feature_inline_operation_fields(&lane, &name),
        Some((0xca, 0))
    );
    assert_eq!(feature_inline_operation(&lane, &name), None);
}

#[test]
fn declared_ice_object_uses_a_unanimous_repeated_class_form() {
    let native_feature = |id: &str, source: &str| Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some(source.into()),
        parent_source_id: None,
        ordinal: source.parse().expect("required invariant"),
        name: id.into(),
        kind: "Extrusion".into(),
        input_class: Some("moICE_c".into()),
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let features = [
        native_feature("first", "67"),
        native_feature("second", "79"),
        native_feature("third", "90"),
    ];
    let names = [
        FeatureInputName {
            id: "first-name".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 33,
            value: "F".into(),
            object_id: Some(67),
        },
        FeatureInputName {
            id: "second-name".into(),
            parent: "lane".into(),
            ordinal: 1,
            offset: 100,
            value: "S".into(),
            object_id: Some(79),
        },
        FeatureInputName {
            id: "third-name".into(),
            parent: "lane".into(),
            ordinal: 2,
            offset: 150,
            value: "T".into(),
            object_id: Some(90),
        },
    ];
    let mut payload = vec![0; 200];
    let trailer = 33 + 6 + 2;
    payload[trailer + 4] = 0xca;
    payload[trailer + 5] = 1;
    payload[trailer + 8..trailer + 12].copy_from_slice(&67u32.to_le_bytes());
    payload[trailer + 16..trailer + 19].copy_from_slice(&[0xff, 0xfe, 0xff]);
    for name_offset in [100_usize, 150] {
        let code_offset = name_offset - 14;
        payload[code_offset..code_offset + 4].copy_from_slice(&1u32.to_le_bytes());
        payload[name_offset - 2..name_offset].copy_from_slice(&0x8000u16.to_le_bytes());
    }
    let mut lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: vec![FeatureInputClass {
            id: "ice".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 20,
            name: "moICE_c".into(),
            role: FeatureInputClassRole::Feature,
        }],
        names: names.to_vec(),
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
    let feature_refs = features.iter().collect::<Vec<_>>();

    assert_eq!(
        class_scoped_extrusion_operation(&features[0], &feature_refs, &lane, &names[0], None,),
        Some(BooleanOp::Cut)
    );
    lane.native_payload[136..140].copy_from_slice(&6u32.to_le_bytes());
    assert_eq!(
        class_scoped_extrusion_operation(&features[0], &feature_refs, &lane, &names[0], None,),
        None
    );
}

#[test]
fn extrusion_form_codes_are_scoped_to_their_native_classes() {
    use cadmpeg_ir::features::BooleanOp;

    assert_eq!(
        extrusion_operation(Some("moExtrusion_c"), 82),
        Some(BooleanOp::Join)
    );
    assert_eq!(
        extrusion_operation(Some("moExtrusion_c"), 4),
        Some(BooleanOp::Join)
    );
    assert_eq!(extrusion_operation(Some("moICE_c"), 82), None);
    for code in [6, 21, 0x3ee4_f8b5] {
        assert_eq!(
            extrusion_operation(Some("moICE_c"), code),
            Some(BooleanOp::Join)
        );
    }
    for code in [0, 1, 2, 5, 7, 10, 14, 15, 22_993, u32::MAX] {
        assert_eq!(
            extrusion_operation(Some("moICE_c"), code),
            Some(BooleanOp::Cut)
        );
    }
    assert_eq!(extrusion_operation(Some("moICE_c"), 11), None);
    assert_eq!(extrusion_operation(Some("moExtrusion_c"), 11), None);
    assert_eq!(extrusion_operation(Some("moExtrusion_c"), u32::MAX), None);
}

#[test]
fn ambiguous_direct_form_code_padding_does_not_shift_the_code() {
    let direct_lane = |code: u32, preceding: u32, padding: usize| {
        let class_offset = 32usize;
        let class_name = "moICE_c";
        let name_offset = class_offset + 6 + class_name.len();
        let code_offset = class_offset - 4 - padding;
        let mut payload = vec![0; 128];
        payload[code_offset..code_offset + 4].copy_from_slice(&code.to_le_bytes());
        payload[class_offset..class_offset + 4].copy_from_slice(&[0xff, 0xff, 0x01, 0x00]);
        payload[class_offset + 4..class_offset + 6]
            .copy_from_slice(&(class_name.len() as u16).to_le_bytes());
        payload[class_offset + 6..name_offset].copy_from_slice(class_name.as_bytes());
        if padding == 4 {
            payload[class_offset - 12..class_offset - 8].copy_from_slice(&preceding.to_le_bytes());
        } else {
            payload[class_offset - 16..class_offset - 12].copy_from_slice(&preceding.to_le_bytes());
        }
        (
            FeatureInputLane {
                id: "lane".into(),
                configuration: None,
                native_payload: payload,
                classes: vec![FeatureInputClass {
                    id: "class".into(),
                    parent: "lane".into(),
                    ordinal: 0,
                    offset: class_offset as u64,
                    name: class_name.into(),
                    role: FeatureInputClassRole::Feature,
                }],
                names: Vec::new(),
                scalars: Vec::new(),
                relation_bindings: Vec::new(),
                relation_instances: Vec::new(),
                body_selections: Vec::new(),
                edge_selections: Vec::new(),
                surface_selections: Vec::new(),
                generated_surface_identities: Vec::new(),
                references: Vec::new(),
                sketch_entities: Vec::new(),
            },
            FeatureInputName {
                id: "name".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: name_offset as u64,
                value: "Feature".into(),
                object_id: Some(1),
            },
        )
    };

    let (lane, name) = direct_lane(3, 11, 4);
    assert_eq!(
        feature_operation_code(&lane, &name, Some("moICE_c"), None),
        Some(3)
    );

    let (lane, name) = direct_lane(0, 11, 4);
    assert_eq!(
        feature_operation_code(&lane, &name, Some("moICE_c"), None),
        None
    );

    let (lane, name) = direct_lane(11, 0, 8);
    assert_eq!(
        feature_operation_code(&lane, &name, Some("moICE_c"), None),
        None
    );

    let (lane, name) = direct_lane(0, 11, 4);
    assert_eq!(
        feature_operation_code(&lane, &name, Some("moICE_c"), Some(FormCodePadding::Four)),
        Some(0)
    );

    let (lane, name) = direct_lane(11, 0, 8);
    assert_eq!(
        feature_operation_code(&lane, &name, Some("moICE_c"), Some(FormCodePadding::Eight)),
        Some(11)
    );
}

#[test]
fn form_code_padding_follows_the_solidworks_schema_version() {
    assert_eq!(form_code_padding(None), None);
    assert_eq!(form_code_padding(Some("")), None);
    assert_eq!(
        form_code_padding(Some("11000")),
        Some(FormCodePadding::Four)
    );
    assert_eq!(
        form_code_padding(Some("12000")),
        Some(FormCodePadding::Eight)
    );
    assert_eq!(
        form_code_padding(Some("34000")),
        Some(FormCodePadding::Eight)
    );
}

#[test]
fn revolution_form_words_distinguish_new_body_and_join() {
    for code in [5, 6, 11, 60, 20_322, 22_016] {
        assert_eq!(
            revolution_operation(Some("moRevolution_c"), code),
            Some(BooleanOp::NewBody)
        );
    }
    assert_eq!(
        revolution_operation(Some("moRevolution_c"), 8),
        Some(BooleanOp::Join)
    );
    assert_eq!(revolution_operation(Some("moRevolution_c"), 7), None);
    assert_eq!(
        revolution_operation(Some("moRevCut_c"), 13),
        Some(BooleanOp::Cut)
    );
}
