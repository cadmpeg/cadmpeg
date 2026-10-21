// SPDX-License-Identifier: Apache-2.0
//! Codec-native validation composes placement frame admission.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::CodecBackend;
use cadmpeg_ir::native::bytes::NativeBytes;

#[test]
fn placement_native_validator_rejects_matrix_that_disagrees_with_source_bytes() {
    let mut bytes = vec![0xff, 0, 0x45, 1, 4, 1];
    bytes.extend([1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0]);
    bytes.extend([0, 0, 0, 1, 63, 0xf0, 0, 0, 0]);
    bytes.extend(b"\x03\x03A\0\x03\x2601234567-89ab-cdef-0123-456789abcdef\0");
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty();
    let mut wire = serde_json::json!({
        "id":"nx:assembly:placement#123", "raw_frame":NativeBytes::new(bytes.as_slice()),
        "matrix":[1.,0.,0.,0., 0.,1.,0.,0., 0.,0.,1.,0.],
        "label":"A", "uuid":"01234567-89ab-cdef-0123-456789abcdef",
        "source_entry":"/Root/UG_PART/UG_PART", "source_offset":123,
    });
    ir.native
        .namespace_mut("nx")
        .set_arena(&ctx, "assembly_placements", &[&wire])
        .unwrap();
    assert!(crate::NxCodec::validate_native(&ctx, &ir)
        .unwrap()
        .is_empty());
    wire["matrix"][3] = serde_json::json!(1.0);
    ir.native
        .namespace_mut("nx")
        .set_arena(&ctx, "assembly_placements", &[&wire])
        .unwrap();
    let findings = crate::NxCodec::validate_native(&ctx, &ir).unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].check,
        cadmpeg_ir::report::check::Check::NativeLinks
    );
    assert_eq!(findings[0].severity, cadmpeg_ir::report::Severity::Error);
    assert!(findings[0].message.contains("fields must match"));
}
