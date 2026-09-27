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
    /// Spline carriers whose pole and weight lanes do not pair, each a loss
    /// naming its attribute id and the pairing's own refusal.
    pub(super) lane_refusals: Vec<LossNote>,
}

impl CarrierIndex {
    pub(super) fn insert(
        &mut self,
        ctx: &DecodeContext<'_>,
        carrier: Carrier,
    ) -> Result<(), cadmpeg_core::CodecError> {
        match carrier {
            Carrier::Curve(carrier) => {
                if !self.curves.contains_key(&carrier.attr) {
                    ctx.charge_collection_items(1, "index SLDPRT curve carriers")?;
                    self.curves.try_reserve(1).map_err(|_| ctx.refuse_codec_limit("index SLDPRT curve carriers", u64::MAX - 1, u64::MAX))?;
                }
                self.curves
                    .insert(carrier.attr, IndexedCurve::Exact(carrier));
            }
            Carrier::Surface(carrier) => {
                if !self.surfaces.contains_key(&carrier.attr) {
                    ctx.charge_collection_items(1, "index SLDPRT surface carriers")?;
                    self.surfaces.try_reserve(1).map_err(|_| ctx.refuse_codec_limit("index SLDPRT surface carriers", u64::MAX - 1, u64::MAX))?;
                }
                self.surfaces.insert(carrier.attr, carrier);
            }
        }
        Ok(())
    }

    pub(super) fn curve(&self, attr: u16) -> Option<&IndexedCurve> {
        self.curves.get(&attr)
    }

