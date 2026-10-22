// SPDX-License-Identifier: Apache-2.0
pub(crate) const FRAMES: [&str; 5] = [
    include_str!("placement_construction/extra-scalar-v14.json"),
    include_str!("placement_construction/unmarked-v4.json"),
    include_str!("placement_construction/explicit-v4.json"),
    include_str!("placement_construction/identity-v6.json"),
    include_str!("placement_construction/unmarked-alignment-v4.json"),
];

pub(crate) fn metadata(
    frame: crate::metastream::PrimaryRecordFrame,
    version: u32,
    type_guid: &str,
    base_guid: &str,
    with_secondary: bool,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    super::lp_ascii(&mut bytes, "Design");
    bytes.extend_from_slice(&0u32.to_le_bytes());
    super::lp_utf16(&mut bytes, "11111111-2222-3333-4444-555555555555");
    for value in [1234u32, 0, 0, 0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    super::lp_ascii(&mut bytes, "Design");
    super::lp_ascii(&mut bytes, "");
    bytes.extend_from_slice(&0u64.to_le_bytes());
    bytes.extend_from_slice(&2u32.to_le_bytes());
    super::lp_ascii(&mut bytes, type_guid);
    super::lp_ascii(&mut bytes, base_guid);
    bytes.extend_from_slice(&version.to_le_bytes());
    super::lp_ascii(&mut bytes, "Fusion");
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&frame.entity_id.to_le_bytes());
    super::lp_ascii(&mut bytes, base_guid);
    super::lp_ascii(&mut bytes, "");
    bytes.extend_from_slice(&0u32.to_le_bytes());
    super::lp_ascii(&mut bytes, "Fusion");
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&frame.entity_id.to_le_bytes());
    bytes.extend_from_slice(&cadmpeg_core::decode::u64_from_index(frame.start).to_le_bytes());
    bytes.extend_from_slice(&u32::from(with_secondary).to_le_bytes());
    if with_secondary {
        bytes.extend_from_slice(&frame.entity_id.to_le_bytes());
        bytes.extend_from_slice(
            &cadmpeg_core::decode::u64_from_index(frame.member_end).to_le_bytes(),
        );
    }
    bytes
}
