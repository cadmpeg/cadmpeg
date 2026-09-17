// SPDX-License-Identifier: Apache-2.0
use super::exact_pattern_identity_wrapper;
use crate::design::test_support::dump::{lp_utf16, IndexedRecordOffsets};

#[test]
fn circular_pattern_identity_wrapper_closes_on_its_persistent_identity() {
    fn header(bytes: &mut Vec<u8>, class_tag: &str, record_index: u32) {
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(class_tag.as_bytes());
        bytes.extend_from_slice(&record_index.to_le_bytes());
    }
    fn marked(bytes: &mut Vec<u8>, record_index: u32) {
        bytes.push(1);
        bytes.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }

    let mut bytes = Vec::new();
    let record_index = 80;
    header(&mut bytes, "308", record_index);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&40u64.to_le_bytes());
    lp_utf16(&mut bytes, "384d79a0-c23e-42aa-b993-74df1f8dfcae");
    lp_utf16(&mut bytes, "352c47d7-42ba-443e-9de1-ae0e37cc129d");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    marked(&mut bytes, record_index + 1);
    header(&mut bytes, "305", record_index + 1);
    bytes.extend_from_slice(&[0; 10]);
    marked(&mut bytes, record_index + 2);
    header(&mut bytes, "300", record_index + 2);
    bytes.extend_from_slice(&[0; 10]);
    let identity_offset = bytes.len();
    bytes.extend_from_slice(&503u64.to_le_bytes());
    header(&mut bytes, "308", record_index + 3);

    assert_eq!(
        exact_pattern_identity_wrapper(&bytes, &IndexedRecordOffsets::build(&bytes), record_index,),
        Some((503, identity_offset as u64))
    );
    bytes[identity_offset - 1] = 1;
    assert_eq!(
        exact_pattern_identity_wrapper(&bytes, &IndexedRecordOffsets::build(&bytes), record_index,),
        None
    );
}