    pub(super) fn curve_attrs(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<HashSet<u16>, cadmpeg_core::CodecError> {
        ctx.charge_collection_items(self.curves.len() as u64, "collect SLDPRT curve attributes")?;
        let mut attrs = HashSet::new();
        attrs.try_reserve(self.curves.len()).map_err(|_| ctx.refuse_codec_limit("collect SLDPRT curve attributes", u64::MAX - 1, u64::MAX))?;
        attrs.extend(self.curves.keys().copied());
        Ok(attrs)
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

    fn insert_intersection(
        &mut self,
        ctx: &DecodeContext<'_>,
        intersection: intersection::IntersectionCarrier,
    ) -> Result<(), cadmpeg_core::CodecError> {
        if !self.curves.contains_key(&intersection.carrier.attr) {
            ctx.charge_collection_items(1, "index SLDPRT intersection carriers")?;
            self.curves.try_reserve(1).map_err(|_| ctx.refuse_codec_limit("index SLDPRT intersection carriers", u64::MAX - 1, u64::MAX))?;
        }
        self.curves
            .entry(intersection.carrier.attr)
            .or_insert(IndexedCurve::Derived(intersection));
        Ok(())
    }

    #[cfg(test)]
    fn insert_blend_support_pair(&mut self, attr: u16, carrier: blend::SupportPairCarrier) {
        self.blend_support_pairs.insert(attr, carrier);
    }

    pub(super) fn merge_missing(
        &mut self,
        ctx: &DecodeContext<'_>,
        other: Self,
    ) -> Result<(), cadmpeg_core::CodecError> {
        merge_missing_map(ctx, &mut self.curves, other.curves, "merge SLDPRT curve carriers")?;
        merge_missing_map(ctx, &mut self.surfaces, other.surfaces, "merge SLDPRT surface carriers")?;
        merge_missing_map(ctx, &mut self.sweeps, other.sweeps, "merge SLDPRT sweep carriers")?;
        merge_missing_map(ctx, &mut self.blends, other.blends, "merge SLDPRT blend carriers")?;
        merge_missing_map(ctx, &mut self.offsets, other.offsets, "merge SLDPRT offset carriers")?;
        merge_missing_map(ctx, &mut self.blend_support_pairs, other.blend_support_pairs, "merge SLDPRT blend support pairs")?;
        ctx.reserve_precharged_vec(&mut self.lane_refusals, other.lane_refusals.len(), "merge SLDPRT lane refusals")?;
        self.lane_refusals.extend(other.lane_refusals);
        Ok(())
    }
}

fn merge_missing_map<K: Eq + Hash, V>(
    ctx: &DecodeContext<'_>,
    target: &mut HashMap<K, V>,
    source: HashMap<K, V>,
    operation: &'static str,
) -> Result<(), cadmpeg_core::CodecError> {
    for (key, value) in source {
        if !target.contains_key(&key) {
            ctx.charge_collection_items(1, operation)?;
            target.try_reserve(1).map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
            target.insert(key, value);
        }
    }
    Ok(())
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
                out.insert(ctx, c)?;
            }
        }
        i += 1;
    }
    let mut lane_refusals = Vec::new();
    for carrier in spline::scan_curve_carriers(ctx, body, &mut lane_refusals)?.into_values() {
        out.insert(ctx, Carrier::Curve(carrier))?;
    }
    for carrier in spline::scan_surface_carriers(ctx, body, &mut lane_refusals)?.into_values() {
        out.insert(ctx, Carrier::Surface(carrier))?;
    }
    for carrier in subset::scan(ctx, body, &out)? {
        out.insert(ctx, Carrier::Curve(carrier))?;
    }
    out.sweeps = sweep::scan_sweep_carriers(ctx, body)?;
    (out.blends, out.blend_support_pairs) = blend::scan(ctx, body)?;
    out.offsets = offset::scan(ctx, body)?;
    for intersection in
        intersection::scan_intersection_carriers(ctx, body, &mut lane_refusals)?.into_values()
    {
        out.insert_intersection(ctx, intersection)?;
    }
    out.lane_refusals = lane_refusals;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{blend, CarrierIndex};

    fn blend_body() -> Vec<u8> {
        let mut bytes = vec![0x00, 0x38, 0xff];
        bytes.extend_from_slice(&9u16.to_be_bytes());
        bytes.extend_from_slice(&17u32.to_be_bytes());
        for reference in [1u16, 2, 3, 4, 1] {
            bytes.extend_from_slice(&reference.to_be_bytes());
        }
        bytes.push(0x2b);
        bytes.push(0x45);
        for reference in [11u16, 12, 13] {
            bytes.extend_from_slice(&reference.to_be_bytes());
        }
        for value in [-0.0005f64, -0.0005, 1.0, -1.0] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes
    }

    fn carrier_refusal(
        dimension: cadmpeg_core::decode::ResourceDimension,
        operation: &str,
    ) -> cadmpeg_core::decode::ResourceLimit {
        let bytes = blend_body();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        match dimension {
            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                policy.limits.max_collection_items = 0;
            }
            cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                policy.limits.max_work_units = 0;
            }
            _ => panic!("carrier test selects a collection or work limit"),
        }
        for _ in 0..1024 {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                &bytes,
                &arena,
                &policy,
            ).expect("test carrier bytes fit the root limit");
            let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) =
                super::scan_carriers(&ctx, &bytes)
            else {
                panic!("expected a carrier resource refusal");
            };
            assert_eq!(limit.dimension, dimension);
            if limit.operation == operation {
                let just_below = limit.used + limit.additional - 1;
                match dimension {
                    cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                        policy.limits.max_collection_items = just_below;
                    }
                    cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                        policy.limits.max_work_units = just_below;
                    }
                    _ => panic!("carrier test selects a collection or work limit"),
                }
                let arena = cadmpeg_core::decode::DecodeArena::new();
                let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                    &bytes,
                    &arena,
                    &policy,
                ).expect("test carrier bytes fit the root limit");
                let Err(cadmpeg_core::CodecError::ResourceLimit(repeated)) =
                    super::scan_carriers(&ctx, &bytes)
                else {
                    panic!("one unit below the carrier request must refuse");
                };
                assert_eq!(repeated.dimension, dimension);
                assert_eq!(repeated.operation, operation);
                return limit;
            }
            let next = limit.used + limit.additional;
            match dimension {
                cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                    policy.limits.max_collection_items = next;
                }
                cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                    policy.limits.max_work_units = next;
                }
                _ => panic!("carrier test selects a collection or work limit"),
            }
        }
        panic!("carrier charge was not reached");
    }

    #[test]
    fn carrier_scan_refuses_collection_limit() {
        let limit = carrier_refusal(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            "index SLDPRT blend carriers",
        );
        assert_eq!(limit.additional, 1);
    }

    #[test]
    fn carrier_scan_refuses_work_limit() {
        let limit = carrier_refusal(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "scan SLDPRT blend carriers",
        );
        assert!(limit.additional > 0);
    }

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

        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        ).expect("test merge fits service policy");
        base.merge_missing(&ctx, delta).expect("test merge fits service policy");

        let pair = base.blend_support_pair(9).expect("support pair");
        assert_eq!(pair.supports, [11, 12]);
        assert_eq!(pair.intersection, 13);
    }
}
