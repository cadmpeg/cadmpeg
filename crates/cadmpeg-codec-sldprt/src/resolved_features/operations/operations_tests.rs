//! Tests for the `operations` module.

use super::*;
use crate::records::{
    Feature, FeatureHistory, FeatureInputClass, FeatureInputClassRole, FeatureInputLane,
    FeatureInputName,
};
use cadmpeg_ir::features::BooleanOp;
use std::collections::BTreeMap;

#[test]
fn split_line_projection_mode_requires_one_owned_project_class() {
    let native_feature = |id: &str, source: &str, class: &str| Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some(source.into()),
        parent_source_id: None,
        ordinal: source.parse().expect("required invariant"),
        name: id.into(),
        kind: "Split Line".into(),
        input_class: Some(class.into()),
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let dimensions = BTreeMap::from([("D1".into(), "2".into())]);
    let mut split = native_feature("split", "40", "moPLine_c");
    split.parameters.clone_from(&dimensions);
    let mut sketch = native_feature("sketch", "30", "moProfileFeature_c");
    sketch.xml_tag = "Sketch".into();
    sketch.kind = "Sketch".into();
    sketch.parameters = dimensions;
    let mut history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![split, sketch, native_feature("next", "50", "moExtrusion_c")],
    };
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: vec![0; 200],
        classes: vec![FeatureInputClass {
            id: "project".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 100,
            name: "moPLineProject_c".into(),
            role: FeatureInputClassRole::Auxiliary,
        }],
        names: vec![
            FeatureInputName {
                id: "split-name".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 20,
                value: "split".into(),
                object_id: Some(40),
            },
            FeatureInputName {
                id: "next-name".into(),
                parent: "lane".into(),
                ordinal: 1,
                offset: 150,
                value: "next".into(),
                object_id: Some(50),
            },
        ],
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

    let mut projected = vec![history.clone()];
    enrich_history_split_lines(&mut projected, std::slice::from_ref(&lane));
    assert_eq!(
        projected[0].features[0]
            .properties
            .get(SPLIT_LINE_MODE_PROPERTY)
            .map(String::as_str),
        Some(SPLIT_LINE_PROJECTION_MODE)
    );
    assert_eq!(
        projected[0].features[0]
            .properties
            .get(SPLIT_LINE_TOOL_PROPERTY)
            .map(String::as_str),
        Some("sketch")
    );

    let mut ambiguous_lane = lane.clone();
    ambiguous_lane.classes.push(FeatureInputClass {
        id: "second-project".into(),
        parent: "lane".into(),
        ordinal: 1,
        offset: 120,
        name: "moPLineProject_c".into(),
        role: FeatureInputClassRole::Auxiliary,
    });
    let mut ambiguous = vec![history.clone()];
    enrich_history_split_lines(&mut ambiguous, &[ambiguous_lane]);
    assert!(!ambiguous[0].features[0]
        .properties
        .contains_key(SPLIT_LINE_MODE_PROPERTY));

    let mut duplicate_sketch = history.features[1].clone();
    duplicate_sketch.id = "duplicate-sketch".into();
    duplicate_sketch.source_id = Some("20".into());
    history.features.insert(2, duplicate_sketch);
    let mut ambiguous_tool = vec![history];
    enrich_history_split_lines(&mut ambiguous_tool, &[lane]);
    assert_eq!(
        ambiguous_tool[0].features[0]
            .properties
            .get(SPLIT_LINE_MODE_PROPERTY)
            .map(String::as_str),
        Some(SPLIT_LINE_PROJECTION_MODE)
    );
    assert!(!ambiguous_tool[0].features[0]
        .properties
        .contains_key(SPLIT_LINE_TOOL_PROPERTY));
}
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
        feature_operation_code(&lane, &name, Some("moICE_c"), Some(8)),
        Some(1)
    );
    assert_eq!(
        feature_inline_operation(&lane, &name),
        Some(BooleanOp::Join)
    );
    // The 0x01ca family supplies subtraction when its operation byte is zero.
    lane.native_payload[trailer + 4] = 0xca;
    assert_eq!(feature_inline_operation(&lane, &name), Some(BooleanOp::Cut));
    assert!(feature_inline_operation_fields(&lane, &name).is_some());
    lane.native_payload[trailer + 6] = 2;
    assert_eq!(feature_inline_operation(&lane, &name), Some(BooleanOp::Cut));
    lane.native_payload[trailer + 4] = 0x40;
    assert_eq!(feature_inline_operation(&lane, &name), None);
    lane.native_payload[trailer + 5] = 2;
    assert_eq!(
        feature_inline_operation_fields(&lane, &name),
        Some((0x0240, 2))
    );
    assert_eq!(feature_inline_operation(&lane, &name), None);
    lane.native_payload[trailer + 5] = 1;
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
    lane.native_payload[trailer + 26..trailer + 30].copy_from_slice(&70_321u32.to_le_bytes());
    assert_eq!(
        feature_inline_operation(&lane, &name),
        Some(BooleanOp::Join)
    );
    lane.native_payload[trailer + 26..trailer + 30].fill(0xff);
    assert_eq!(feature_inline_operation_fields(&lane, &name), None);
    lane.native_payload[trailer + 26..trailer + 30].fill(0);
    lane.native_payload[trailer + 38..trailer + 40].fill(0);
    assert_eq!(feature_inline_operation_fields(&lane, &name), None);

    lane.native_payload[trailer + 4] = 0xca;
    lane.native_payload[trailer + 16..trailer + 40].fill(0);
    lane.native_payload[trailer + 18..trailer + 20].copy_from_slice(&[1, 0]);
    lane.native_payload[trailer + 20..trailer + 24].copy_from_slice(&360u32.to_le_bytes());
    lane.native_payload[trailer + 34..trailer + 36].copy_from_slice(&435u16.to_le_bytes());
    assert_eq!(
        feature_inline_operation_fields(&lane, &name),
        Some((0x01ca, 0))
    );
    assert_eq!(feature_inline_operation(&lane, &name), Some(BooleanOp::Cut));

    // The common sparse form has its marker at +22 and final token at +38.
    lane.native_payload[trailer + 16..trailer + 40].fill(0);
    lane.native_payload[trailer + 6] = 2;
    lane.native_payload[trailer + 22..trailer + 24].copy_from_slice(&[1, 0]);
    lane.native_payload[trailer + 24..trailer + 26].copy_from_slice(&0x04efu16.to_le_bytes());
    lane.native_payload[trailer + 38..trailer + 40].copy_from_slice(&0x008bu16.to_le_bytes());
    assert_eq!(
        feature_inline_operation_fields(&lane, &name),
        Some((0x01ca, 2))
    );
    assert_eq!(feature_inline_operation(&lane, &name), Some(BooleanOp::Cut));

    // An older schema stores u64(1) in the field before the final token.
    lane.native_payload[trailer + 30] = 1;
    assert_eq!(
        feature_inline_operation_fields(&lane, &name),
        Some((0x01ca, 2))
    );

    // A compact continuation may retain either secondary family word.
    lane.native_payload[trailer + 16..trailer + 40].fill(0);
    lane.native_payload[trailer + 20..trailer + 22].copy_from_slice(&0x00b3u16.to_le_bytes());
    lane.native_payload[trailer + 22..trailer + 24].copy_from_slice(&[1, 0]);
    lane.native_payload[trailer + 24..trailer + 28].copy_from_slice(&0x0252u32.to_le_bytes());
    lane.native_payload[trailer + 38..trailer + 40].copy_from_slice(&0x0097u16.to_le_bytes());
    assert_eq!(
        feature_inline_operation_fields(&lane, &name),
        Some((0x01ca, 2))
    );
    lane.native_payload[trailer + 20..trailer + 22].copy_from_slice(&0x00b2u16.to_le_bytes());
    assert_eq!(
        feature_inline_operation_fields(&lane, &name),
        Some((0x01ca, 2))
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
        feature_operation_code(&lane, &name, Some("moICE_c"), Some(4)),
        Some(0)
    );

    let (lane, name) = direct_lane(11, 0, 8);
    assert_eq!(
        feature_operation_code(&lane, &name, Some("moICE_c"), Some(8)),
        Some(11)
    );
}

