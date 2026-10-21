// SPDX-License-Identifier: Apache-2.0
//! Registered native CHECK validates body-write selector and frame boundaries.

use cadmpeg_ir::codec::CodecBackend;

#[test]
fn body_write_native_check_rejects_zero_class_indices() {
    for arena in [
        "feature_operation_body_writes",
        "feature_unlabeled_operation_body_writes",
    ] {
        let id = if arena == "feature_operation_body_writes" {
            "nx:feature-history:operation-body-write#0"
        } else {
            "nx:feature-history:unlabeled-operation-body-write#0"
        };
        let mut wire = serde_json::json!({
            "id": id, "operation_record":"record", "ordinal":0,
            "body_identity":11, "group_node":1, "raw_group_node":"01",
            "group_node_source_offset":103, "endpoint_class_index":17,
            "body_image_object_index":2, "raw_body_image_object_index":"02",
            "body_image_object_index_source_offset":109, "byte_len":11, "source_offset":100
        });
        crate::test_support::with_decode_context(|ctx| {
            let mut ir = cadmpeg_ir::CadIr::empty();
            ir.native
                .namespace_mut("nx")
                .set_arena(ctx, arena, &[&wire])
                .unwrap();
            assert!(crate::NxCodec::validate_native(ctx, &ir)
                .unwrap()
                .is_empty());
            wire["endpoint_class_index"] = serde_json::json!(0);
            ir.native
                .namespace_mut("nx")
                .set_arena(ctx, arena, &[&wire])
                .unwrap();
            let findings = crate::NxCodec::validate_native(ctx, &ir).unwrap();
            assert_eq!(findings.len(), 1);
            assert_eq!(
                findings[0].check,
                cadmpeg_ir::report::check::Check::NativeLinks
            );
            assert_eq!(findings[0].severity, cadmpeg_ir::report::Severity::Error);
            assert!(findings[0]
                .message
                .contains("endpoint_class_index must be nonzero"));
        });
    }
}

#[test]
fn multiple_body_writes_form_one_checked_feature_result() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    let payload = crate::test_support::test_om::composed_feature_history_payload(
        &[(
            &[0xff; 4],
            "BLOCK",
            vec![
                0x01, 0x02, 0x11, 0x31, 0x97, 0x75, 0x01, 0x02, 0x10, 0x41, 0xff, 0x01, 0x02, 0x12,
                0x31, 0x97, 0x75, 0x01, 0x02, 0x10, 0x42, 0xff,
            ],
        )],
        &[],
    );
    let file = crate::test_support::test_prt::prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        payload,
    )]);
    let result = crate::NxCodec
        .decode(&mut std::io::Cursor::new(file), &DecodeOptions::default())
        .expect("two-body write fixture");
    let [state] = result.ir().model.feature_result_topologies.as_slice() else {
        panic!("one result state for the operation");
    };
    assert_eq!(
        state
            .bodies()
            .iter()
            .map(cadmpeg_core::text::NonBlankString::as_str)
            .collect::<Vec<_>>(),
        [
            "nx:feature-history:body-identity#0000000017",
            "nx:feature-history:body-identity#0000000018",
        ]
    );
    assert_eq!(state.native_ref, result.ir().model.features[0].native_ref);
    let (ir, report, fidelity) = result.into_parts();
    let validation =
        cadmpeg_ir::validate_neutral_with_source_fidelity(&ir, &fidelity, report.losses)
            .expect("complete model/fidelity check");
    assert!(validation.is_ok(), "{validation:#?}");
    crate::test_support::with_decode_context(|ctx| {
        assert!(<crate::NxCodec as CodecBackend>::validate_native(ctx, &ir)
            .unwrap()
            .is_empty());
    });
}
