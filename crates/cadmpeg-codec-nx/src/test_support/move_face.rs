// SPDX-License-Identifier: Apache-2.0
//! Synthetic complete native core for parser and record-wire admission tests.

pub(crate) fn core(entries: u8) -> Vec<u8> {
    let mut bytes = b"\x0f\0\0\x01\0\x52\0\xff\xff\x12\xf0\x01\0\xf0\x02\x13\x01\x01\x06\xff\xf0\x03\xff\0\0\0\xf0\x04\0\xff\xff\xff\xff\xff\xff\xff\xff\xff\0\0\0\0".to_vec();
    bytes.extend([1, entries + 1]);
    for i in 0..entries {
        bytes.extend([11 + 2 * i, 20 + i, 0x97, 0x75]);
    }
    bytes.extend([1, entries + 1]);
    for i in 0..entries {
        bytes.extend([16, 40 + i, 255]);
        bytes.extend(b"\0\0\0\x0b\0\xf0\x01\x02\x02");
        bytes.extend([60 + i, 1]);
    }
    bytes.extend(b"\0\0\0\0\0\0\x0d\x01\x02\x01\0\0\0\x89\x6c\x01\x01\x01\x01\x02\xa5\xa5\x51\xa5\xa5\x51\x01\0\0\xff\0\xff\x09\xc0\x1f\xff\xff\x01\0\0\x81\x72\x80\x86\x01\x03\x02\x01\x01\x01\0\0\x01\0\0\0\x22\x22\x41\0");
    bytes
}
