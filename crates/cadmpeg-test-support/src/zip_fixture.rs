// SPDX-License-Identifier: Apache-2.0
//! Raw ZIP fixture assembly, for compression methods no enabled `zip` feature can write.
//!
//! The workspace builds `zip` without its `zstd` feature (it is a libzstd binding), which removes
//! both the `CompressionMethod::Zstd` variant and `ZipWriter`'s ability to emit method 93.  A
//! test that needs a Zstandard entry therefore has to lay the archive out itself.
//!
//! Only the parts a fixture needs are written: no ZIP64, no data descriptors, no archive
//! comment, one disk; flags and an extra field only where an entry asks for them.  The readers
//! these fixtures are fed to — `zip::ZipArchive` and `cadmpeg-container`'s
//! own central-directory and local-header parsers — share no code with this module, so they,
//! not it, decide whether the layout conforms.

/// One entry, with its payload already in the form the compression method stores.
#[derive(Debug, Clone)]
pub struct RawEntry {
    /// Entry name, written unflagged, so it must be ASCII (bit 11 is left clear).
    pub name: String,
    /// ZIP compression method number: 0 stored, 8 deflate, 93 Zstandard, 99 AE-x.
    pub method: u16,
    /// The bytes as stored in the archive, already compressed by `method`.
    pub payload: Vec<u8>,
    /// Size of the payload after decompression.
    pub uncompressed_size: u32,
    /// CRC-32 of the payload after decompression.
    pub crc32: u32,
    /// General-purpose bit flag.  Bit 0 is the encrypted bit; a reader must refuse such an
    /// entry, and something has to be able to build one for that refusal to have a fixture.
    pub flags: u16,
    /// Extra field, written into BOTH headers.  The 0x9901 AE-x record lives here: it names the
    /// real compression method for an entry whose method field says 99, which is how an
    /// encrypted entry can present with the encrypted bit CLEAR.
    pub extra: Vec<u8>,
}

impl RawEntry {
    /// Builds a `Stored` (method 0) entry from its plaintext.
    #[must_use]
    pub fn stored(name: &str, plain: &[u8]) -> Self {
        Self {
            name: name.to_owned(),
            method: STORED,
            payload: plain.to_vec(),
            uncompressed_size: u32::try_from(plain.len()).expect("fixture entry fits u32"),
            crc32: crc32(plain),
            flags: 0,
            extra: Vec::new(),
        }
    }

    /// Builds an entry from an already-compressed payload and the plaintext it expands to.
    #[must_use]
    pub fn compressed(name: &str, method: u16, payload: &[u8], plain: &[u8]) -> Self {
        Self {
            name: name.to_owned(),
            method,
            payload: payload.to_vec(),
            uncompressed_size: u32::try_from(plain.len()).expect("fixture entry fits u32"),
            crc32: crc32(plain),
            flags: 0,
            extra: Vec::new(),
        }
    }

    /// Sets the general-purpose bit flag (bit 0 is "encrypted").
    #[must_use]
    pub fn with_flags(mut self, flags: u16) -> Self {
        self.flags = flags;
        self
    }

    /// Declares this entry AE-x encrypted: method 99 plus a 0x9901 extra field naming
    /// `inner_method` as the real one.
    ///
    /// The general-purpose encrypted bit is deliberately NOT set, because that is the shape that
    /// slips past a reader checking only the flag IF it takes the method from `zip`: `zip`
    /// rewrites `compression_method` from the extra field, so the entry then presents as ordinary
    /// Stored or Deflate.  A reader that takes the method from the central record sees 99.
    #[must_use]
    pub fn aes_without_the_encrypted_bit(mut self, inner_method: u16) -> Self {
        self.extra = Vec::new();
        self.extra.extend_from_slice(&0x9901_u16.to_le_bytes()); // header id
        self.extra.extend_from_slice(&7_u16.to_le_bytes()); // data size
        self.extra.extend_from_slice(&2_u16.to_le_bytes()); // AE-2
        self.extra.extend_from_slice(b"AE"); // vendor id
        self.extra.push(3); // 256-bit key
        self.extra.extend_from_slice(&inner_method.to_le_bytes());
        self.method = 99;
        self
    }
}

/// ZIP compression method number for a stored entry.
pub const STORED: u16 = 0;
/// ZIP compression method number for a raw-DEFLATE entry.
pub const DEFLATE: u16 = 8;
/// ZIP compression method number for a Zstandard entry (APPNOTE 6.3.7).
pub const ZSTD: u16 = 93;

