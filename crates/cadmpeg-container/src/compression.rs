// SPDX-License-Identifier: Apache-2.0
//! Compression helpers shared by container codecs.

use std::io::Read;

use flate2::read::ZlibDecoder;

/// Inflate a zlib member, accepting a decoded prefix when its trailing input is
/// truncated or contains bytes from a following packed stream.
pub fn inflate_zlib_prefix(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut decoder = ZlibDecoder::new(bytes);
    let mut output = Vec::new();
    match decoder.read_to_end(&mut output) {
        Ok(_) => Some(output),
        Err(_) if !output.is_empty() => Some(output),
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use flate2::{write::ZlibEncoder, Compression};

    use super::inflate_zlib_prefix;

    #[test]
    fn inflates_complete_member_with_trailing_bytes() {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(b"parasolid")
            .expect("writing to an in-memory zlib encoder succeeds");
        let mut compressed = encoder
            .finish()
            .expect("finishing an in-memory zlib encoder succeeds");
        compressed.extend_from_slice(b"next stream");
        assert_eq!(
            inflate_zlib_prefix(&compressed).as_deref(),
            Some(b"parasolid".as_slice())
        );
    }
}
