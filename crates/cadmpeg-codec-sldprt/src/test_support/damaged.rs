// SPDX-License-Identifier: Apache-2.0
//! Synthesized §4.5 damaged-input builders for crate tests.
#![allow(clippy::unwrap_used)]

use crate::container::MARKER;
use crate::layout::block_frame_header as block_hdr;
use crate::layout::outer_header as outer_hdr;
use crate::layout::zip_central_header as central;

/// The section whose semantic record the damaged fixture gives up.
const PMI_SECTION: &str = "Contents/PMISemanticDataDB";

/// Undo the nibble swap the container stores section names with.
fn unswap_name(stored: &[u8]) -> Option<String> {
    let unswapped: Vec<u8> = stored
        .iter()
        .map(|byte| (byte << 4) | (byte >> 4))
        .collect();
    String::from_utf8(unswapped).ok()
}

/// One local block frame: its marker offset and the layout of its fields.
struct LocalFrame {
    offset: usize,
    comp_start: usize,
    comp_len: usize,
    uncomp_len: usize,
    crc_at: usize,
}

/// Walk the outer container and return the local frame of the named section.
fn local_frame(source: &[u8], section: &str) -> Option<LocalFrame> {
    let mut at = outer_hdr::LEN;
    while at + block_hdr::LEN <= source.len() {
        if source.get(at..at + MARKER.len()) != Some(MARKER.as_slice()) {
            at += 1;
            continue;
        }
        let field = |offset: usize| {
            let bytes: [u8; 4] = source.get(at + offset..at + offset + 4)?.try_into().ok()?;
            Some(u32::from_le_bytes(bytes) as usize)
        };
        let pre_sz = field(block_hdr::PRE_SZ)?;
        let name_start = at + block_hdr::LEN;
        let name_end = name_start.checked_add(pre_sz)?;
        let name = unswap_name(source.get(name_start..name_end)?)?;
        let comp_len = field(block_hdr::COMP_SZ)?;
        let comp_start = name_end;
        let comp_end = comp_start.checked_add(comp_len)?;
        if comp_end > source.len() {
            return None;
        }
        if name == section {
            return Some(LocalFrame {
                offset: at,
                comp_start,
                comp_len,
                uncomp_len: field(block_hdr::UNCOMP_SZ)?,
                crc_at: at + block_hdr::CRC32,
            });
        }
        at = comp_end;
    }
    panic!("damaged fixture base has no {section} block");
}

/// Inflate one raw-deflate member, as the container reads this block.
fn inflate_member(compressed: &[u8], uncomp_len: usize) -> Vec<u8> {
    use std::io::Read;
    let mut decoder = flate2::read::DeflateDecoder::new(compressed);
    let mut inflated = Vec::new();
    decoder
        .read_to_end(&mut inflated)
        .expect("damaged fixture base PMI block inflates");
    assert_eq!(
        inflated.len(),
        uncomp_len,
        "damaged fixture base PMI block states its inflated length"
    );
    inflated
}

/// Compress with the same raw-deflate framing the fixture builders use.
fn deflate_member(inflated: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut encoder =
        flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    encoder
        .write_all(inflated)
        .expect("damaged fixture recompresses");
    encoder.finish().expect("damaged fixture finishes deflate")
}

/// Patch the central-directory mirror of the local frame's header CRC.
///
/// The tail directory mirrors every local header; the mirror must agree or
/// the envelope refuses the image as a cut tail.
fn patch_central_crc(source: &mut [u8], frame: &LocalFrame, crc: u32) {
    let mut patched = false;
    let last = source.len().checked_sub(central::LEN).expect("tail room");
    let mut start = outer_hdr::LEN;
    while start <= last {
        let marker_at = start + central::VERSION_NEEDED;
        if source.get(marker_at..marker_at + MARKER.len()) == Some(MARKER.as_slice())
            && source.get(start + central::VERSION_MADE_BY..start + central::VERSION_MADE_BY + 2)
                == Some(&[0, 0])
        {
            let relative = u32::from_le_bytes(
                source[start + central::LOCAL_OFFSET..start + central::LOCAL_OFFSET + 4]
                    .try_into()
                    .expect("central offset"),
            ) as usize;
            let local = relative + outer_hdr::LEN + 4;
            if local == frame.offset {
                let mirror = start + central::VERSION_NEEDED;
                assert_eq!(
                    &source[mirror..mirror + block_hdr::LEN],
                    &source[frame.offset..frame.offset + block_hdr::LEN],
                    "central entry mirrors the PMI block header"
                );
                source[mirror + block_hdr::CRC32..mirror + block_hdr::CRC32 + 4]
                    .copy_from_slice(&crc.to_le_bytes());
                patched = true;
            }
        }
        start += 1;
    }
    assert!(patched, "central directory mirrors the PMI block");
}

/// A copy of the committed PMI fixture whose semantic record declares an
/// empty `dimItems` array while the item bytes follow.
///
/// The record still opens with its map header after its GUID key, so the
/// scan admits it as a candidate and the parse fails it as malformed; the
/// decoder gives up exactly that `sldprt.pmi_record` part with one
/// `record.unreadable` loss. The deflate length is unchanged, so the tail
/// directory still selects its envelope.
pub(crate) fn damaged_pmi_record_sldprt() -> Vec<u8> {
    let base = include_bytes!("../../tests/golden/fixtures/authored_pmi_dimension.sldprt").to_vec();
    let frame = local_frame(&base, PMI_SECTION).expect("damaged fixture base holds its PMI block");
    let compressed = base[frame.comp_start..frame.comp_start + frame.comp_len].to_vec();
    let mut inflated = inflate_member(&compressed, frame.uncomp_len);
    let key = [0xa8]
        .into_iter()
        .chain("dimItems".bytes())
        .collect::<Vec<_>>();
    let key_at = inflated
        .windows(key.len())
        .position(|window| window == key.as_slice())
        .expect("PMI record carries its dimItems key");
    let header_at = key_at + key.len();
    assert_eq!(
        inflated[header_at], 0x91,
        "the dimItems array declares its one item"
    );
    inflated[header_at] = 0x90;
    let recompressed = deflate_member(&inflated);
    assert_eq!(
        recompressed.len(),
        frame.comp_len,
        "the damaged PMI block keeps its compressed length"
    );
    let crc = crc32fast::hash(&inflated);
    let mut damaged = base;
    damaged[frame.comp_start..frame.comp_start + frame.comp_len].copy_from_slice(&recompressed);
    patch_central_crc(&mut damaged, &frame, crc);
    damaged[frame.crc_at..frame.crc_at + 4].copy_from_slice(&crc.to_le_bytes());
    damaged
}
