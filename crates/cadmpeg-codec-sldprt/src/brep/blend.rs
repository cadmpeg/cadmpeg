// SPDX-License-Identifier: Apache-2.0
//! Constant-radius rolling-ball surface carriers.
//!
//! A `00 38` record names two support surfaces and a spine curve. Equal offset
//! magnitudes define a circular rolling-ball envelope; their signs and two unit
//! scalars select the support-normal sides.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::View;
use cadmpeg_parasolid::records::node_type::{BLEND_BOUND, BLEND_SURF};
use cadmpeg_parasolid::records::PrimaryRecords;

use super::LEN_TO_MM;

use crate::layout::blend_bound_00_3b as bound_rec;
use crate::layout::rolling_ball_blend_00_38 as blend_rec;

const EPS_BLEND_PARSE_RAW_E12: f64 = 1.0e-12;
const EPS_BLEND_PARSE_BLEND_E12: f64 = 1.0e-12;

/// One exact constant-radius blend construction.
#[derive(Debug, Clone)]
pub(super) struct BlendCarrier {
    /// Stream-local surface carrier attribute.
    attr: u32,
    /// Tag-byte offset in the stream.
    pub(super) offset: usize,
    /// Ordered support references.
    pub(super) supports: [BlendSupportRef; 2],
    /// Stored center/spine curve attribute.
    pub(super) spine: u32,
    /// Signed rolling-ball radius in millimetres.
    pub(super) signed_radius: f64,
    /// Whether each support uses the opposite natural-normal side.
    pub(super) reversed: [bool; 2],
}

/// Reference used by one rolling-ball support side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BlendSupportRef {
    /// Direct surface-carrier attribute.
    Surface(u32),
    /// Zero-offset support-pair attribute; topology selects one member.
    Pair(u32),
}

/// Two candidate surface carriers associated with one intersection curve.
#[derive(Debug, Clone)]
pub(super) struct SupportPairCarrier {
    /// Ordered candidate surface-carrier attributes.
    pub(super) supports: [u32; 2],
    /// Intersection-curve attribute.
    pub(super) intersection: u32,
}

#[derive(PartialEq, Eq)]
enum SupportSelector {
    PairThenSurface,
    TwoSurfaces,
}

impl SupportSelector {
    fn read(byte: u8) -> Option<Self> {
        match byte {
            0x45 => Some(Self::PairThenSurface),
            0x52 => Some(Self::TwoSurfaces),
            _ => None,
        }
    }
}

struct RawCarrier {
    attr: u32,
    selector: SupportSelector,
    references: [u32; 3],
    values: [f64; 4],
}

fn parse_raw(bytes: &[u8], offset: usize) -> Option<RawCarrier> {
    if bytes.get(offset..offset + 2) != Some(&[0x00, 0x38]) {
        return None;
    }
    let mut body = offset + 2;
    if bytes.get(body) == Some(&0xff) {
        body += 1;
    }
    let attr = super::word_at(bytes, body + blend_rec::ATTR)?;
    if attr == 0 || !matches!(bytes.get(body + blend_rec::MARKER), Some(0x2b | 0x2d)) {
        return None;
    }
    let selector = SupportSelector::read(*bytes.get(body + blend_rec::SELECTOR)?)?;
    let references = [
        super::word_at(bytes, body + blend_rec::SUPPORT0)?,
        super::word_at(bytes, body + blend_rec::SUPPORT1)?,
        super::word_at(bytes, body + blend_rec::SPINE)?,
    ];
    if references.contains(&0) {
        return None;
    }
    let values = [
        View::f64_be_at(bytes, body + blend_rec::OFFSET0)?,
        View::f64_be_at(bytes, body + blend_rec::OFFSET1)?,
        View::f64_be_at(bytes, body + blend_rec::SIDE0)?,
        View::f64_be_at(bytes, body + blend_rec::SIDE1)?,
    ];
    if values.iter().any(|value| !value.is_finite())
        || (values[2].abs() - 1.0).abs() > EPS_BLEND_PARSE_RAW_E12
        || (values[3].abs() - 1.0).abs() > EPS_BLEND_PARSE_RAW_E12
    {
        return None;
    }
    Some(RawCarrier {
        attr,
        selector,
        references,
        values,
    })
}

