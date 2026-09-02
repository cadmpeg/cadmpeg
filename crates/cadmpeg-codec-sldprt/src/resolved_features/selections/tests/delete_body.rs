// SPDX-License-Identifier: Apache-2.0
//! Compact delete-body selection decode and native-store tests.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::SldprtCodec;

#[test]
fn decode_and_validate_compact_delete_body_selection() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Body-Delete/Keep 1" Type="Body-Delete/Keep " id="41"/></Keywords>"#,
    ));
    let mut payload =
        resolved_feature_classes_with_ids(&[("moDeleteBody_c", "Body-Delete/Keep 1", 41)]);
    payload.extend([0xff, 0xff, 0x01, 0x00]);
    payload.extend(18u16.to_le_bytes());
    payload.extend(b"moDeleteBodyData_c");
    payload.extend([0x08, 0x00]);
    let token = 0x89a4u16;
    let mut state = [0u8; 83];
    state[0..2].copy_from_slice(&token.to_le_bytes());
    state[2..11].copy_from_slice(&[0x2b, 0x80, 0x02, 0, 0, 0, 0, 0, 0]);
    state[11..15].copy_from_slice(&287u32.to_le_bytes());
    state[15..19].copy_from_slice(&287u32.to_le_bytes());
    state[47..63].fill(0xff);
    payload.extend(state);
    payload.extend([0x30, 0x80]);
    payload.extend(1u32.to_le_bytes());
    payload.extend([0; 4]);
    payload.extend(11000u32.to_le_bytes());
    payload.extend([0; 8]);
    payload.extend(2u32.to_le_bytes());
    payload.extend(287u32.to_le_bytes());
    payload.extend(115u32.to_le_bytes());
    payload.extend(u32::MAX.to_le_bytes());
    payload.extend([0; 12]);
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("body delete/keep feature(s)")));
    let mut native = sldprt_native(decoded.ir());
    let [selection] = native.feature_input_lanes[0].body_selections.as_slice() else {
        panic!("one compact body selection");
    };
    assert_eq!(selection.local_body_ids, [287, 115]);
    assert_eq!(selection.body_state_ids, [287]);
    assert_eq!(
        selection.mode,
        Some(cadmpeg_ir::features::BodyRetentionMode::DeleteSelected)
    );

    let mut legacy = decoded.ir().native.namespace("sldprt").unwrap().clone();
    legacy.version = 5;
    for record in legacy
        .arenas
        .get_mut("feature_input_body_selections")
        .unwrap()
    {
        let mut fields = record.fields();
        fields.remove("mode");
        *record = cadmpeg_ir::NativeRecord::new(record.id().to_string(), fields);
    }
    let migrated = crate::native::SldprtNative::load(&legacy).unwrap();
    assert_eq!(
        migrated.feature_input_lanes[0].body_selections[0].mode,
        Some(cadmpeg_ir::features::BodyRetentionMode::DeleteSelected)
    );
    assert!(selection.feature_ref.starts_with("sldprt:history:feature#"));
    let delete_feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Body-Delete/Keep 1"))
        .expect("delete-body feature");
    assert!(matches!(
        &delete_feature.definition,
        cadmpeg_ir::features::FeatureDefinition::DeleteBody { bodies, mode }
            if bodies == &cadmpeg_ir::features::BodySelection::Local {
                bodies: vec!["287".into(), "115".into()],
                native: "sldprt:feature-input:body-ids:287,115".into(),
            } && *mode == cadmpeg_ir::features::BodyRetentionMode::DeleteSelected
    ));
    SldprtCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap();

    decoded
        .ir_mut()
        .model
        .features
        .iter_mut()
        .find(|feature| feature.name.as_deref() == Some("Body-Delete/Keep 1"))
        .expect("delete-body feature")
        .name = Some("Renamed Delete Body".into());
    let mut renamed = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut renamed)
        .unwrap();
    let renamed = SldprtCodec
        .decode(&mut Cursor::new(renamed), &DecodeOptions::default())
        .unwrap();
    let renamed_native = sldprt_native(renamed.ir());
    assert!(!renamed_native.feature_histories[0].features[0]
        .properties
        .contains_key("Bodies"));
    assert_eq!(
        renamed_native.feature_input_lanes[0].body_selections[0].local_body_ids,
        [287, 115]
    );

    {
        {
            let mut ir_edit = decoded.ir_mut();
            let delete_feature = ir_edit
                .model
                .features
                .iter_mut()
                .find(|feature| feature.name.as_deref() == Some("Renamed Delete Body"))
                .expect("delete-body feature");
            let cadmpeg_ir::features::FeatureDefinition::DeleteBody { bodies, .. } =
                &mut delete_feature.definition
            else {
                panic!("typed delete-body feature");
            };
            *bodies = cadmpeg_ir::features::BodySelection::Native(
                "sldprt:feature-input:body-ids:287".into(),
            );
        }
        let error = SldprtCodec
            .write_preserved_with_source_fidelity(
                decoded.ir(),
                decoded.source_fidelity(),
                &mut Vec::new(),
            )
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("changes a compact body selection"));
    }

    {
        {
            let mut ir_edit = decoded.ir_mut();
            let delete_feature = ir_edit
                .model
                .features
                .iter_mut()
                .find(|feature| feature.name.as_deref() == Some("Renamed Delete Body"))
                .expect("delete-body feature");
            let cadmpeg_ir::features::FeatureDefinition::DeleteBody { bodies, mode } =
                &mut delete_feature.definition
            else {
                unreachable!("typed delete-body feature");
            };
            *bodies = cadmpeg_ir::features::BodySelection::Local {
                bodies: vec!["287".into(), "115".into()],
                native: "sldprt:feature-input:body-ids:287,115".into(),
            };
            *mode = cadmpeg_ir::features::BodyRetentionMode::KeepSelected;
        }
        let error = SldprtCodec
            .write_preserved_with_source_fidelity(
                decoded.ir(),
                decoded.source_fidelity(),
                &mut Vec::new(),
            )
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("changes a compact body retention mode"));
    }

    native.feature_input_lanes[0].body_selections[0]
        .body_state_ids
        .push(287);
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    let error = native.store(&mut namespace).unwrap_err();
    assert!(
        error.to_string().contains("body selection")
            && error.to_string().contains("inconsistent ownership")
    );
    native.feature_input_lanes[0].body_selections[0].body_state_ids = vec![287];

    native.feature_input_lanes[0].body_selections[0].mode =
        Some(cadmpeg_ir::features::BodyRetentionMode::KeepSelected);
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    let error = native.store(&mut namespace).unwrap_err();
    assert!(
        error.to_string().contains("body selection")
            && error.to_string().contains("inconsistent ownership")
    );
    native.feature_input_lanes[0].body_selections[0].mode =
        Some(cadmpeg_ir::features::BodyRetentionMode::DeleteSelected);

    native.feature_input_lanes[0].body_selections[0].local_body_ids[0] = 288;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    let error = native.store(&mut namespace).unwrap_err();
    assert!(
        error.to_string().contains("body selection")
            && error.to_string().contains("inconsistent ownership")
    );
}