/// Lays the entries out as one ZIP archive: local headers, then the central directory, then EOCD.
#[must_use]
pub fn archive(entries: &[RawEntry]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut offsets = Vec::with_capacity(entries.len());
    for entry in entries {
        offsets.push(u32::try_from(out.len()).expect("fixture archive fits u32"));
        let name = entry.name.as_bytes();
        let compressed = u32::try_from(entry.payload.len()).expect("fixture payload fits u32");
        out.extend_from_slice(&0x0403_4b50_u32.to_le_bytes()); // local file header signature
        out.extend_from_slice(&20_u16.to_le_bytes()); // version needed to extract
        out.extend_from_slice(&entry.flags.to_le_bytes());
        out.extend_from_slice(&entry.method.to_le_bytes());
        out.extend_from_slice(&0_u16.to_le_bytes()); // last mod time
        out.extend_from_slice(&0_u16.to_le_bytes()); // last mod date
        out.extend_from_slice(&entry.crc32.to_le_bytes());
        out.extend_from_slice(&compressed.to_le_bytes());
        out.extend_from_slice(&entry.uncompressed_size.to_le_bytes());
        out.extend_from_slice(
            &u16::try_from(name.len())
                .expect("name fits u16")
                .to_le_bytes(),
        );
        out.extend_from_slice(
            &u16::try_from(entry.extra.len())
                .expect("extra fits u16")
                .to_le_bytes(),
        );
        out.extend_from_slice(name);
        out.extend_from_slice(&entry.extra);
        out.extend_from_slice(&entry.payload);
    }
    let central_start = u32::try_from(out.len()).expect("fixture archive fits u32");
    for (entry, offset) in entries.iter().zip(&offsets) {
        let name = entry.name.as_bytes();
        let compressed = u32::try_from(entry.payload.len()).expect("fixture payload fits u32");
        out.extend_from_slice(&0x0201_4b50_u32.to_le_bytes()); // central file header signature
        out.extend_from_slice(&20_u16.to_le_bytes()); // version made by
        out.extend_from_slice(&20_u16.to_le_bytes()); // version needed to extract
        out.extend_from_slice(&entry.flags.to_le_bytes());
        out.extend_from_slice(&entry.method.to_le_bytes());
        out.extend_from_slice(&0_u16.to_le_bytes()); // last mod time
        out.extend_from_slice(&0_u16.to_le_bytes()); // last mod date
        out.extend_from_slice(&entry.crc32.to_le_bytes());
        out.extend_from_slice(&compressed.to_le_bytes());
        out.extend_from_slice(&entry.uncompressed_size.to_le_bytes());
        out.extend_from_slice(
            &u16::try_from(name.len())
                .expect("name fits u16")
                .to_le_bytes(),
        );
        out.extend_from_slice(
            &u16::try_from(entry.extra.len())
                .expect("extra fits u16")
                .to_le_bytes(),
        );
        out.extend_from_slice(&0_u16.to_le_bytes()); // file comment length
        out.extend_from_slice(&0_u16.to_le_bytes()); // disk number start
        out.extend_from_slice(&0_u16.to_le_bytes()); // internal file attributes
        out.extend_from_slice(&0_u32.to_le_bytes()); // external file attributes
        out.extend_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(name);
        out.extend_from_slice(&entry.extra);
    }
    let central_size = u32::try_from(out.len()).expect("fixture archive fits u32") - central_start;
    let count = u16::try_from(entries.len()).expect("fixture entry count fits u16");
    out.extend_from_slice(&0x0605_4b50_u32.to_le_bytes()); // end of central directory signature
    out.extend_from_slice(&0_u16.to_le_bytes()); // number of this disk
    out.extend_from_slice(&0_u16.to_le_bytes()); // disk with the central directory
    out.extend_from_slice(&count.to_le_bytes()); // entries on this disk
    out.extend_from_slice(&count.to_le_bytes()); // entries in total
    out.extend_from_slice(&central_size.to_le_bytes());
    out.extend_from_slice(&central_start.to_le_bytes());
    out.extend_from_slice(&0_u16.to_le_bytes()); // archive comment length
    out
}

/// A Zstandard frame produced by the reference implementation.
///
/// Written by the Zstandard CLI v1.5.7 (`zstd -19 --check`) over [`zstd_golden_plaintext`], on
/// 2026-08-22, and committed as bytes.  Its provenance is the point: a frame this repository's
/// own decoder produced would make every test that reads it a round trip, which cannot see the
/// two implementations disagreeing about the format.  405 bytes, real compressed blocks (the
/// plaintext is 19 968), frame-descriptor bit 2 set, so decoding it also exercises the xxhash64
/// content checksum.
pub const ZSTD_GOLDEN_FRAME: &[u8] = include_bytes!("../fixtures/zstd-golden.zst");