fn parse_blend(bytes: &[u8], offset: usize) -> Option<BlendCarrier> {
    blend_from_raw(&parse_raw(bytes, offset)?, offset)
}

/// The zero-offset support pair a raw record states, when it states one.
fn pair_from_raw(raw: &RawCarrier) -> Option<SupportPairCarrier> {
    (raw.selector == SupportSelector::TwoSurfaces
        && raw.values[0].abs() <= f64::EPSILON
        && raw.values[1].abs() <= f64::EPSILON)
        .then_some(SupportPairCarrier {
            supports: [raw.references[0], raw.references[1]],
            intersection: raw.references[2],
        })
}

fn blend_from_raw(raw: &RawCarrier, offset: usize) -> Option<BlendCarrier> {
    let [first_radius, second_radius, first_side, second_side] = raw.values;
    if first_radius.abs() <= f64::EPSILON
        || (first_radius.abs() - second_radius.abs()).abs() > EPS_BLEND_PARSE_BLEND_E12
    {
        return None;
    }
    let supports = match raw.selector {
        SupportSelector::PairThenSurface => [
            BlendSupportRef::Pair(raw.references[0]),
            BlendSupportRef::Surface(raw.references[1]),
        ],
        SupportSelector::TwoSurfaces => [
            BlendSupportRef::Surface(raw.references[0]),
            BlendSupportRef::Surface(raw.references[1]),
        ],
    };
    Some(BlendCarrier {
        attr: raw.attr,
        offset,
        supports,
        spine: raw.references[2],
        signed_radius: first_radius * LEN_TO_MM,
        reversed: [
            first_side.is_sign_negative(),
            second_side.is_sign_negative()
                ^ (first_radius.is_sign_negative() != second_radius.is_sign_negative()),
        ],
    })
}

pub(super) struct BlendCarriers {
    pub blends: HashMap<u32, BlendCarrier>,
    pub pairs: HashMap<u32, SupportPairCarrier>,
    pub bounds: HashMap<u32, BlendBoundCarrier>,
}

/// A `00 3b` blend-boundary record: the boundary of one side of a rolling-ball
/// blend. A surface-intersection chart between a support and this record is
/// that side's contact track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct BlendBoundCarrier {
    /// The stored boundary index: `0` bounds the contact on `support1` and
    /// `1` the contact on `support0`.
    pub(super) side: usize,
    /// The blend surface carrier attribute.
    pub(super) blend: u32,
}

fn parse_bound(bytes: &[u8], offset: usize) -> Option<(u32, BlendBoundCarrier)> {
    if bytes.get(offset..offset + 2) != Some(&[0x00, 0x3b]) {
        return None;
    }
    let mut body = offset + 2;
    if bytes.get(body) == Some(&0xff) {
        body += 1;
    }
    let attr = super::word_at(bytes, body + bound_rec::ATTR)?;
    if attr == 0 || !matches!(bytes.get(body + bound_rec::MARKER), Some(0x2b | 0x2d)) {
        return None;
    }
    let side = match super::word_at(bytes, body + bound_rec::SIDE)? {
        0 => 0,
        1 => 1,
        _ => return None,
    };
    let blend = super::word_at(bytes, body + bound_rec::BLEND)?;
    (blend != 0).then_some((attr, BlendBoundCarrier { side, blend }))
}

