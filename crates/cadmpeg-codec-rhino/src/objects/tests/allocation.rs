// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn skipped_uuid_lists_validate_the_bounded_prefix_without_allocating_values() {
    let archive = ArchiveVersion::V5;
    let mut body = 1_i32.to_le_bytes().to_vec();
    body.extend(0_i32.to_le_bytes());
    body.extend(2_i32.to_le_bytes());
    body.extend([7_u8; 32]);
    let bytes = crc_chunk(archive, 0x4000_8000, &body);
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
    crate::objects::skip_uuid_list(&mut reader, archive).expect("bounded UUID prefix");
    assert_eq!(reader.position(), bytes.len());
    body[8..12].copy_from_slice(&3_i32.to_le_bytes());
    let bytes = crc_chunk(archive, 0x4000_8000, &body);
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
    assert!(crate::objects::skip_uuid_list(&mut reader, archive).is_err());
}
