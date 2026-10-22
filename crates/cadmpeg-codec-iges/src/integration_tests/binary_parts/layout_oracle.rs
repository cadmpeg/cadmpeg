// SPDX-License-Identifier: Apache-2.0
//! Independent source framing for the last Point in the literal Binary controls.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};

pub(super) fn peer_parameter(bytes: &[u8], ordinal: u32) -> (String, u64, u64) {
    let arena = DecodeArena::new();
    let (ctx, source) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let flag = crate::layout::binary_flag::SECTION_DISPLACEMENTS;
    let mut displacements = source
        .child(
            flag,
            crate::layout::binary_flag::SECTION_DISPLACEMENTS + 6 * (1 + size_of::<u32>()),
        )
        .unwrap();
    let mut start = source.start();
    let mut directory = None;
    let mut parameter = None;
    for expected in b"BSGDPT" {
        assert_eq!(displacements.req_u8().unwrap(), *expected);
        let length = usize::try_from(displacements.req_u32_be().unwrap()).unwrap();
        let section = source.child(start, start + length).unwrap();
        if matches!(expected, b'D' | b'P') {
            let mut header = section;
            assert_eq!(header.req_u8().unwrap(), *expected);
            let payload_len = usize::try_from(header.req_u32_be().unwrap()).unwrap();
            let payload = section
                .child(
                    header.start() + 1 + size_of::<u32>(),
                    header.start() + 1 + size_of::<u32>() + payload_len,
                )
                .unwrap();
            if *expected == b'D' {
                directory = Some(payload);
            } else {
                parameter = Some(payload);
            }
        }
        start += length;
    }
    let primitive_lengths = crate::layout::binary_flag::PRIMITIVE_BIT_LENGTHS;
    let mut lengths = source
        .child(primitive_lengths, primitive_lengths + 2)
        .unwrap();
    assert_eq!(lengths.req_u8().unwrap(), 32);
    assert_eq!(lengths.req_u8().unwrap(), 32);
    let directory = directory.unwrap();
    let mut cursor = directory.start();
    let mut actual_ordinal = 1;
    let mut owner = None;
    while cursor < directory.end() {
        let record = counted(directory, cursor);
        if actual_ordinal == ordinal {
            owner = Some(u32::try_from(record.start() - directory.start() + 1).unwrap());
        }
        actual_ordinal += 2;
        cursor = record.end();
    }
    assert_eq!(cursor, directory.end());
    let parameter = parameter.unwrap();
    cursor = parameter.start();
    let mut last = None;
    while cursor < parameter.end() {
        let record = counted(parameter, cursor);
        cursor = record.end();
        last = Some(record);
    }
    assert_eq!(cursor, parameter.end());
    let record = last.unwrap();
    let mut body = record;
    body.req_u32_be().unwrap();
    assert_eq!(body.req_u8().unwrap(), 1);
    assert_eq!(body.req_u32_be().unwrap(), 116);
    assert_eq!(body.req_u8().unwrap(), 5);
    assert_eq!(body.req_u32_be().unwrap(), owner.unwrap());
    let extent = ctx.capture_part_extent(record).unwrap();
    (
        extent.stream().to_owned(),
        extent.offset(),
        extent.byte_len(),
    )
}

fn counted(source: View<'_>, cursor: usize) -> View<'_> {
    let mut header = source.child(cursor, source.end()).unwrap();
    let length = usize::try_from(header.req_u32_be().unwrap()).unwrap();
    source
        .child(cursor, cursor + size_of::<u32>() + length)
        .unwrap()
}