#[test]
fn form_code_padding_follows_the_solidworks_schema_version() {
    use crate::dialect::SldprtDialect;

    assert_eq!(
        SldprtDialect::from_declaration(None).form_code_padding(),
        None
    );
    assert_eq!(
        SldprtDialect::from_declaration(Some("")).form_code_padding(),
        None
    );
    assert_eq!(
        SldprtDialect::from_declaration(Some("11000")).form_code_padding(),
        Some(4)
    );
    assert_eq!(
        SldprtDialect::from_declaration(Some("12000")).form_code_padding(),
        Some(8)
    );
    assert_eq!(
        SldprtDialect::from_declaration(Some("34000")).form_code_padding(),
        Some(8)
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

#[test]
fn configuration_operation_fallback_fills_only_unresolved_matching_operations() {
    use cadmpeg_ir::features::{
        ExtrudeDirection, ExtrudeExtent, ExtrudeSide, ExtrudeStart, FeatureDefinition, Length,
        ProfileRef, RevolutionAxis, RevolutionConstruction, RevolveExtent, Termination,
    };
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::sketches::SketchId;

    let extrude = |op| FeatureDefinition::Extrude {
        profile: ProfileRef::Sketch(SketchId("sketch".into())),
        direction: ExtrudeDirection::ProfileNormal,
        start: ExtrudeStart::ProfilePlane,
        extent: ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: Termination::Blind {
                    length: Length(1.0),
                },
                draft: None,
                offset: None,
            },
        },
        op,
        direction_source: None,
        solid: Some(true),
        face_maker: None,
        inner_wire_taper: None,
        length_along_profile_normal: None,
        allow_multi_profile_faces: None,
    };
    let revolve = |op| FeatureDefinition::Revolve {
        construction: RevolutionConstruction {
            profile: Some(ProfileRef::Sketch(SketchId("sketch".into()))),
            axis: Some(RevolutionAxis {
                origin: Point3::new(0.0, 0.0, 0.0),
                direction: Vector3::new(0.0, 0.0, 1.0),
            }),
            extent: Some(RevolveExtent::OneSided {
                termination: Termination::ThroughAll,
            }),
            axis_reference: None,
            solid: Some(true),
            face_maker_class: None,
            fuse_order: None,
            allow_multi_profile_faces: None,
        },
        op,
    };
    let feature = |id: &str, native_ref: &str, definition| cadmpeg_ir::features::Feature {
        id: id.into(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition,
        native_ref: Some(native_ref.into()),
    };
    let native_feature = |id: &str, class: &str| Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some("1".into()),
        parent_source_id: None,
        ordinal: 0,
        name: id.into(),
        kind: "operation".into(),
        input_class: Some(class.into()),
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let histories = [FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![
            native_feature("native-extrude", "moExtrusion_c"),
            native_feature("native-revolve", "moRevolution_c"),
        ],
    }];
    let base = vec![
        feature("extrude", "native-extrude", extrude(BooleanOp::Cut)),
        feature("revolve", "native-revolve", revolve(BooleanOp::Join)),
    ];
    let mut configured = vec![
        feature("extrude", "native-extrude", extrude(BooleanOp::Unresolved)),
        feature("revolve", "native-revolve", revolve(BooleanOp::Unresolved)),
    ];

    inherit_configuration_operations(&mut configured, &base, &histories, &[], None);

    assert!(matches!(
        configured[0].definition,
        FeatureDefinition::Extrude {
            op: BooleanOp::Cut,
            ..
        }
    ));
    assert!(matches!(
        configured[1].definition,
        FeatureDefinition::Revolve {
            op: BooleanOp::Join,
            ..
        }
    ));

    configured[0].definition = extrude(BooleanOp::NewBody);
    inherit_configuration_operations(&mut configured, &base, &histories, &[], None);
    assert!(matches!(
        configured[0].definition,
        FeatureDefinition::Extrude {
            op: BooleanOp::NewBody,
            ..
        }
    ));

    let mut operation_lane = FeatureInputLane {
        id: "lane".into(),
        configuration: Some("0".into()),
        native_payload: vec![0; 128],
        classes: vec![FeatureInputClass {
            id: "class".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 33,
            name: "moExtrusion_c".into(),
            role: FeatureInputClassRole::Feature,
        }],
        names: vec![FeatureInputName {
            id: "name".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 52,
            value: "native-extrude".into(),
            object_id: Some(1),
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
    };
    operation_lane.native_payload[25..29].copy_from_slice(&11_u32.to_le_bytes());
    let mut inherited = vec![feature(
        "extrude",
        "native-extrude",
        extrude(BooleanOp::Unresolved),
    )];
    inherit_configuration_operations(
        &mut inherited,
        &base,
        &histories,
        &[operation_lane.clone()],
        Some(4),
    );
    assert!(matches!(
        inherited[0].definition,
        FeatureDefinition::Extrude {
            op: BooleanOp::Cut,
            ..
        }
    ));

    operation_lane.native_payload[25..29].copy_from_slice(&999_u32.to_le_bytes());
    let mut unresolved = vec![feature(
        "extrude",
        "native-extrude",
        extrude(BooleanOp::Unresolved),
    )];
    inherit_configuration_operations(
        &mut unresolved,
        &base,
        &histories,
        &[operation_lane],
        Some(4),
    );
    assert!(matches!(
        unresolved[0].definition,
        FeatureDefinition::Extrude {
            op: BooleanOp::Unresolved,
            ..
        }
    ));
}
