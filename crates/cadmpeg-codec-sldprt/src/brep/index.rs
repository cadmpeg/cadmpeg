// SPDX-License-Identifier: Apache-2.0
use super::{
    blend, intersection, offset, parse_carrier, parse_carrier_for_role, spline, subset, sweep,
    Carrier, CurveCarrier, SurfaceCarrier,
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::report::loss::LossNote;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;

/// An exact carrier or a derived intersection carrier.
pub(super) enum IndexedCurve {
    Exact(CurveCarrier),
    Derived(intersection::IntersectionCarrier),
}

impl IndexedCurve {
    /// Returns the curve carrier for either provenance variant.
    pub(super) fn carrier(&self) -> &CurveCarrier {
        match self {
            Self::Exact(carrier) => carrier,
            Self::Derived(intersection) => &intersection.carrier,
        }
    }
}

#[derive(Default)]
pub(super) struct CarrierIndex {
    curves: HashMap<u16, IndexedCurve>,
    surfaces: HashMap<u16, SurfaceCarrier>,
    /// Swept/spun surface constructions, resolved to a patch at face binding.
    sweeps: HashMap<u16, sweep::SweepCarrier>,
    /// Constant-radius rolling-ball constructions, resolved at face binding.
    blends: HashMap<u16, blend::BlendCarrier>,
    /// Exact offset-surface constructions, resolved recursively at face binding.
    offsets: HashMap<u16, offset::OffsetCarrier>,
    /// Zero-offset surface pairs referenced by rolling-ball constructions.
    blend_support_pairs: HashMap<u16, blend::SupportPairCarrier>,
    /// Spline carriers whose pole and weight lanes do not pair, each a loss
    /// naming its attribute id and the pairing's own refusal.
    pub(super) lane_refusals: Vec<LossNote>,
}

impl CarrierIndex {
    pub(super) fn insert(&mut self, carrier: Carrier) {
        match carrier {
            Carrier::Curve(carrier) => {
                self.curves
                    .insert(carrier.attr, IndexedCurve::Exact(carrier));
            }
            Carrier::Surface(carrier) => {
                self.surfaces.insert(carrier.attr, carrier);
            }
        }
    }

    pub(super) fn curve(&self, attr: u16) -> Option<&IndexedCurve> {
        self.curves.get(&attr)
    }

    pub(super) fn curve_attrs(&self) -> HashSet<u16> {
        self.curves.keys().copied().collect()
    }

    fn surface_category_ambiguous(&self, attr: u16) -> bool {
        usize::from(self.surfaces.contains_key(&attr))
            + usize::from(self.sweeps.contains_key(&attr))
            + usize::from(self.blends.contains_key(&attr))
            + usize::from(self.offsets.contains_key(&attr))
            > 1
    }

    pub(super) fn has_surface_category_conflict(&self) -> bool {
        self.surfaces
            .keys()
            .chain(self.sweeps.keys())
            .chain(self.blends.keys())
            .chain(self.offsets.keys())
            .any(|attr| self.surface_category_ambiguous(*attr))
    }

    pub(super) fn surface(&self, attr: u16) -> Option<&SurfaceCarrier> {
        if self.surface_category_ambiguous(attr) {
            None
        } else {
            self.surfaces.get(&attr)
        }
    }

    /// Swept/spun surface construction carried by one attribute.
    pub(super) fn sweep(&self, attr: u16) -> Option<&sweep::SweepCarrier> {
        if self.surface_category_ambiguous(attr) {
            None
        } else {
            self.sweeps.get(&attr)
        }
    }

    /// Constant-radius rolling-ball construction carried by `attr`.
    pub(super) fn blend(&self, attr: u16) -> Option<&blend::BlendCarrier> {
        if self.surface_category_ambiguous(attr) {
            None
        } else {
            self.blends.get(&attr)
        }
    }

    /// Exact offset-surface construction carried by `attr`.
    pub(super) fn offset(&self, attr: u16) -> Option<&offset::OffsetCarrier> {
        if self.surface_category_ambiguous(attr) {
            None
        } else {
            self.offsets
                .get(&attr)
                .filter(|carrier| carrier.transferable)
        }
    }

    pub(super) fn has_unvalidated_offset(&self) -> bool {
        self.offsets.values().any(|carrier| !carrier.transferable)
    }

    /// Zero-offset surface pair carried by `attr`.
    pub(super) fn blend_support_pair(&self, attr: u16) -> Option<&blend::SupportPairCarrier> {
        self.blend_support_pairs.get(&attr)
    }

    fn insert_intersection(&mut self, intersection: intersection::IntersectionCarrier) {
        self.curves
            .entry(intersection.carrier.attr)
            .or_insert(IndexedCurve::Derived(intersection));
    }

    #[cfg(test)]
    fn insert_blend_support_pair(&mut self, attr: u16, carrier: blend::SupportPairCarrier) {
        self.blend_support_pairs.insert(attr, carrier);
    }

    pub(super) fn merge_missing(&mut self, other: Self) {
        merge_missing_map(&mut self.curves, other.curves);
        merge_missing_map(&mut self.surfaces, other.surfaces);
        merge_missing_map(&mut self.sweeps, other.sweeps);
        merge_missing_map(&mut self.blends, other.blends);
        merge_missing_map(&mut self.offsets, other.offsets);
        merge_missing_map(&mut self.blend_support_pairs, other.blend_support_pairs);
        self.lane_refusals.extend(other.lane_refusals);
    }
}

fn merge_missing_map<K: Eq + Hash, V>(target: &mut HashMap<K, V>, source: HashMap<K, V>) {
    for (key, value) in source {
        target.entry(key).or_insert(value);
    }
}

/// Scan the whole stream body for compact analytic carriers, keyed by attribute
/// id. A later occurrence in one stream replaces an earlier occurrence at the
/// same identity. Cross-stream precedence is applied by [`CarrierIndex::merge_missing`]:
/// the partition carrier remains authoritative and a deltas carrier fills only
/// an absent identity.
pub(super) fn scan_carriers(
    ctx: &DecodeContext<'_>,
    body: &[u8],
) -> Result<CarrierIndex, cadmpeg_core::CodecError> {
    scan_carriers_with_framing(ctx, body, None)
}

pub(super) fn scan_carriers_for_role(
    ctx: &DecodeContext<'_>,
    body: &[u8],
    deltas: bool,
) -> Result<CarrierIndex, cadmpeg_core::CodecError> {
    scan_carriers_with_framing(ctx, body, Some(deltas))
}

fn scan_carriers_with_framing(
    ctx: &DecodeContext<'_>,
    body: &[u8],
    deltas: Option<bool>,
) -> Result<CarrierIndex, cadmpeg_core::CodecError> {
    let mut out = CarrierIndex::default();
    let mut i = 0usize;
    while i + 2 <= body.len() {
        if body[i] == 0x00 {
            let carrier = match deltas {
                Some(deltas) => parse_carrier_for_role(body, i, deltas),
                None => parse_carrier(body, i),
            };
            if let Some(c) = carrier {
                out.insert(c);
            }
        }
        i += 1;
    }
    let mut lane_refusals = Vec::new();
    for carrier in spline::scan_curve_carriers(body, &mut lane_refusals).into_values() {
        out.insert(Carrier::Curve(carrier));
    }
    for carrier in spline::scan_surface_carriers(body, &mut lane_refusals).into_values() {
        out.insert(Carrier::Surface(carrier));
    }
    for carrier in subset::scan(ctx, body, &out)? {
        out.insert(Carrier::Curve(carrier));
    }
    out.sweeps = sweep::scan_sweep_carriers(body);
    let blend::BlendCarriers { blends, pairs } = blend::scan(body);
    out.blends = blends;
    out.blend_support_pairs = pairs;
    out.offsets = offset::scan(body);
    for intersection in
        intersection::scan_intersection_carriers(body, &mut lane_refusals).into_values()
    {
        out.insert_intersection(intersection);
    }
    out.lane_refusals = lane_refusals;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{blend, CarrierIndex};

    #[test]
    fn merge_retains_zero_offset_blend_support_pairs() {
        let mut base = CarrierIndex::default();
        let mut delta = CarrierIndex::default();
        delta.insert_blend_support_pair(
            9,
            blend::SupportPairCarrier {
                supports: [11, 12],
                intersection: 13,
            },
        );

        base.merge_missing(delta);

        let pair = base.blend_support_pair(9).expect("support pair");
        assert_eq!(pair.supports, [11, 12]);
        assert_eq!(pair.intersection, 13);
    }
    fn authored_surface(kind: u8, attr: u16) -> Vec<u8> {
        let mut bytes = vec![0, kind];
        bytes.extend_from_slice(&attr.to_be_bytes());
        bytes.extend_from_slice(&901u32.to_be_bytes());
        for reference in [1u16, 11, 13, 17, 19] {
            bytes.extend_from_slice(&reference.to_be_bytes());
        }
        bytes.push(b'+');
        let values: &[f64] = match kind {
            0x32 => &[0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0],
            0x3c => {
                bytes.extend_from_slice(&[b'V', 0]);
                bytes.extend_from_slice(&23u16.to_be_bytes());
                &[0.002]
            }
            0x43 => {
                bytes.extend_from_slice(&23u16.to_be_bytes());
                &[0.0, 0.0, 1.0]
            }
            0x38 => {
                bytes.push(b'R');
                for reference in [23u16, 29, 31] {
                    bytes.extend_from_slice(&reference.to_be_bytes());
                }
                &[0.005, 0.005, 1.0, 1.0]
            }
            _ => panic!("authored category"),
        };
        for value in values {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        if kind == 0x3c {
            bytes.extend_from_slice(&(-31_415_800_000_000.0f64).to_be_bytes());
        }
        bytes.extend_from_slice(&[0; 3]);
        bytes
    }

    fn available_categories(index: &CarrierIndex, attr: u16) -> [bool; 4] {
        [
            index.surface(attr).is_some(),
            index.offset(attr).is_some(),
            index.sweep(attr).is_some(),
            index.blend(attr).is_some(),
        ]
    }

    #[test]
    fn unique_surface_categories_keep_their_typed_carriers() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        for (position, kind) in [0x32, 0x3c, 0x43, 0x38].into_iter().enumerate() {
            let index = super::scan_carriers(&ctx, &authored_surface(kind, 73)).unwrap();
            let mut expected = [false; 4];
            expected[position] = true;
            assert_eq!(available_categories(&index, 73), expected);
            assert_eq!(available_categories(&index, 79), [false; 4]);
        }
    }

    #[test]
    fn conflicting_surface_categories_have_no_probe_order_winner() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let kinds = [0x32, 0x3c, 0x43, 0x38];
        for first in 0..kinds.len() {
            for second in first + 1..kinds.len() {
                for order in [[first, second], [second, first]] {
                    let mut bytes = authored_surface(kinds[order[0]], 73);
                    bytes.extend(authored_surface(kinds[order[1]], 73));
                    let index = super::scan_carriers(&ctx, &bytes).unwrap();
                    assert_eq!(available_categories(&index, 73), [false; 4]);
                }
            }
        }
    }

    #[test]
    fn merging_surface_categories_cannot_restore_a_conflicting_family() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        for kinds in [[0x32, 0x3c], [0x3c, 0x32]] {
            let mut first = super::scan_carriers(&ctx, &authored_surface(kinds[0], 73)).unwrap();
            let second = super::scan_carriers(&ctx, &authored_surface(kinds[1], 73)).unwrap();
            first.merge_missing(second);
            assert_eq!(available_categories(&first, 73), [false; 4]);
        }
    }
    #[test]
    fn ineligible_offset_claim_prevents_another_category_from_replacing_it() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        for status in *b"IU" {
            let mut raw = authored_surface(0x3c, 73);
            raw[2 + crate::layout::offset_surface_00_3c::DISCRIMINATOR] = status;
            let mut index = super::scan_carriers(&ctx, &raw).unwrap();
            assert!(index.has_unvalidated_offset());
            assert!(index.offset(73).is_none());
            index.merge_missing(super::scan_carriers(&ctx, &authored_surface(0x32, 73)).unwrap());
            assert_eq!(available_categories(&index, 73), [false; 4]);
        }
    }
}
