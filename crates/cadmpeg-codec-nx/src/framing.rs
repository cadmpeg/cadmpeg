// SPDX-License-Identifier: Apache-2.0
//! Shared framing for fixed Parasolid records.
//!
//! The frame parser resolves the optional envelope escape, every extended XMT
//! in the record's known header, and the complete logical record boundary. It
//! does not assign family semantics; topology and geometry apply their own
//! field validity gates after framing.
#![deny(clippy::disallowed_methods)]

use crate::framing::node_kind::NodeKind;
use crate::framing::xmt_reference::NonNullXmt;
use cadmpeg_core::decode::View;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) mod node_kind;
pub(crate) mod xmt_reference;

use crate::layout::analytic_common_header as analytic;
use crate::layout::circle_payload as circle;
use crate::layout::cone_payload as cone;
use crate::layout::cylinder_payload as cylinder;
use crate::layout::edge_node as edge;
use crate::layout::ellipse_payload as ellipse;
use crate::layout::face_node as face;
use crate::layout::fin_node as fin;
use crate::layout::intersection_type_38 as intersection;
use crate::layout::line_payload as line;
use crate::layout::loop_node;
use crate::layout::offset_surf_payload as offset_surf;
use crate::layout::plane_payload as plane;
use crate::layout::point_node as point;
use crate::layout::shell_node as shell;
use crate::layout::sp_curve_payload as sp_curve;
use crate::layout::sphere_payload as sphere;
use crate::layout::torus_payload as torus;
use crate::layout::trimmed_curve_payload as trimmed;
use crate::layout::vertex_node as vertex;

/// One structurally complete fixed-record interpretation.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FixedRecordFrame {
    xmt: NonNullXmt,
    shift: usize,
    payload_shift: usize,
    end: usize,
}

impl FixedRecordFrame {
    pub(crate) fn xmt(self) -> NonNullXmt { self.xmt }
    pub(crate) fn shift(self) -> usize { self.shift }
    pub(crate) fn payload_shift(self) -> usize { self.payload_shift }
    pub(crate) fn end(self) -> usize { self.end }
}

/// The framing grammar admits at most the direct and escaped readings at one
/// type-tag offset. Keep both slots inline so a rejected probe does not allocate
/// while a whole stream is scanned.
type FixedRecordCandidates = [Option<FixedRecordFrame>; 2];

/// Build all complete direct and escaped interpretations at one fixed-record tag.
pub(crate) fn fixed_record_candidates(
    stream: &[u8],
    pos: usize,
    kind: NodeKind,
) -> FixedRecordCandidates {
    let mut candidates = [None; 2];
    let len = fixed_len(kind);
    let Some(identity_at) = pos.checked_add(2) else { return candidates; };
    if stream.get(pos..identity_at) != Some(&[0, kind.code()]) { return candidates; }
    if let Some((xmt, shift)) = read_xmt(stream, identity_at) {
        candidates[0] = complete_frame(stream, pos, kind, len, xmt, shift);
    }
    if stream.get(identity_at) == Some(&0xff) {
        let Some(escaped_at) = identity_at.checked_add(1) else { return candidates; };
        if let Some((xmt, shift)) = read_xmt(stream, escaped_at) {
            candidates[1] = complete_frame(stream, pos, kind, len, xmt, shift + 1);
        }
    }
    candidates
}

fn complete_frame(
    stream: &[u8],
    pos: usize,
    kind: NodeKind,
    len: usize,
    xmt: u32,
    shift: usize,
) -> Option<FixedRecordFrame> {
    let xmt = NonNullXmt::try_from(xmt).ok()?;
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
}

/// Return whether `end` is the stream boundary or a complete fixed-record start.
pub(crate) fn fixed_record_boundary(stream: &[u8], end: usize) -> bool {
    if end == stream.len() {
        return true;
    }
    if stream.get(end) != Some(&0) {
        return false;
    }
    let Some(&kind) = end.checked_add(1).and_then(|at| stream.get(at)) else {
        return false;
    };
    let Ok(kind) = NodeKind::try_from(kind) else {
        return false;
    };
    fixed_record_candidates(stream, end, kind)
        .iter()
        .flatten()
        .next()
        .is_some()
}

pub(crate) fn read_and_advance(stream: &[u8], at: &mut usize) -> Option<u32> {
    let (value, extra) = read_xmt(stream, *at)?;
    *at += 2 + extra;
    Some(value)
}

