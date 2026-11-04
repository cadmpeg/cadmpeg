// SPDX-License-Identifier: Apache-2.0

use super::blend_history::primary_partition;
use super::test_bytes::zlib_compress;
use super::test_om::composed_feature_history_section;
use super::test_prt::prt_with_named_payloads;

pub(crate) fn prt() -> Vec<u8> {
    let operations = (0..2_u8)
        .map(|index| {
            let mut branch = vec![17, 0, 0, 1, 0];
            branch.extend([
                0x2f, 0x03, 0xec, 0x46, 0x0e, 0xd8, 0x0a, 0x18, 0x2f, 0x60, 0x62, 0x4d, 0xd2, 0xf1,
                0xa9, 0xfc, 0x2f, 0x14, 0xf8, 0xb5, 0x88, 0xe3, 0x68, 0xf1, 0x2f, 0x04, 0xf8, 0xb5,
                0x88, 0xe3, 0x68, 0xf1,
            ]);
            if index == 0 {
                branch.extend([18, 1, 4, 0xf0, 1, 0xf0, 2, 0xf0, 3]);
                branch.extend([1, 3, 0, 1, 0, 1, 0, 0]);
                branch.extend([0xff, 1, 2, 0xf0, 4, 0]);
            } else {
                branch.extend([0, 18, 1, 2, 0xf0, 1]);
                branch.extend([0; 5]);
                branch.extend([0xff, 1, 2, 0xf0, 2, 0]);
            }
            branch.extend([1, 2, 17, 49 + index, 0x97, 0x75, 1, 2, 16, 65, 0xff]);
            ([0xff; 4], "SWP104", branch)
        })
        .collect::<Vec<_>>();
    let borrowed = operations
        .iter()
        .map(|(slots, label, payload)| (slots.as_slice(), *label, payload.clone()))
        .collect::<Vec<_>>();
    let history = composed_feature_history_section(&borrowed);
    let mut partition = primary_partition();
    for index in 0..2_u16 {
        partition.extend_from_slice(&90_u16.to_be_bytes());
        partition.extend_from_slice(&(800 + index).to_be_bytes());
        partition.extend_from_slice(&u32::from(49 + index).to_be_bytes());
        for _ in 0..4 {
            partition.extend_from_slice(&1_u16.to_be_bytes());
        }
        partition.push(4);
        partition.extend_from_slice(&1_u16.to_be_bytes());
    }
    partition.extend_from_slice(&[0, 1, 0, 1]);
    let compressed = zlib_compress(&partition);
    let wrapper = 48 + u32::try_from(history.len()).unwrap();
    let mut payload = [48, 9, 11, 1, 1, 48, wrapper, 0, 17, 65, 19, 0]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    payload.extend(history);
    payload.extend_from_slice(&0x8000_0000_u32.to_le_bytes());
    payload.extend_from_slice(&u32::try_from(compressed.len()).unwrap().to_le_bytes());
    payload.extend(compressed);
    prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)])
}