/// Scan rolling-ball carriers and their zero-offset support-pair records.
pub(super) fn scan(bytes: &[u8]) -> BlendCarriers {
    let mut blends = HashMap::new();
    let mut pairs = HashMap::new();
    let mut bounds = HashMap::new();
    for offset in 0..bytes.len() {
        if let Some((attr, bound)) = parse_bound(bytes, offset) {
            bounds.entry(attr).or_insert(bound);
        }
    }
    let Some(last_offset) = bytes.len().checked_sub(57) else {
        return BlendCarriers {
            blends,
            pairs,
            bounds,
        };
    };
    for offset in 0..last_offset {
        if let Some(carrier) = parse_blend(bytes, offset) {
            blends.entry(carrier.attr).or_insert(carrier);
        }
        if let Some(pair) = parse_raw(bytes, offset).and_then(|raw| {
            let pair = pair_from_raw(&raw)?;
            Some((raw.attr, pair))
        }) {
            pairs.entry(pair.0).or_insert(pair.1);
        }
    }
    BlendCarriers {
        blends,
        pairs,
        bounds,
    }
}

/// The rolling-ball carriers, support pairs, and blend bounds of a node
/// sequence whose body starts at `body_offset`, when the base fields of every
/// `BLEND_SURF` and `BLEND_BOUND` node read and no two nodes share an index.
/// Offsets are body-relative. A node outside the guards of its compact form
/// is omitted.
pub(super) fn from_nodes(
    records: &PrimaryRecords<'_>,
    body_offset: usize,
) -> Option<BlendCarriers> {
    let mut blends = HashMap::new();
    let mut pairs = HashMap::new();
    let mut bounds = HashMap::new();
    let mut indices = HashSet::new();
    for node in records.nodes() {
        let kind = node.kind();
        if kind != BLEND_SURF && kind != BLEND_BOUND {
            continue;
        }
        indices.insert(node.index()).then_some(())?;
        let marked = matches!(node.char("sense")?, 0x2b | 0x2d);
        if kind == BLEND_BOUND {
            let side = node.short("boundary")?;
            let blend = node.pointer("blend")?;
            let side = match side {
                0 => 0,
                1 => 1,
                _ => continue,
            };
            if marked && blend > 1 {
                bounds.insert(node.index(), BlendBoundCarrier { side, blend });
            }
            continue;
        }
        let selector = SupportSelector::read(node.char("blend_type")?);
        let [first, second] = node.pointer_array::<2>("surface")?;
        let spine = node.pointer("spine")?;
        let [first_offset, second_offset] = node.real_array::<2>("range")?;
        let [first_side, second_side] = node.real_array::<2>("thumb_weight")?;
        let offset = node.range().start.checked_sub(body_offset)?;
        let Some(selector) = selector.filter(|_| marked) else {
            continue;
        };
        let values = [first_offset, second_offset, first_side, second_side];
        if values.iter().any(|value| !value.is_finite())
            || (first_side.abs() - 1.0).abs() > EPS_BLEND_PARSE_RAW_E12
            || (second_side.abs() - 1.0).abs() > EPS_BLEND_PARSE_RAW_E12
        {
            continue;
        }
        let raw = RawCarrier {
            attr: node.index(),
            selector,
            references: [first, second, spine],
            values,
        };
        if let Some(pair) = pair_from_raw(&raw) {
            pairs.insert(raw.attr, pair);
        }
        if let Some(carrier) = blend_from_raw(&raw, offset) {
            blends.insert(raw.attr, carrier);
        }
    }
    Some(BlendCarriers {
        blends,
        pairs,
        bounds,
    })
}

#[cfg(test)]
mod tests {
    use super::{BlendCarrier, BlendSupportRef, SupportPairCarrier};
    use std::collections::HashMap;

    fn scan_pairs(bytes: &[u8]) -> (HashMap<u32, BlendCarrier>, HashMap<u32, SupportPairCarrier>) {
        let super::BlendCarriers { blends, pairs, .. } = super::scan(bytes);
        (blends, pairs)
    }

