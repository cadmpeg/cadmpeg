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
    let namespace = crate::test_support::source_fidelity::native_namespace(
        result.ir(),
        result.source_fidelity(),
    );
    let records = &namespace.arenas()["feature_extract_face_construction_envelopes"];
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
        let canonical_ir = ir.clone();
        ir.native.0.insert("nx".into(), namespace);
        assert!(
            crate::native::source_validation_findings(ctx, ir.native.namespace("nx").unwrap())
                .unwrap()
                .is_empty()
        );
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
        let findings =
            crate::native::source_validation_findings(ctx, ir.native.namespace("nx").unwrap())
                .unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("value or width mismatch"));
        let canonical = serde_json::to_value(
            &canonical_ir.native.namespace("nx").unwrap().arenas()
                ["feature_extract_face_construction_envelopes"][0],
        )
        .unwrap();
        assert!(canonical.get("source_offset").is_none());
        for (path, value, field) in [
            (
                "/lanes/0/0/right/object_index",
                serde_json::json!(u32::from(u16::MAX) + 1),
                "object_index",
            ),
            (
                "/operation_label",
                serde_json::json!("nx:feature-history:operation-label#missing"),
                "operation_label",
            ),
        ] {
            let mut document = canonical_ir.clone();
            let mut value_record = canonical.clone();
            *value_record.pointer_mut(path).unwrap() = value;
            document
                .native
                .namespace_mut("nx")
                .set_arena(
                    ctx,
                    "feature_extract_face_construction_envelopes",
                    &[value_record],
                )
                .unwrap();
            let findings =
                <crate::NxCodec as CodecBackend>::validate_native(ctx, &document).unwrap();
            assert_eq!(findings.len(), 1, "{path}: {findings:?}");
            assert_eq!(
                findings[0].check,
                cadmpeg_ir::report::check::Check::NativeLinks
            );
            assert_eq!(findings[0].severity, cadmpeg_ir::report::Severity::Error);
            assert!(findings[0].message.contains(field), "{path}: {findings:?}");
        }
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
