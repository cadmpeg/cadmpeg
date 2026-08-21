// SPDX-License-Identifier: Apache-2.0
//! Native lane-validation findings.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::SldprtCodec;

#[test]
fn native_validation_rejects_duplicate_history_ordinals() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        native.feature_histories[0].features[1].ordinal = 0;
    });
    assert!(crate::validate_native(decoded.ir())
        .iter()
        .any(|finding| finding.message.contains("repeats feature ordinal")));
}

#[test]
fn native_validation_rejects_broken_feature_graph() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        native.feature_histories[0].features[1].tree_parent = Some("missing-record".into());
    });
    assert!(crate::validate_native(decoded.ir())
        .iter()
        .any(|finding| finding.message.contains("missing tree parent")));
}

#[test]
fn native_validation_rejects_broken_history_root_graph() {
    use crate::records::HistoryContent;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Default"/><Feature Name="Root" Type="Custom" id="1"><Feature Name="Nested" Type="Custom" id="2"/></Feature></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        let history = &mut native.feature_histories[0];
        let nested = history
            .features
            .iter()
            .find(|feature| feature.name == "Nested")
            .unwrap()
            .id
            .clone();
        history.content = vec![
            HistoryContent::Feature(nested),
            HistoryContent::Configuration("missing-configuration".into()),
        ];
    });

    let messages = crate::validate_native(decoded.ir())
        .into_iter()
        .map(|finding| finding.message)
        .collect::<Vec<_>>();
    assert!(messages
        .iter()
        .any(|message| message.contains("references nested feature")));
    assert!(messages
        .iter()
        .any(|message| message.contains("references missing configuration")));
    assert!(messages
        .iter()
        .any(|message| message.contains("omits configuration")));
    assert!(messages
        .iter()
        .any(|message| message.contains("omits feature")));
}

#[test]
fn native_validation_rejects_orphan_history_records() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let orphan = decoded
        .ir_mut()
        .native
        .namespace_mut("sldprt")
        .arenas
        .get_mut("features")
        .unwrap()[0]
        .clone();
    let mut orphan_fields = orphan.fields();
    orphan_fields.insert(
        "parent".into(),
        serde_json::Value::String("missing-history".into()),
    );
    decoded
        .ir_mut()
        .native
        .namespace_mut("sldprt")
        .arenas
        .get_mut("features")
        .unwrap()[0] = cadmpeg_ir::NativeRecord::new(orphan.id().to_string(), orphan_fields);
    assert!(crate::validate_native(decoded.ir()).iter().any(|finding| {
        finding.message.contains("invalid owner") && finding.message.contains("missing-history")
    }));
}

#[test]
fn native_validation_rejects_edited_relation_binding() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_resolved_features(
                &triangle_body(),
                &[0, 1],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        native.feature_input_lanes[0].relation_bindings[0].family =
            crate::records::FeatureInputRelationFamily::LineLineDistance;
    });

    assert!(crate::validate_native(decoded.ir()).iter().any(|finding| {
        finding
            .message
            .contains("relation bindings do not match the native payload")
    }));
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(error.to_string().contains("edited relation bindings"));
}

#[test]
fn native_validation_rejects_edited_relation_instance() {
    let mut source = sldprt_with_compact_relation_pair(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        native.feature_input_lanes[0].relation_instances[0].parameter_scalar_ref = None;
    });

    assert!(crate::validate_native(decoded.ir()).iter().any(|finding| {
        finding
            .message
            .contains("relation instances do not match the native payload")
    }));
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(error.to_string().contains("edited relation instances"));
}