    #[test]
    fn parses_constant_radius_support_and_spine_payload() {
        for selector in [0x45, 0x52] {
            let mut bytes = vec![0x00, 0x38, 0xff];
            bytes.extend_from_slice(&9u16.to_be_bytes());
            bytes.extend_from_slice(&17u32.to_be_bytes());
            for reference in [1u16, 2, 3, 4, 1] {
                bytes.extend_from_slice(&reference.to_be_bytes());
            }
            bytes.push(0x2b);
            bytes.push(selector);
            for reference in [11u16, 12, 13] {
                bytes.extend_from_slice(&reference.to_be_bytes());
            }
            for value in [-0.0005f64, -0.0005, 1.0, -1.0] {
                bytes.extend_from_slice(&value.to_be_bytes());
            }

            let carrier = scan_pairs(&bytes).0.remove(&9).expect("blend carrier");
            assert_eq!(carrier.offset, 0);
            assert_eq!(
                carrier.supports,
                if selector == 0x45 {
                    [BlendSupportRef::Pair(11), BlendSupportRef::Surface(12)]
                } else {
                    [BlendSupportRef::Surface(11), BlendSupportRef::Surface(12)]
                }
            );
            assert_eq!(carrier.spine, 13);
            assert_eq!(carrier.signed_radius, -0.5);
            assert_eq!(carrier.reversed, [false, true]);
        }
    }

    #[test]
    fn unequal_radius_magnitudes_are_not_a_constant_radius_blend() {
        let mut bytes = vec![0x00, 0x38];
        bytes.extend_from_slice(&9u16.to_be_bytes());
        bytes.extend_from_slice(&17u32.to_be_bytes());
        bytes.extend_from_slice(&[0; 10]);
        bytes.push(0x2b);
        bytes.push(0x52);
        for reference in [11u16, 12, 13] {
            bytes.extend_from_slice(&reference.to_be_bytes());
        }
        for value in [-0.0005f64, -0.0006, 1.0, 1.0] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        assert!(scan_pairs(&bytes).0.is_empty());
    }

    #[test]
    fn opposite_offset_sign_reverses_the_second_support() {
        let mut bytes = vec![0x00, 0x38];
        bytes.extend_from_slice(&9u16.to_be_bytes());
        bytes.extend_from_slice(&17u32.to_be_bytes());
        bytes.extend_from_slice(&[0; 10]);
        bytes.push(0x2b);
        bytes.push(0x52);
        for reference in [11u16, 12, 13] {
            bytes.extend_from_slice(&reference.to_be_bytes());
        }
        for value in [0.0005f64, -0.0005, 1.0, 1.0] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        assert_eq!(
            scan_pairs(&bytes).0.get(&9).expect("blend").reversed,
            [false, true]
        );
    }

    #[test]
    fn zero_offsets_define_a_support_pair() {
        let mut bytes = vec![0x00, 0x38];
        bytes.extend_from_slice(&9u16.to_be_bytes());
        bytes.extend_from_slice(&17u32.to_be_bytes());
        bytes.extend_from_slice(&[0; 10]);
        bytes.push(0x2b);
        bytes.push(0x52);
        for reference in [11u16, 12, 13] {
            bytes.extend_from_slice(&reference.to_be_bytes());
        }
        for value in [0.0f64, 0.0, 1.0, 1.0] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }

