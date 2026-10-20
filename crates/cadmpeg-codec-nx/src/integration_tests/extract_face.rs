// SPDX-License-Identifier: Apache-2.0
//! `EXTRACT_FACE` extraction and composed neutral, fidelity, and native admission.

use cadmpeg_ir::codec::{Codec, CodecBackend, DecodeOptions};

fn decode(selector: u8) -> cadmpeg_ir::codec::DecodeResult {
    let mut prefix = vec![selector, 0, 0, 1, 0];
    for _ in 0..4 {
        prefix.extend([1, 0, 0, 0, 0]);
    }
    prefix.extend([0; 6]);
    // Zero is a required endpoint, and duplicate relation keys remain ordered.
    prefix.extend([1, 3, 7, 0, 0x80, 1, 7, 0, 0x80, 1]);
    prefix.extend([1, 1, 255, 0, 0, 0, 0, 0, 0, 3, 4, 0xc3, 0xa9, 0]);
    let part = crate::test_support::test_om::composed_feature_history_payload(
        &[(&[0xff; 4], "EXTRACT_FACE", prefix)],
        &[],
    );
    let file =
        crate::test_support::test_prt::prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", part)]);
    crate::NxCodec
        .decode(&mut std::io::Cursor::new(file), &DecodeOptions::default())
        .unwrap()
}

#[test]
fn extract_face_pipeline_retains_relations_without_inventing_authored_face_roles() {
    let result = decode(3);
    let records = &result.ir().native.namespace("nx").unwrap().arenas()
        ["feature_extract_face_construction_envelopes"];
    assert_eq!(records.len(), 1);
    let wire = serde_json::to_value(&records[0]).unwrap();
    let relation = serde_json::json!({"key":7,"left":{"object_index":0,"raw_object_index":"00"},"right":{"object_index":1,"raw_object_index":"8001"}});
    assert_eq!(
        wire["lanes"],
        serde_json::json!([[relation.clone(), relation], []])
    );
    assert_eq!(wire["navigator_text"], "é");
    assert_eq!(wire["byte_len"], 55);
    assert_eq!(
        serde_json::to_value(result.ir().model.features[0].evaluation.definition()).unwrap(),
        serde_json::json!({"definition":"unresolved", "family":"extract_face"})
    );
    let (mut ir, report, fidelity) = result.into_parts();
    let validation =
        cadmpeg_ir::validate_neutral_with_source_fidelity(&ir, &fidelity, report.losses).unwrap();
    assert!(validation.is_ok(), "{validation:#?}");
    crate::test_support::with_decode_context(|ctx| {
        assert!(<crate::NxCodec as CodecBackend>::validate_native(ctx, &ir)
            .unwrap()
            .is_empty());
        let mut invalid = wire;
        invalid["lanes"][0][0]["right"]["object_index"] = serde_json::json!(2);
        ir.native
            .namespace_mut("nx")
            .set_arena(
                ctx,
                "feature_extract_face_construction_envelopes",
                &[invalid],
            )
            .unwrap();
        let findings = <crate::NxCodec as CodecBackend>::validate_native(ctx, &ir).unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("value or width mismatch"));
    });
    for selector in [0, 1, 4] {
        assert!(!decode(selector)
            .ir()
            .native
            .namespace("nx")
            .unwrap()
            .arenas()
            .contains_key("feature_extract_face_construction_envelopes"));
    }
}