/// The plaintext [`ZSTD_GOLDEN_FRAME`] decodes to, generated rather than stored.
///
/// Generating it here rather than committing it beside the frame is what makes the comparison
/// worth making: the expected bytes and the frame do not come from one file.
#[must_use]
pub fn zstd_golden_plaintext() -> Vec<u8> {
    let mut out = Vec::with_capacity(512 * 39);
    for line in 0..512 {
        out.extend_from_slice(format!("cadmpeg zstd interop fixture line {line:04}\n").as_bytes());
    }
    out
}

/// The same plaintext as [`ZSTD_GOLDEN_FRAME`], compressed with NO content checksum.
///
/// This is the REPRESENTATIVE case, not the exotic one, which is why it is committed rather than
/// derived: libzstd's `ZSTD_c_checksumFlag` defaults to 0, and `zip`'s own zstd writer -- the one
/// the workspace disables -- builds entries with a bare `zstd::stream::write::Encoder`, so every
/// method-93 entry it ever produced carries no checksum.  A decoder's checksum comparison is
/// therefore inert on the input it will actually meet, and the arm that skips it needs a fixture
/// of its own.  `zstd -19 --no-check`, v1.5.7, 2026-08-22; 401 bytes, frame-descriptor bit 2
/// clear.
pub const ZSTD_GOLDEN_NOCHECK_FRAME: &[u8] = include_bytes!("../fixtures/zstd-golden-nocheck.zst");

/// A larger Zstandard frame from the reference implementation: 17 876 bytes over a 200 000-byte
/// plaintext.
///
/// The small golden is under one 16 KiB read on the way OUT, which makes it blind to the classic
/// `Read` misuse -- treating a short read as end of stream.  Measured, the small golden's read
/// sequence is `16384, 16384, 3584, 0`, so its only short read is the last one; this one spans
/// thirteen reads.  Written by the Zstandard CLI v1.5.7 (`zstd -19 --check`) on 2026-08-22, so
/// the large-payload test is fed by a reference frame too and is not a round trip.
pub const ZSTD_GOLDEN_LARGE_FRAME: &[u8] = include_bytes!("../fixtures/zstd-golden-large.zst");

/// The plaintext [`ZSTD_GOLDEN_LARGE_FRAME`] decodes to, generated rather than stored.
///
/// Deliberately varied rather than a run of one byte: a uniform payload compresses to almost
/// nothing and would not exercise multi-block decoding on the way out.
#[must_use]
pub fn zstd_golden_large_plaintext() -> Vec<u8> {
    let mut out = Vec::with_capacity(200_000);
    for line in 0_u32..200_000 / 40 {
        let salt = line.wrapping_mul(2_654_435_761) & 0xffff;
        out.extend_from_slice(
            format!("cadmpeg large fixture line {line:06} {salt:05x}\n").as_bytes(),
        );
    }
    out.truncate(200_000);
    out
}

/// Wraps `plain` in a Zstandard frame WITHOUT compressing it: one single-segment frame of
/// raw blocks, no content checksum.
///
/// For tests that need some valid method-93 payload for bytes of their own choosing, which
/// the committed goldens cannot provide and the workspace cannot compress (it links no Zstandard
/// encoder).  It is laid out by hand from RFC 8878 and is NOT evidence of interoperability:
/// only the `ZSTD_GOLDEN_*` frames come from the reference implementation.  The Zstandard CLI
/// v1.5.5 decodes this function's output for 0, 8 and 300 000 bytes (2026-10-12).
#[must_use]
pub fn zstd_stored_frame(plain: &[u8]) -> Vec<u8> {
    let mut out = vec![0x28, 0xb5, 0x2f, 0xfd];
    out.push(0xe0); // single segment, eight-byte Frame_Content_Size, no checksum, no dictionary
    out.extend_from_slice(
        &u64::try_from(plain.len())
            .expect("length fits u64")
            .to_le_bytes(),
    );
    push_raw_blocks(&mut out, plain);
    out
}

