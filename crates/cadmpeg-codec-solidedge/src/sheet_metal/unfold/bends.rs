// SPDX-License-Identifier: Apache-2.0
//! The bends proxy of an unfold (`docs/formats/solidedge.md` §10.6): a
//! `CLSID_JMTopologyProxy` of 1410 bytes that selects the two cylindrical layer
//! faces of one bend. Each layer face is a group of four edge records: arc,
//! straight edge, arc, straight edge.
//!
//! Stored lengths are metres; the values returned are metres.

use cadmpeg_core::decode::View;

/// Bytes of a bends proxy of a bend that is not flat.
const PROXY_BYTES: usize = 1410;
/// Proxy offsets of the two groups.
const GROUP_STARTS: [usize; 2] = [237, 695];
/// Edge records of a layer-face group.
const GROUP_EDGES: usize = 4;
/// Bytes of the group marker: `u32` 1, `u32` edge count, `u16` 2.
const MARKER_BYTES: usize = 10;
/// The `u16` that ends the group marker.
const MARKER_WORD: u16 = 2;
/// The kind byte and the size of a straight edge record.
const KIND_LINE: u8 = 3;
const LINE_BYTES: usize = 65;
/// Bytes from the kind byte of a straight edge to its length.
const LINE_LENGTH: usize = 57;
/// The kind byte and the size of an arc record.
const KIND_ARC: u8 = 16;
const ARC_BYTES: usize = 97;
/// Bytes from the kind byte of an arc to its radius vector.
const ARC_RADIUS_VECTOR: usize = 49;
/// Bytes from the kind byte of an arc to its start angle; the end angle follows.
const ARC_START_ANGLE: usize = 81;
/// Two radii or two lengths of one group agree within this distance, in metres.
const EPS_SAME_LENGTH: f64 = 1e-9;
/// Two bend angles agree within this distance, in radians.
const EPS_SAME_ANGLE: f64 = 1e-9;

/// The bend a bends proxy selects, in metres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct BendFaces {
    /// The length of the straight edges of both layer faces.
    pub(super) width: f64,
    /// The smaller radius of the two layer faces.
    pub(super) inside_radius: f64,
    /// The larger radius of the two layer faces.
    pub(super) outside_radius: f64,
    /// The bend angle: the end angle of each arc minus its start angle, in
    /// radians.
    pub(super) angle: f64,
}

/// The arc radius, the straight-edge length and the arc span of a layer-face
/// group that starts at `at`. `None` where the group does not hold the edges of a
/// cylindrical layer face.
fn layer_face(bytes: &[u8], at: usize) -> Option<(f64, f64, f64)> {
    if View::u32_le_at(bytes, at)? != 1
        || usize::try_from(View::u32_le_at(bytes, at.checked_add(4)?)?).ok()? != GROUP_EDGES
        || View::u16_le_at(bytes, at.checked_add(8)?)? != MARKER_WORD
    {
        return None;
    }
    let mut radii = [0.0_f64; 2];
    let mut lengths = [0.0_f64; 2];
    let mut spans = [0.0_f64; 2];
    let mut next = at.checked_add(MARKER_BYTES)?;
    for ((radius, length), span) in radii
        .iter_mut()
        .zip(lengths.iter_mut())
        .zip(spans.iter_mut())
    {
        if *bytes.get(next)? != KIND_ARC {
            return None;
        }
        let vector = next.checked_add(ARC_RADIUS_VECTOR)?;
        let mut sum = 0.0_f64;
        for axis in 0..3_usize {
            let value = View::f64_le_at(bytes, vector.checked_add(axis * 8)?)?;
            sum += value * value;
        }
        *radius = sum.sqrt();
        let start = View::f64_le_at(bytes, next.checked_add(ARC_START_ANGLE)?)?;
        let end = View::f64_le_at(bytes, next.checked_add(ARC_START_ANGLE + 8)?)?;
        *span = end - start;
        next = next.checked_add(ARC_BYTES)?;
        if *bytes.get(next)? != KIND_LINE {
            return None;
        }
        *length = View::f64_le_at(bytes, next.checked_add(LINE_LENGTH)?)?;
        next = next.checked_add(LINE_BYTES)?;
    }
    let same = |a: f64, b: f64| (a - b).abs() <= EPS_SAME_LENGTH;
    if !same(radii[0], radii[1])
        || !same(lengths[0], lengths[1])
        || (spans[0] - spans[1]).abs() > EPS_SAME_ANGLE
    {
        return None;
    }
    Some((radii[0], lengths[0], spans[0]))
}

/// The bend a bends proxy of an unfold selects. `None` where the proxy is not
/// of 1410 bytes, a group does not hold the four edges of a cylindrical layer
/// face, the two groups state different widths or angles or the same radius, or a
/// value is not finite and positive, or the bend angle is a full turn or more.
pub(super) fn bend_faces(bytes: &[u8]) -> Option<BendFaces> {
    if bytes.len() != PROXY_BYTES {
        return None;
    }
    let (first_radius, first_width, first_angle) = layer_face(bytes, GROUP_STARTS[0])?;
    let (second_radius, second_width, second_angle) = layer_face(bytes, GROUP_STARTS[1])?;
    let valid = |value: f64| value.is_finite() && value > 0.0;
    let consistent = valid(first_radius)
        && valid(second_radius)
        && valid(first_width)
        && valid(first_angle)
        && first_angle < std::f64::consts::TAU
        && (first_angle - second_angle).abs() <= EPS_SAME_ANGLE
        && (first_width - second_width).abs() <= EPS_SAME_LENGTH
        && (first_radius - second_radius).abs() > EPS_SAME_LENGTH;
    if !consistent {
        return None;
    }
    Some(BendFaces {
        width: first_width,
        inside_radius: first_radius.min(second_radius),
        outside_radius: first_radius.max(second_radius),
        angle: first_angle,
    })
}
