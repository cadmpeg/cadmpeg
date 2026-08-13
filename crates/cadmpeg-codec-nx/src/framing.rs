// SPDX-License-Identifier: Apache-2.0
//! Shared framing for fixed Parasolid records.
//!
//! The frame parser resolves the optional envelope escape, every extended XMT
//! in the record's known header, and the complete logical record boundary. It
//! does not assign family semantics; topology and geometry apply their own
//! field validity gates after framing.
#![deny(clippy::disallowed_methods)]

use cadmpeg_core::decode::View;

/// One structurally complete fixed-record interpretation.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FixedRecordFrame {
    /// Record XMT identity.
    pub(crate) xmt: u32,
    /// Bytes inserted after the record type before the logical payload.
    pub(crate) shift: usize,
    /// Additional bytes inserted by extended references after the XMT.
    pub(crate) payload_shift: usize,
    /// First byte after the complete record.
    pub(crate) end: usize,
}

/// Build all complete direct and escaped interpretations at one fixed-record tag.
pub(crate) fn fixed_record_candidates(
    stream: &[u8],
    pos: usize,
    kind: u8,
    len: usize,
) -> Vec<FixedRecordFrame> {
    let mut candidates = Vec::new();
    if let Some((xmt, shift)) = read_xmt(stream, pos + 2) {
        candidates.push((xmt, shift));
    }
    if stream.get(pos + 2) == Some(&0xff) {
        if let Some((xmt, shift)) = read_xmt(stream, pos + 3) {
            candidates.push((xmt, shift + 1));
        }
    }
    candidates
        .into_iter()
        .filter_map(|(xmt, shift)| {
            // 1 is Parasolid's null reference. A record itself cannot occupy it.
            if xmt <= 1 {
                return None;
            }
            let payload_shift = payload_shift(stream, pos, kind, shift)?;
            let end = pos
                .checked_add(len)?
                .checked_add(shift)?
                .checked_add(payload_shift)?;
            stream.get(pos..end)?;
            Some(FixedRecordFrame {
                xmt,
                shift,
                payload_shift,
                end,
            })
        })
        .collect()
}

/// Return whether `end` is the stream boundary or a complete fixed-record start.
pub(crate) fn fixed_record_boundary(stream: &[u8], end: usize) -> bool {
    if end == stream.len() {
        return true;
    }
    if stream.get(end) != Some(&0) {
        return false;
    }
    let Some(&kind) = stream.get(end + 1) else {
        return false;
    };
    let Some(len) = fixed_len(kind) else {
        return false;
    };
    !fixed_record_candidates(stream, end, kind, len).is_empty()
}

pub(crate) fn read_and_advance(stream: &[u8], at: &mut usize) -> Option<u32> {
    let (value, extra) = read_xmt(stream, *at)?;
    *at += 2 + extra;
    Some(value)
}

pub(crate) fn read_sequence_at(stream: &[u8], at: &mut usize, count: usize) -> Option<Vec<u32>> {
    (0..count).map(|_| read_and_advance(stream, at)).collect()
}

/// Decode the compact and extended XMT forms. The extended form uses a negative
/// signed remainder followed by a quotient: `quotient * 32767 + remainder`.
pub(crate) fn read_xmt(stream: &[u8], at: usize) -> Option<(u32, usize)> {
    let mut view = View::over_retained(stream);
    view.seek(at)?;
    let first = view.i16_be()?;
    if first >= 0 {
        return Some((first as u32, 0));
    }
    let remainder = first.unsigned_abs();
    let quotient = view.u16_be()?;
    let value = u32::from(quotient) * 32_767 + u32::from(remainder);
    Some((value, 2))
}

/// Decode XMT and return the full encoded width (2 or 4 bytes).
///
/// Prefer this when the caller advances by the returned length. Topology keeps
/// [`read_xmt`]'s extra-bytes convention (`at += 2 + extra`) so its offsets stay
/// correct.
pub(crate) fn read_xmt_width(stream: &[u8], at: usize) -> Option<(u32, usize)> {
    let (value, extra) = read_xmt(stream, at)?;
    Some((value, 2 + extra))
}

fn payload_shift(stream: &[u8], pos: usize, kind: u8, header_shift: usize) -> Option<usize> {
    if kind == 14 {
        let mut at = pos + 8 + header_shift;
        let start = at;
        read_and_advance(stream, &mut at)?;
        at += 8;
        read_sequence_at(stream, &mut at, 5)?;
        at += 1;
        read_sequence_at(stream, &mut at, 5)?;
        return Some(at - start - 31);
    }
    if kind == 16 {
        let mut at = pos + 8 + header_shift;
        let start = at;
        read_and_advance(stream, &mut at)?;
        at += 8;
        read_sequence_at(stream, &mut at, 7)?;
        return Some(at - start - 24);
    }
    let (offset, before, trailing_bytes, after) = match kind {
        13 => (8, 8, 0, 0),
        15 => (8, 4, 0, 0),
        17 => (4, 9, 1, 0),
        18 => (8, 5, 8, 1),
        29 => (8, 4, 24, 0),
        _ => (0, 0, 0, 0),
    };
    if before != 0 {
        let mut at = pos + offset + header_shift;
        let start = at;
        read_sequence_at(stream, &mut at, before)?;
        at += trailing_bytes;
        read_sequence_at(stream, &mut at, after)?;
        let compact = before * 2 + trailing_bytes + after * 2;
        return Some(at - start - compact);
    }
    let compact_kind = matches!(
        kind,
        30..=32 | 38 | 50..=54 | 56 | 60 | 124 | 133 | 134 | 137
    );
    if !compact_kind {
        return Some(0);
    }
    let mut at = pos + 8 + header_shift;
    let start = at;
    read_sequence_at(stream, &mut at, 5)?;
    matches!(stream.get(at), Some(b'+' | b'-')).then_some(())?;
    at += 1;
    let common_extra = at - start - 11;
    let tail_start = at;
    match kind {
        38 => {
            read_sequence_at(stream, &mut at, 6)?;
        }
        56 => {
            at += 1;
            read_sequence_at(stream, &mut at, 3)?;
        }
        60 => {
            at += 2;
            read_and_advance(stream, &mut at)?;
        }
        124 | 134 => {
            read_sequence_at(stream, &mut at, 2)?;
        }
        133 => {
            read_and_advance(stream, &mut at)?;
        }
        137 => {
            read_sequence_at(stream, &mut at, 3)?;
        }
        _ => {}
    }
    let compact_tail_len = match kind {
        38 => 12,
        56 => 7,
        60 => 4,
        124 | 134 => 4,
        133 => 2,
        137 => 6,
        _ => 0,
    };
    Some(common_extra + at - tail_start - compact_tail_len)
}

pub(crate) fn fixed_len(kind: u8) -> Option<usize> {
    Some(match kind {
        12 | 13 => 24,
        14 => 39,
        15 => 16,
        16 => 32,
        17 => 23,
        18 => 28,
        19 => 16,
        29 => 40,
        30 => 67,
        31 => 99,
        32 => 107,
        38 => 31,
        50 => 91,
        51 => 99,
        52 => 115,
        53 => 99,
        54 => 107,
        56 => 66,
        60 => 31,
        124 | 134 => 23,
        133 => 85,
        137 => 33,
        _ => return None,
    })
}
