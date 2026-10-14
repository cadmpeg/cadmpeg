// SPDX-License-Identifier: Apache-2.0
pub(super) fn group_record(xmt: u16, node_id: u32, linked_reference: u16) -> Vec<u8> {
    let mut bytes = vec![0, 90];
    bytes.extend_from_slice(&xmt.to_be_bytes());
    bytes.extend_from_slice(&node_id.to_be_bytes());
    for reference in [3u16, 4, 5, 6] {
        bytes.extend_from_slice(&reference.to_be_bytes());
        bytes.push(1);
    }
    bytes.push(4);
    bytes.extend_from_slice(&linked_reference.to_be_bytes());
    bytes.push(0);
    bytes
}
