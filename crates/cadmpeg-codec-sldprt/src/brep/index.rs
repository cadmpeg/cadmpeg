// SPDX-License-Identifier: Apache-2.0
use super::{
    blend, intersection, offset, parse_carrier, spline, subset, sweep, Carrier, CurveCarrier,
    SurfaceCarrier,
};
use std::collections::{HashMap, HashSet};

/// An exact carrier or a derived intersection carrier.
pub(crate) enum IndexedCurve {
    Exact(CurveCarrier),
    Derived(intersection::IntersectionCarrier),
}

impl IndexedCurve {
    /// Returns the curve carrier for either provenance variant.
    pub(crate) fn carrier(&self) -> &CurveCarrier {
        match self {
            Self::Exact(carrier) => carrier,
            Self::Derived(intersection) => &intersection.carrier,
        }
    }
}

#[derive(Default)]
pub(crate) struct CarrierIndex {
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

    pub(crate) fn curve(&self, attr: u16) -> Option<&IndexedCurve> {
        self.curves.get(&attr)
    }

    pub(crate) fn curve_attrs(&self) -> HashSet<u16> {
        self.curves.keys().copied().collect()
    }

    pub(crate) fn surface(&self, attr: u16) -> Option<&SurfaceCarrier> {
        self.surfaces.get(&attr)
    }

    /// Swept/spun surface construction carried by one attribute.
    pub(crate) fn sweep(&self, attr: u16) -> Option<&sweep::SweepCarrier> {
        self.sweeps.get(&attr)
    }

    /// Constant-radius rolling-ball construction carried by `attr`.
    pub(crate) fn blend(&self, attr: u16) -> Option<&blend::BlendCarrier> {
        self.blends.get(&attr)
    }

    /// Exact offset-surface construction carried by `attr`.
    pub(crate) fn offset(&self, attr: u16) -> Option<&offset::OffsetCarrier> {
        self.offsets.get(&attr)
    }

    /// Zero-offset surface pair carried by `attr`.
    pub(crate) fn blend_support_pair(&self, attr: u16) -> Option<&blend::SupportPairCarrier> {
        self.blend_support_pairs.get(&attr)
    }

    fn insert_intersection(&mut self, intersection: intersection::IntersectionCarrier) {
        self.curves
            .entry(intersection.carrier.attr)
            .or_insert(IndexedCurve::Derived(intersection));
    }

    #[cfg(test)]
    pub(super) fn insert_blend_support_pair(
        &mut self,
        attr: u16,
        carrier: blend::SupportPairCarrier,
    ) {
        self.blend_support_pairs.insert(attr, carrier);
    }

    pub(crate) fn merge_missing(&mut self, other: Self) {
        for (attr, carrier) in other.curves {
            self.curves.entry(attr).or_insert(carrier);
        }
        for (attr, carrier) in other.surfaces {
            self.surfaces.entry(attr).or_insert(carrier);
        }
        for (attr, carrier) in other.sweeps {
            self.sweeps.entry(attr).or_insert(carrier);
        }
        for (attr, carrier) in other.blends {
            self.blends.entry(attr).or_insert(carrier);
        }
        for (attr, carrier) in other.offsets {
            self.offsets.entry(attr).or_insert(carrier);
        }
        for (attr, carrier) in other.blend_support_pairs {
            self.blend_support_pairs.entry(attr).or_insert(carrier);
        }
    }
}

/// Scan the whole stream body for compact analytic carriers, keyed by attribute
/// id. A later occurrence in one stream replaces an earlier occurrence at the
/// same identity. Cross-stream precedence is applied by [`CarrierIndex::merge_missing`]:
/// the partition carrier remains authoritative and a deltas carrier fills only
/// an absent identity.
pub(crate) fn scan_carriers(body: &[u8]) -> CarrierIndex {
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
    for carrier in spline::scan_curve_carriers(body).into_values() {
        out.insert(Carrier::Curve(carrier));
    }
    for carrier in spline::scan_surface_carriers(body).into_values() {
        out.insert(Carrier::Surface(carrier));
    }
    for carrier in subset::scan(body, &out) {
        out.insert(Carrier::Curve(carrier));
    }
    out.sweeps = sweep::scan_sweep_carriers(body);
    (out.blends, out.blend_support_pairs) = blend::scan(body);
    out.offsets = offset::scan(body);
    for intersection in intersection::scan_intersection_carriers(body).into_values() {
        out.insert_intersection(intersection);
    }
    out
}
