// SPDX-License-Identifier: Apache-2.0
use super::{redirections_json, f3d_with_redirections_json, DecodeOptions, F3dCodec, Cursor};
use cadmpeg_ir::codec::Codec;


#[test]
fn unsupported_redirections_schema_preserves_error_kind() {
    let mut table: serde_json::Value = serde_json::from_str(&redirections_json("part.f3d", &[])).unwrap();
    table["schema-version"] = serde_json::json!(1);
    let bytes = serde_json::to_vec(&table).unwrap();
    crate::test_support::with_decode_context(|ctx| {
        let error = super::super::parse(ctx, &bytes).unwrap_err();
        assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(message)
            if message.contains("unsupported schema-version 1")));
    });
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let error = super::super::parse(ctx, &bytes).unwrap_err();
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(failure)
            if failure.operation == "describe unsupported F3D redirections schema"));
    });
    for container_only in [false, true] {
        let options = DecodeOptions { container_only, ..DecodeOptions::default() };
        let error = F3dCodec.decode(&mut Cursor::new(f3d_with_redirections_json("assembly-design", &bytes)), &options).err().unwrap();
        assert!(matches!(error, cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::NotImplemented(message))
            if message.contains("unsupported schema-version 1")));
    }
}
