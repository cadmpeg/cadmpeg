// SPDX-License-Identifier: Apache-2.0
//! Registered native CHECK admits the complete SWP104 column sequence.

use cadmpeg_ir::codec::CodecBackend;

#[test]
fn swp104_native_validator_rejects_a_nonconsecutive_successor() {
    let mut wire = serde_json::json!({
        "id":"nx:feature-history:swp104-leading-branch#0", "operation_label":"operation",
        "parameter_class_index":33, "profile_class_index":35,
        "scalars":[0.04,0.04,0.04,0.04],
        "raw_scalars":["2fa47ae147ae147b","2fa47ae147ae147b",
                       "2fa47ae147ae147b","2fa47ae147ae147b"],
        "leading_zero":false, "declared_count":2, "state_lane":"0000000000",
        "members":[{"ordinal":0,"object_index":255,"raw_object_index":"f0ff",
                    "source_offset":240}],
        "successor":{"ordinal":1,"object_index":256,"raw_object_index":"f10100",
                     "source_offset":250},
        "byte_len":54, "source_offset":200
    });
    crate::test_support::with_decode_context(|ctx| {
        let mut ir = cadmpeg_ir::CadIr::empty();
        ir.native
            .namespace_mut("nx")
            .set_arena(ctx, "feature_swp104_leading_branches", &[&wire])
            .unwrap();
        assert!(
            crate::native::source_validation_findings(ctx, ir.native.namespace("nx").unwrap())
                .unwrap()
                .is_empty()
        );
        wire["successor"]["object_index"] = serde_json::json!(257);
        wire["successor"]["raw_object_index"] = serde_json::json!("f10101");
        ir.native
            .namespace_mut("nx")
            .set_arena(ctx, "feature_swp104_leading_branches", &[&wire])
            .unwrap();
        let findings =
            crate::native::source_validation_findings(ctx, ir.native.namespace("nx").unwrap())
                .unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].check,
            cadmpeg_ir::report::check::Check::NativeLinks
        );
        assert_eq!(findings[0].severity, cadmpeg_ir::report::Severity::Error);
        assert!(findings[0].message.contains("consecutive column indices"));
    });
}

#[test]
fn canonical_swp104_checks_successor_order_and_operation_ownership() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    // Same scalar/index values as the source-backed regression above, inside
    // an actual operation container so its owner is supplied by the decoder.
    let mut payload = vec![33, 0, 0, 1, 0];
    for _ in 0..4 {
        payload.extend([47, 164, 122, 225, 71, 174, 20, 123]);
    }
    payload.extend([35, 1, 2, 240, 255]);
    payload.extend([0; 5]);
    payload.extend([255, 1, 2, 241, 1, 0, 0]);
    let part = crate::test_support::test_om::composed_feature_history_payload(
        &[(&[0xff; 4], "SWP104", payload)],
        &[],
    );
    let file =
        crate::test_support::test_prt::prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", part)]);
    let result = crate::NxCodec
        .decode(&mut std::io::Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    let canonical_ir = result.ir();
    crate::test_support::with_decode_context(|ctx| {
        assert!(
            <crate::NxCodec as CodecBackend>::validate_native(ctx, canonical_ir)
                .unwrap()
                .is_empty()
        );
        let records = &canonical_ir.native.namespace("nx").unwrap().arenas()
            ["feature_swp104_leading_branches"];
        assert_eq!(records.len(), 1);
        let original = serde_json::to_value(&records[0]).unwrap();
        assert!(original.get("source_offset").is_none());
        assert_eq!(
            original["scalars"],
            serde_json::json!([0.04, 0.04, 0.04, 0.04])
        );
        assert_eq!(original["members"][0]["object_index"], 255);
        assert_eq!(original["successor"]["object_index"], 256);
        for (path, value, expected) in [
            (
                "/successor/object_index",
                serde_json::json!(257),
                "consecutive column indices",
            ),
            (
                "/operation_label",
                serde_json::json!("nx:feature-history:operation-label#missing"),
                "operation_label",
            ),
        ] {
            let mut document = canonical_ir.clone();
            let mut value_record = original.clone();
            *value_record.pointer_mut(path).unwrap() = value;
            document
                .native
                .namespace_mut("nx")
                .set_arena(ctx, "feature_swp104_leading_branches", &[value_record])
                .unwrap();
            let findings =
                <crate::NxCodec as CodecBackend>::validate_native(ctx, &document).unwrap();
            assert_eq!(findings.len(), 1, "{path}: {findings:?}");
            assert_eq!(
                findings[0].check,
                cadmpeg_ir::report::check::Check::NativeLinks
            );
            assert_eq!(findings[0].severity, cadmpeg_ir::report::Severity::Error);
            assert!(
                findings[0].message.contains(expected),
                "{path}: {findings:?}"
            );
        }
    });
}