/// Wraps `plain` in a Zstandard frame WITHOUT compressing it, as a streaming encoder frames
/// it: a declared window of `2^window_log` bytes, no `Frame_Content_Size`, raw blocks, no
/// content checksum.
///
/// A decoder of this frame buffers the whole window, which is what the window admission of
/// `cadmpeg_core`'s `open_zstd` measures. `window_log` is 17 or more, so a 128 KiB raw block
/// fits the frame's block maximum. Laid out by hand from RFC 8878, like
/// [`zstd_stored_frame`].
#[must_use]
pub fn zstd_streamed_frame(window_log: u8, plain: &[u8]) -> Vec<u8> {
    assert!((17..=41).contains(&window_log), "windowLog {window_log}");
    let mut out = vec![0x28, 0xb5, 0x2f, 0xfd];
    out.push(0x00); // no Frame_Content_Size, not single segment, no checksum, no dictionary
    out.push((window_log - 10) << 3);
    push_raw_blocks(&mut out, plain);
    out
}

/// Appends `plain` as raw blocks of at most 128 KiB, the last one flagged.
fn push_raw_blocks(out: &mut Vec<u8>, plain: &[u8]) {
    const BLOCK: usize = 128 * 1024;
    let mut blocks = plain.chunks(BLOCK).peekable();
    if blocks.peek().is_none() {
        out.extend_from_slice(&[1, 0, 0]); // one empty raw block, last
    }
    while let Some(block) = blocks.next() {
        let last = u32::from(blocks.peek().is_none());
        let header = u32::try_from(block.len()).expect("block fits") << 3 | last;
        out.extend_from_slice(&header.to_le_bytes()[..3]);
        out.extend_from_slice(block);
    }
}

/// CRC-32 (IEEE, the ZIP polynomial) of `bytes`, computed here so this crate needs no dependency.
#[must_use]
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFF_u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = 0_u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_golden_frame_and_its_plaintext_are_the_committed_pair() {
        // Pins both halves against the reference run that produced them: the frame's size and
        // magic, and the plaintext's length and digest-by-construction.  A regenerated frame
        // that decodes to something else fails here rather than in a decoder test, where it
        // would read as a decoder defect.
        assert_eq!(ZSTD_GOLDEN_FRAME.len(), 405);
        assert_eq!(&ZSTD_GOLDEN_FRAME[..4], &[0x28, 0xb5, 0x2f, 0xfd]);
        assert_eq!(
            ZSTD_GOLDEN_FRAME[4] & 0x04,
            0x04,
            "frame descriptor must keep the content-checksum bit set"
        );
        let plain = zstd_golden_plaintext();
        assert_eq!(plain.len(), 19_968);
        assert_eq!(crc32(&plain), 0x0361_5668);

        // The no-checksum twin: same plaintext, checksum bit CLEAR.
        assert_eq!(ZSTD_GOLDEN_NOCHECK_FRAME.len(), 401);
        assert_eq!(&ZSTD_GOLDEN_NOCHECK_FRAME[..4], &[0x28, 0xb5, 0x2f, 0xfd]);
        assert_eq!(
            ZSTD_GOLDEN_NOCHECK_FRAME[4] & 0x04,
            0,
            "the no-check frame must keep the content-checksum bit CLEAR"
        );

        // The large pair, pinned the same way.
        assert_eq!(ZSTD_GOLDEN_LARGE_FRAME.len(), 17_876);
        assert_eq!(&ZSTD_GOLDEN_LARGE_FRAME[..4], &[0x28, 0xb5, 0x2f, 0xfd]);
        let large = zstd_golden_large_plaintext();
        assert_eq!(large.len(), 200_000);
        assert_eq!(crc32(&large), 0x0f72_6cb6);
    }

    #[test]
    fn the_stored_frame_has_the_layout_its_documentation_claims() {
        assert_eq!(
            zstd_stored_frame(b"geometry"),
            [
                &[0x28, 0xb5, 0x2f, 0xfd, 0xe0][..],
                &8_u64.to_le_bytes(),
                &[0x41, 0, 0],
                b"geometry"
            ]
            .concat()
        );
        assert_eq!(zstd_stored_frame(b"").len(), 4 + 1 + 8 + 3);
        // Two full blocks and a partial one: three headers, the last one flagged.
        let frame = zstd_stored_frame(&vec![7; 300_000]);
        assert_eq!(frame.len(), 13 + 3 * 3 + 300_000);
        assert_eq!(frame[13] & 1, 0);
        assert_eq!(frame[13 + 3 + 131_072 + 3 + 131_072] & 1, 1);
    }

    #[test]
    fn crc32_matches_the_published_check_value() {
        // The IEEE CRC-32 check value: "123456789" is 0xCBF43926 by definition, so this is a
        // control on the table-free implementation above rather than on any fixture.
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }
}