        let (_, pairs) = scan_pairs(&bytes);
        let pair = pairs.get(&9).expect("support pair");
        assert_eq!(pair.supports, [11, 12]);
        assert_eq!(pair.intersection, 13);
    }

    #[test]
    fn a_blend_boundary_record_names_a_side_of_its_blend() {
        let record = |ff: bool, marker: u8, side: u16, blend: u16| {
            let mut bytes = vec![0x00, 0x3b];
            if ff {
                bytes.push(0xff);
            }
            bytes.extend_from_slice(&21u16.to_be_bytes());
            bytes.extend_from_slice(&[0; 14]);
            bytes.push(marker);
            bytes.extend_from_slice(&side.to_be_bytes());
            bytes.extend_from_slice(&blend.to_be_bytes());
            bytes
        };
        let bound = |bytes: Vec<u8>| super::scan(&bytes).bounds.remove(&21);
        let expected = super::BlendBoundCarrier { side: 1, blend: 9 };
        assert_eq!(bound(record(false, 0x2b, 1, 9)), Some(expected));
        assert_eq!(bound(record(true, 0x2d, 1, 9)), Some(expected));
        assert_eq!(
            bound(record(false, 0x2b, 0, 9)),
            Some(super::BlendBoundCarrier { side: 0, blend: 9 })
        );
        assert_eq!(bound(record(false, 0x2b, 2, 9)), None);
        assert_eq!(bound(record(false, 0x2b, 1, 0)), None);
        assert_eq!(bound(record(false, 0x00, 1, 9)), None);
    }

    fn blend_surface(
        blend_type: u8,
        supports: [u16; 2],
        range: [f64; 2],
        sides: [f64; 2],
    ) -> Vec<crate::test_support::parasolid::sequence::F> {
        use crate::test_support::parasolid::sequence::F::{C, I, P, R};
        vec![
            I(1),
            P(1),
            P(1),
            P(1),
            P(1),
            P(1),
            C(b'+'),
            C(blend_type),
            P(supports[0]),
            P(supports[1]),
            P(30),
            R(range[0]),
            R(range[1]),
            R(sides[0]),
            R(sides[1]),
            P(1),
            P(1),
            P(1),
            P(1),
        ]
    }

    fn blend_bound(side: u16, blend: u16) -> Vec<crate::test_support::parasolid::sequence::F> {
        use crate::test_support::parasolid::sequence::F::{C, I, P, S};
        vec![
            I(1),
            P(1),
            P(1),
            P(1),
            P(1),
            P(1),
            C(b'-'),
            S(side),
            P(blend),
        ]
    }

    #[test]
    fn blend_nodes_give_carriers_pairs_and_bounds() {
        use crate::brep::topology::nodes::sequence;
        use crate::test_support::parasolid::sequence::sequence_of;
        let (payload, body_offset) = sequence_of(&[
            (
                56,
                3,
                blend_surface(b'R', [20, 21], [0.002, -0.002], [1.0, -1.0]),
            ),
            (56, 4, blend_surface(b'R', [22, 23], [0.0, 0.0], [1.0, 1.0])),
            (
                56,
                5,
                blend_surface(b'R', [24, 25], [0.002, 0.002], [0.5, 1.0]),
            ),
            (59, 6, blend_bound(1, 3)),
            (59, 7, blend_bound(2, 3)),
        ]);
        let records = sequence(&payload).unwrap();
        let carriers = super::from_nodes(&records, body_offset).unwrap();
        let blend = &carriers.blends[&3];
        assert_eq!(
            blend.supports,
            [BlendSupportRef::Surface(20), BlendSupportRef::Surface(21)]
        );
        assert_eq!(blend.spine, 30);
        assert!((blend.signed_radius - 2.0).abs() < 1.0e-12);
        assert_eq!(blend.reversed, [false, false]);
        assert_eq!(carriers.pairs[&4].supports, [22, 23]);
        assert!(!carriers.blends.contains_key(&5));
        assert_eq!(carriers.bounds.len(), 1);
        assert_eq!(carriers.bounds[&6].blend, 3);
        let first = records.nodes().next().unwrap();
        assert_eq!(blend.offset, first.range().start - body_offset);
    }

    #[test]
    fn repeated_blend_indices_leave_the_stream_to_the_scanner() {
        use crate::brep::topology::nodes::sequence;
        use crate::test_support::parasolid::sequence::sequence_of;
        let surface = || blend_surface(b'R', [20, 21], [0.002, -0.002], [1.0, -1.0]);
        let (payload, body_offset) = sequence_of(&[(56, 3, surface()), (56, 3, surface())]);
        let records = sequence(&payload).unwrap();
        assert!(super::from_nodes(&records, body_offset).is_none());
    }
}
