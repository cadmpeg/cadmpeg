//! Inline extrusion continuation framing.

use super::{feature_inline_operation, feature_inline_operation_fields, trailer_lane};
use cadmpeg_ir::features::BooleanOp;

fn continuation(family: u16, operation: u8) -> Vec<u8> {
    let mut bytes = vec![0; 40];
    bytes[4..6].copy_from_slice(&family.to_le_bytes());
    bytes[6] = operation;
    bytes[7] = 0xc0;
    bytes[8..12].copy_from_slice(&7u32.to_le_bytes());
    bytes[16..18].copy_from_slice(&9u16.to_le_bytes());
    bytes[18..20].copy_from_slice(&1u16.to_le_bytes());
    bytes[20..22].copy_from_slice(&13u16.to_le_bytes());
    bytes[34..36].copy_from_slice(&17u16.to_le_bytes());
    bytes
}

#[test]
fn complete_continuations_preserve_family_scoped_join_and_cut() {
    for (family, operation, expected) in [
        (0x0140, 0, Some(BooleanOp::Join)),
        (0x01ca, 0, Some(BooleanOp::Cut)),
        (0x01ca, 2, Some(BooleanOp::Cut)),
        (0x0140, 2, None),
        (0x0000, 0, None),
    ] {
        let bytes = continuation(family, operation);
        let (lane, name) = trailer_lane(7, &bytes);
        assert_eq!(feature_inline_operation(&lane, &name), expected);
    }
}

#[test]
fn continuation_identity_padding_marker_and_owner_are_required() {
    let bytes = continuation(0x01ca, 0);
    let (lane, name) = trailer_lane(7, &bytes);
    let start = 10;
    for length in 0..bytes.len() {
        let mut short = lane.clone();
        short.native_payload.truncate(start + length);
        assert_eq!(feature_inline_operation_fields(&short, &name), None);
    }
    let marker = 22;
    let final_token = 34;
    for offset in [8, 12, 18, marker, final_token + 2] {
        let mut bad = lane.clone();
        bad.native_payload[start + offset] ^= 2;
        assert_eq!(feature_inline_operation_fields(&bad, &name), None);
    }
    let mut bad = lane.clone();
    bad.native_payload[start + final_token..start + final_token + 2].fill(0);
    assert_eq!(feature_inline_operation_fields(&bad, &name), None);
}

#[test]
fn compact_join_retains_the_cut_thin_class_guard() {
    use crate::resolved_features::scalars::FeatureObjectNames;

    let (lane, _) = trailer_lane(7, &continuation(0x0140, 0));
    let feature = native_feature(Some("moCutThin_c"));
    assert!(super::super::ambiguous_thin_join_operation(
        &feature,
        &[(&lane, FeatureObjectNames::new(&lane.names))]
    ));
}

#[test]
fn compact_continuation_requires_a_short_non_null_reference() {
    for reference in [0u16, 0x7fff, 0xffff] {
        let mut bytes = continuation(0x01ca, 0);
        bytes[20..22].copy_from_slice(&reference.to_le_bytes());
        let (lane, name) = trailer_lane(7, &bytes);
        assert_eq!(feature_inline_operation_fields(&lane, &name), None);
    }
}

fn native_feature(class: Option<&str>) -> crate::records::Feature {
    use crate::records::{Feature, FeatureSource};
    use std::collections::BTreeMap;

    Feature {
        id: "native".into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: FeatureSource::from_value(7),
        ordinal: 0,
        name: "F".into(),
        kind: "Extrude".into(),
        input_class: class.map(str::to_owned),
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    }
}

#[test]
fn compact_join_requires_a_declared_ordinary_class_for_a_nonzero_continuation() {
    use crate::records::{FeatureHistory, FeatureInputClass};
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    use std::collections::BTreeMap;

    let project = |class: Option<&str>, bytes: &[u8], declared, default_join| {
        let (mut lane, _) = trailer_lane(7, bytes);
        if let Some(class) = class.filter(|_| declared) {
            let mut declaration = vec![0xff, 0xff, 1, 0];
            declaration.extend(
                u16::try_from(class.len())
                    .expect("short synthetic class")
                    .to_le_bytes(),
            );
            declaration.extend(class.as_bytes());
            let name_offset = 2 + u64::try_from(declaration.len()).expect("short declaration");
            let mut payload = vec![0; 2];
            payload.extend(declaration);
            payload.extend(&lane.native_payload[2..]);
            lane.native_payload = payload.into();
            lane.names[0].offset = name_offset;
            lane.classes.push(FeatureInputClass {
                id: "class".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 2,
                name: class.into(),
            });
        }
        let mut native = native_feature(class);
        native.xml_tag = "Extrusion".into();
        native.kind = "Extrusion".into();
        if default_join {
            native
                .parameters
                .insert(cadmpeg_core::nonblank_literal!("D1"), "2".into());
            native
                .content
                .push(crate::records::FeatureContent::Dimension("D1".into()));
        }
        let histories = [FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![native],
        }];
        let mut features = crate::history::project::project_features(&histories)
            .expect("synthetic extrusion projects");
        if default_join {
            assert!(crate::history::classify::has_default_extrusion_operation(
                &histories[0].features[0]
            ));
            features[0].evaluation.edit(|definition, _| {
                let FeatureDefinition::Operation(FeatureOperation::Extrude { op, .. }) = definition
                else {
                    panic!("extrusion definition");
                };
                *op = BooleanOp::Join;
            });
        }
        super::super::bind_extrusion_operations(&mut features, &histories, &[lane], None);
        let FeatureDefinition::Operation(FeatureOperation::Extrude { op, .. }) =
            features[0].evaluation.definition()
        else {
            panic!("extrusion definition");
        };
        *op
    };
    let mut bytes = continuation(0x0140, 0);
    for class in ["moExtrusion_c", "moICE_c", "moBossThin_c"] {
        assert_eq!(project(Some(class), &bytes, true, false), BooleanOp::Join);
    }
    for class in [None, Some("moCutThin_c"), Some("moExtruRefSurface_c")] {
        assert_eq!(project(class, &bytes, true, false), BooleanOp::Unresolved);
    }
    assert_eq!(
        project(Some("moICE_c"), &bytes, false, false),
        BooleanOp::Unresolved
    );
    for operation in [0, 2] {
        let mut cut = continuation(0x01ca, operation);
        assert_eq!(project(None, &cut, false, true), BooleanOp::Join);
        cut[16..18].fill(0);
        assert_eq!(project(None, &cut, false, true), BooleanOp::Cut);
    }
    bytes[16..18].fill(0);
    assert_eq!(project(None, &bytes, false, false), BooleanOp::Join);
}