pub(crate) fn read_sequence_at<const N: usize>(stream: &[u8], at: &mut usize) -> Option<[u32; N]> {
    let mut references = [0; N];
    for reference in &mut references {
        *reference = read_and_advance(stream, at)?;
    }
    Some(references)
}

/// Advance across encoded XMT references without retaining them.
///
/// Fixed-record probing uses a reference sequence only to establish the shifted
/// field boundary. Keeping that validation allocation-free prevents rejected
/// byte candidates from creating temporary vectors during a whole-stream scan.
pub(crate) fn skip_sequence_at(stream: &[u8], at: &mut usize, count: usize) -> Option<()> {
    for _ in 0..count {
        read_and_advance(stream, at)?;
    }
    Some(())
}

/// Decode the compact and extended XMT forms. The extended form uses a negative
/// signed remainder followed by a quotient: `quotient * 32767 + remainder`.
pub(crate) fn read_xmt(stream: &[u8], at: usize) -> Option<(u32, usize)> {
    let mut view = View::over_retained(stream);
    view.seek(at)?;
    let first = view.i16_be()?;
    if first >= 0 {
        return Some((u32::try_from(first).ok()?, 0));
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

/// Record an XMT-keyed entry, dropping every entry of an XMT that repeats.
pub(crate) fn insert_unique<T>(
    records: &mut BTreeMap<u32, T>,
    duplicates: &mut BTreeSet<u32>,
    xmt: u32,
    record: T,
) {
    if duplicates.contains(&xmt) {
        return;
    }
    if records.insert(xmt, record).is_some() {
        records.remove(&xmt);
        duplicates.insert(xmt);
    }
}

fn payload_shift(stream: &[u8], pos: usize, kind: NodeKind, header_shift: usize) -> Option<usize> {
    if kind == NodeKind::Face {
        let mut at = pos + face::ATTRIBUTES + header_shift;
        let start = at;
        read_and_advance(stream, &mut at)?;
        at += 8;
        skip_sequence_at(stream, &mut at, 5)?;
        at += 1;
        skip_sequence_at(stream, &mut at, 5)?;
        return Some(at - start - 31);
    }
    if kind == NodeKind::Edge {
        let mut at = pos + edge::ATTRIBUTES + header_shift;
        let start = at;
        read_and_advance(stream, &mut at)?;
        at += 8;
        skip_sequence_at(stream, &mut at, 7)?;
        return Some(at - start - 24);
    }
    let (offset, before, trailing_bytes, after) = match kind {
        NodeKind::Shell => (shell::ATTRIBUTES, 8, 0, 0),
        NodeKind::Loop => (loop_node::ATTRIBUTES, 4, 0, 0),
        NodeKind::Fin => (fin::ATTRIBUTES, 9, 1, 0),
        NodeKind::Vertex => (vertex::ATTRIBUTES, 5, 8, 1),
        NodeKind::Point => (point::ATTRIBUTES, 4, 24, 0),
        _ => (0, 0, 0, 0),
    };
    if before != 0 {
        let mut at = pos + offset + header_shift;
        let start = at;
        skip_sequence_at(stream, &mut at, before)?;
        at += trailing_bytes;
        skip_sequence_at(stream, &mut at, after)?;
        let compact = before * 2 + trailing_bytes + after * 2;
        return Some(at - start - compact);
    }
    let compact_kind = matches!(
        kind,
        NodeKind::Line
            | NodeKind::Circle
            | NodeKind::Ellipse
            | NodeKind::Intersection
            | NodeKind::Plane
            | NodeKind::Cylinder
            | NodeKind::Cone
            | NodeKind::Sphere
            | NodeKind::Torus
            | NodeKind::BlendSurface
            | NodeKind::OffsetSurface
            | NodeKind::BSurface
            | NodeKind::TrimmedCurve
            | NodeKind::BCurve
            | NodeKind::SpCurve
    );
    if !compact_kind {
        return Some(0);
    }
    let mut at = pos + analytic::ATTRIBUTES + header_shift;
    let start = at;
    skip_sequence_at(stream, &mut at, 5)?;
    matches!(stream.get(at), Some(b'+' | b'-')).then_some(())?;
    at += 1;
    let common_extra = at - start - 11;
    let tail_start = at;
    match kind {
        NodeKind::Intersection => {
            skip_sequence_at(stream, &mut at, 6)?;
        }
        NodeKind::BlendSurface => {
            at += 1;
            skip_sequence_at(stream, &mut at, 3)?;
        }
        NodeKind::OffsetSurface => {
            at += 2;
            read_and_advance(stream, &mut at)?;
        }
        NodeKind::BSurface | NodeKind::BCurve => {
            skip_sequence_at(stream, &mut at, 2)?;
        }
        NodeKind::TrimmedCurve => {
            read_and_advance(stream, &mut at)?;
        }
        NodeKind::SpCurve => {
            skip_sequence_at(stream, &mut at, 3)?;
        }
        _ => {}
    }
    let compact_tail_len = match kind {
        NodeKind::Intersection => 12,
        NodeKind::BlendSurface => 7,
        NodeKind::OffsetSurface => 4,
        NodeKind::BSurface | NodeKind::BCurve => 4,
        NodeKind::TrimmedCurve => 2,
        NodeKind::SpCurve => 6,
        _ => 0,
    };
    Some(common_extra + at - tail_start - compact_tail_len)
}

pub(crate) fn fixed_len(kind: NodeKind) -> usize {
    match kind {
        NodeKind::Body => 24,
        NodeKind::Shell => shell::LEN,
        NodeKind::Face => face::LEN,
        NodeKind::Loop => loop_node::LEN,
        NodeKind::Edge => edge::LEN,
        NodeKind::Fin => fin::LEN,
        NodeKind::Vertex => vertex::LEN,
        NodeKind::Region => 16,
        NodeKind::Point => point::LEN,
        NodeKind::Line => line::LEN,
        NodeKind::Circle => circle::LEN,
        NodeKind::Ellipse => ellipse::LEN,
        NodeKind::Intersection => intersection::LEN,
        NodeKind::Plane => plane::LEN,
        NodeKind::Cylinder => cylinder::LEN,
        NodeKind::Cone => cone::LEN,
        NodeKind::Sphere => sphere::LEN,
        NodeKind::Torus => torus::LEN,
        NodeKind::BlendSurface => 66,
        NodeKind::OffsetSurface => offset_surf::LEN,
        NodeKind::BSurface | NodeKind::BCurve => 23,
        NodeKind::TrimmedCurve => trimmed::LEN,
        NodeKind::SpCurve => sp_curve::LEN,
    }
}

#[cfg(test)]
mod tests {
    use super::{read_sequence_at, skip_sequence_at};

    #[test]
    fn skip_sequence_tracks_compact_and_extended_xmt_widths() {
        let bytes = [0xff, 0xfe, 0x00, 0x02, 0x00, 0x03];
        let mut skipped_at = 0;
        assert_eq!(skip_sequence_at(&bytes, &mut skipped_at, 2), Some(()));
        assert_eq!(skipped_at, bytes.len());

        let mut read_at = 0;
        assert_eq!(
            read_sequence_at::<2>(&bytes, &mut read_at),
            Some([65_536, 3])
        );
        assert_eq!(read_at, skipped_at);
    }

    #[test]
    fn skip_sequence_rejects_truncated_xmt() {
        let mut at = 0;
        assert_eq!(skip_sequence_at(&[0xff, 0xfe, 0x00], &mut at, 1), None);
        assert_eq!(at, 0);
    }
    #[test]
    fn fixed_frame_requires_nonnull_identity_and_a_bounded_complete_span() {
        let mut bytes = vec![0xff; 9];
        bytes.extend_from_slice(&[0; 40]);
        bytes[9..13].copy_from_slice(&[0, 29, 0, 2]);
        let frame = super::fixed_record_candidates(&bytes, 9, super::NodeKind::Point)[0].unwrap();
        assert_eq!(u32::from(frame.xmt()), 2);
        assert_eq!(frame.end(), bytes.len());
        assert_eq!((frame.shift(), frame.payload_shift()), (0, 0));
        assert!(super::fixed_record_candidates(&bytes[..48], 9, super::NodeKind::Point).iter().all(Option::is_none));
        assert!(super::fixed_record_candidates(&bytes, usize::MAX, super::NodeKind::Point).iter().all(Option::is_none));
        for identity in [0_u16, 1] {
            bytes[11..13].copy_from_slice(&identity.to_be_bytes());
            assert!(super::fixed_record_candidates(&bytes, 9, super::NodeKind::Point).iter().all(Option::is_none));
        }
    }

}
