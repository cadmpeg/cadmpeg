// SPDX-License-Identifier: Apache-2.0
//! Symbolic-thread witness extraction, neutral projection, and native admission.

use cadmpeg_ir::codec::{Codec, CodecBackend, DecodeOptions};
use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation};

fn decode(selector: u8) -> cadmpeg_ir::codec::DecodeResult {
    let mut payload = vec![selector];
    for text in ["M Profile", "M3_x_0.5"] {
        payload.extend([3, u8::try_from(text.len() + 2).unwrap()]);
        payload.extend(text.as_bytes());
        payload.push(0);
    }
    // Shifted binary64 0.1 gives a point at z=25mm; preserve both copies.
    let tuple = [0, 0, 0x2f, 0xb9, 0x99, 0x99, 0x99, 0x99, 0x99, 0x9a];
    payload.extend(tuple);
    payload.extend(tuple);
    payload.extend([1, 3, 0xf0, 0, 0xf0, 255, 0xf1, 1, 0, 255]);
    payload.extend(b"\x03\x05Cut\0");
    let part = crate::test_support::test_om::composed_feature_history_payload(
        &[(&[0xff; 4], "SYMBOLIC_THREAD", payload)],
        &[],
    );
    let file =
        crate::test_support::test_prt::prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", part)]);
    crate::NxCodec
        .decode(&mut std::io::Cursor::new(file), &DecodeOptions::default())
        .unwrap()
}

#[test]
fn symbolic_thread_pipeline_checks_witness_without_inventing_faces_or_extent() {
    let result = decode(3);
    let wire = serde_json::to_value(
        &result.ir().native.namespace("nx").unwrap().arenas()["feature_symbolic_threads"][0],
    )
    .unwrap();
    assert_eq!(wire["construction"]["parameter_class_index"], 3);
    assert_eq!(
        wire["construction"]["coordinates"][2],
        serde_json::json!({"value":0.1,"raw":"2fb999999999999a"})
    );
    assert_eq!(wire["construction"]["byte_len"], 30);
    let FeatureDefinition::Operation(FeatureOperation::CosmeticThread {
        face,
        diameter,
        extent,
    }) = result.ir().model.features[0].evaluation.definition()
    else {
        panic!("cosmetic thread")
    };
    assert_eq!(*face, FaceSelection::Unresolved);
    assert_eq!(diameter.unwrap().get(), 3.0);
    assert!(extent.is_none());
    let (mut ir, report, fidelity) = result.into_parts();
    let validation =
        cadmpeg_ir::validate_neutral_with_source_fidelity(&ir, &fidelity, report.losses).unwrap();
    assert!(validation.is_ok(), "{validation:#?}");
    crate::test_support::with_decode_context(|ctx| {
        assert!(<crate::NxCodec as CodecBackend>::validate_native(ctx, &ir)
            .unwrap()
            .is_empty());
        let mut invalid = wire.clone();
        invalid["construction"]["coordinates"][2]["value"] = serde_json::json!(1.0);
        ir.native
            .namespace_mut("nx")
            .set_arena(ctx, "feature_symbolic_threads", &[invalid])
            .unwrap();
        let findings = <crate::NxCodec as CodecBackend>::validate_native(ctx, &ir).unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("value"));
        let mut null = wire;
        null["construction"] = serde_json::Value::Null;
        ir.native
            .namespace_mut("nx")
            .set_arena(ctx, "feature_symbolic_threads", &[null])
            .unwrap();
        let findings = <crate::NxCodec as CodecBackend>::validate_native(ctx, &ir).unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0]
            .message
            .contains("construction: this key states a value"));
    });
    for selector in [0, 1, 4] {
        let result = decode(selector);
        let wire = serde_json::to_value(
            &result.ir().native.namespace("nx").unwrap().arenas()["feature_symbolic_threads"][0],
        )
        .unwrap();
        assert!(wire.get("construction").is_none());
        let FeatureDefinition::Operation(FeatureOperation::CosmeticThread { diameter, .. }) =
            result.ir().model.features[0].evaluation.definition()
        else {
            panic!("cosmetic thread")
        };
        assert!(diameter.is_none());
    }
}
