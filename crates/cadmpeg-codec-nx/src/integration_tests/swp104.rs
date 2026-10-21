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
        assert!(crate::NxCodec::validate_native(ctx, &ir)
            .unwrap()
            .is_empty());
        wire["successor"]["object_index"] = serde_json::json!(257);
        wire["successor"]["raw_object_index"] = serde_json::json!("f10101");
        ir.native
            .namespace_mut("nx")
            .set_arena(ctx, "feature_swp104_leading_branches", &[&wire])
            .unwrap();
        let findings = crate::NxCodec::validate_native(ctx, &ir).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].check,
            cadmpeg_ir::report::check::Check::NativeLinks
        );
        assert_eq!(findings[0].severity, cadmpeg_ir::report::Severity::Error);
        assert!(findings[0].message.contains("consecutive column indices"));
    });
}
