// SPDX-License-Identifier: Apache-2.0
use super::ExtractFaceEnvelope;
use crate::om::operation_record::OperationPayload;

fn prefix() -> Vec<u8> {
    let mut bytes = vec![3, 0, 0, 1, 0];
    bytes.extend([
        1, 0, 0, 0, 1, 1, 0, 0, 1, 0, 1, 0, 1, 0, 0, 1, 255, 255, 255, 255,
    ]);
    bytes.extend([0; 6]);
    // Count 3 means two ordered triples. Duplicate keys and relations are valid.
    bytes.extend([1, 3, 7, 0x80, 0, 0x90, 1, 0, 7, 0x80, 0, 0x90, 1, 0]);
    bytes.extend([1, 1, 255, 0, 0, 0, 0, 0, 0, 3, 4, 0xc3, 0xa9, 0]);
    bytes
}

#[test]
fn extract_face_prefix_preserves_duplicate_order_zero_endpoints_and_utf8_width() {
    let bytes = prefix();
    crate::test_support::with_decode_context(|_ctx| {
        let read = |bytes: &[u8]| {
            ExtractFaceEnvelope::read(OperationPayload::new(bytes, 100, "EXTRACT_FACE").unwrap())
        };
        let field = read(&bytes).expect("complete extraction fixture");
        assert_eq!(field.control_words, [1, 256, 65536, u32::MAX]);
        assert_eq!(field.byte_len(), bytes.len());
        assert_eq!(field.navigator_text(), "é");
        assert_eq!(field.lanes[0].0.len(), 2);
        assert_eq!(field.lanes[0].0[0], field.lanes[0].0[1]);
        assert_eq!(field.lanes[0].0[0].left.value(), 0);
        assert_eq!(field.lanes[0].0[0].left.raw(), [0x80, 0]);
        assert_eq!(field.lanes[0].0[0].right.value(), 256);
        assert!(field.lanes[1].0.is_empty());
        for end in 0..bytes.len() {
            assert!(read(&bytes[..end]).is_none(), "end={end}");
        }
        for (at, value) in [(0, 0), (32, 0), (32, 255), (bytes.len() - 2, 255)] {
            let mut invalid = bytes.clone();
            invalid[at] = value;
            assert!(read(&invalid).is_none());
        }
        assert!(
            ExtractFaceEnvelope::read(OperationPayload::new(&bytes, 0, "ENLARGE").unwrap())
                .is_none()
        );
    });
}
