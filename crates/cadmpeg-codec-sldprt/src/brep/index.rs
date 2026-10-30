// SPDX-License-Identifier: Apache-2.0
use super::{
    blend, intersection, offset, parse_carrier, spline, subset, sweep, Carrier, CurveCarrier,
    SurfaceCarrier,
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
    /// Compact curves whose records share one attribute and geometry but not
    /// one orientation marker (spec §7.2).
    marker_conflicts: HashSet<u16>,
    /// Spline carriers whose pole and weight lanes do not pair, each a loss
    /// naming its attribute id and the pairing's own refusal.
    pub(super) lane_refusals: Vec<LossNote>,
}

impl CarrierIndex {
    pub(super) fn insert(&mut self, carrier: Carrier) {
        match carrier {
            Carrier::Curve(carrier) => {
                if let Some(IndexedCurve::Exact(known)) = self.curves.get(&carrier.attr) {
                    if known.geometry == carrier.geometry
                        && known.runs_against_edges != carrier.runs_against_edges
                    {
                        self.marker_conflicts.insert(carrier.attr);
                    }
                }
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

    /// Whether compact records of `attr` disagree on their marker (spec §7.2).
    pub(super) fn marker_conflict(&self, attr: u16) -> bool {
        self.marker_conflicts.contains(&attr)
    }

    pub(super) fn curve_attrs(&self) -> HashSet<u16> {
        self.curves.keys().copied().collect()
    }

    pub(super) fn surface(&self, attr: u16) -> Option<&SurfaceCarrier> {
        self.surfaces.get(&attr)
    }

    /// Swept/spun surface construction carried by one attribute.
    pub(super) fn sweep(&self, attr: u16) -> Option<&sweep::SweepCarrier> {
        self.sweeps.get(&attr)
    }

    /// Constant-radius rolling-ball construction carried by `attr`.
    pub(super) fn blend(&self, attr: u16) -> Option<&blend::BlendCarrier> {
        self.blends.get(&attr)
    }

    /// Exact offset-surface construction carried by `attr`.
    pub(super) fn offset(&self, attr: u16) -> Option<&offset::OffsetCarrier> {
        self.offsets.get(&attr)
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
        for attr in other.marker_conflicts {
            if !self.curves.contains_key(&attr) {
                self.marker_conflicts.insert(attr);
            }
        }
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
    let mut out = CarrierIndex::default();
    let mut i = 0usize;
    while i + 2 <= body.len() {
        if body[i] == 0x00 {
            if let Some(c) = parse_carrier(body, i) {
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

    #[test]
    fn compact_records_of_one_curve_with_different_markers_conflict() {
        use crate::test_support::parasolid::circle_carrier;

        let circle = |radius: f64, marker: u8| {
            let mut bytes = circle_carrier(70, [0.0; 3], [0.0, 0.0, 1.0], radius);
            bytes[18] = marker;
            bytes
        };
        let scan = |records: &[Vec<u8>]| {
            super::scan_carriers(
                &cadmpeg_test_support::service_decode_context(),
                &records.concat(),
            )
            .expect("carrier scan")
        };
        assert!(scan(&[circle(1.0, 0x2b), circle(1.0, 0x2d)]).marker_conflict(70));
        assert!(!scan(&[circle(1.0, 0x2b), circle(1.0, 0x2b)]).marker_conflict(70));
        assert!(!scan(&[circle(1.0, 0x2b), circle(2.0, 0x2d)]).marker_conflict(70));
    }
}
