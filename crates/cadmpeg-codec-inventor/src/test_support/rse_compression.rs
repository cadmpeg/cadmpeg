// SPDX-License-Identifier: Apache-2.0

use flate2::{write::ZlibEncoder, Compression};
use std::io::Write as _;

pub(crate) fn encoded(method: u8, body: &[u8]) -> Vec<u8> {
    if method == 1 {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(body).expect("zlib input");
        return encoder.finish().expect("zlib member");
    }
    assert_eq!(method, 2);
    // Zstandard single-segment frame, four-byte content size, one raw last block.
    // No encoder dependency: this also runs with PR #197's shared ruzstd decoder.
    let len = u32::try_from(body.len()).expect("fixture size");
    assert!(len < 128 * 1024);
    let mut bytes = vec![0x28, 0xb5, 0x2f, 0xfd, 0xa0];
    bytes.extend_from_slice(&len.to_le_bytes());
    bytes.extend_from_slice(&((len << 3) | 1).to_le_bytes()[..3]);
    bytes.extend_from_slice(body);
    bytes
}
