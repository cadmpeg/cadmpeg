// SPDX-License-Identifier: Apache-2.0
use super::{
    blend, intersection, offset, parse_carrier, spline, subset, sweep, Carrier, CurveCarrier,
    SurfaceCarrier,
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::report::loss::LossNote;
use std::collections::{BTreeMap, BTreeSet};

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
    curves: BTreeMap<u16, IndexedCurve>,
    surfaces: BTreeMap<u16, SurfaceCarrier>,
    /// Swept/spun surface constructions, resolved to a patch at face binding.
    sweeps: BTreeMap<u16, sweep::SweepCarrier>,
    /// Constant-radius rolling-ball constructions, resolved at face binding.
    blends: BTreeMap<u16, blend::BlendCarrier>,
    /// Exact offset-surface constructions, resolved recursively at face binding.
    offsets: BTreeMap<u16, offset::OffsetCarrier>,
    /// Zero-offset surface pairs referenced by rolling-ball constructions.
    blend_support_pairs: BTreeMap<u16, blend::SupportPairCarrier>,
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
                ctx.insert_btree_map(
                    &mut self.curves,
                    carrier.attr,
                    IndexedCurve::Exact(carrier),
                    "index SLDPRT curve carriers",
                )?;
            }
            Carrier::Surface(carrier) => {
                ctx.insert_btree_map(
                    &mut self.surfaces,
                    carrier.attr,
                    carrier,
                    "index SLDPRT surface carriers",
                )?;
            }
        }
        Ok(())
    }

    pub(super) fn curve(
        &self,
        ctx: &DecodeContext<'_>,
        attr: u16,
    ) -> Result<Option<&IndexedCurve>, cadmpeg_core::CodecError> {
        ctx.get_btree_map(&self.curves, &attr, "lookup SLDPRT curve carrier")
    }

    pub(super) fn curve_attrs(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<BTreeSet<u16>, cadmpeg_core::CodecError> {
        ctx.collect_btree_set(
            self.curves.iter().map(|(attr, _)| *attr),
            "collect SLDPRT curve attributes",
        )
    }

    pub(super) fn surface(
        &self,
        ctx: &DecodeContext<'_>,
        attr: u16,
    ) -> Result<Option<&SurfaceCarrier>, cadmpeg_core::CodecError> {
        ctx.get_btree_map(&self.surfaces, &attr, "lookup SLDPRT surface carrier")
    }

    /// Swept/spun surface construction carried by one attribute.
    pub(super) fn sweep(
        &self,
        ctx: &DecodeContext<'_>,
        attr: u16,
    ) -> Result<Option<&sweep::SweepCarrier>, cadmpeg_core::CodecError> {
        ctx.get_btree_map(&self.sweeps, &attr, "lookup SLDPRT sweep carrier")
    }

    /// Constant-radius rolling-ball construction carried by `attr`.
    pub(super) fn blend(
        &self,
        ctx: &DecodeContext<'_>,
        attr: u16,
    ) -> Result<Option<&blend::BlendCarrier>, cadmpeg_core::CodecError> {
        ctx.get_btree_map(&self.blends, &attr, "lookup SLDPRT blend carrier")
    }

    /// Exact offset-surface construction carried by `attr`.
    pub(super) fn offset(
        &self,
        ctx: &DecodeContext<'_>,
        attr: u16,
    ) -> Result<Option<&offset::OffsetCarrier>, cadmpeg_core::CodecError> {
        ctx.get_btree_map(&self.offsets, &attr, "lookup SLDPRT offset carrier")
    }

    /// Zero-offset surface pair carried by `attr`.
    pub(super) fn blend_support_pair(
        &self,
        ctx: &DecodeContext<'_>,
        attr: u16,
    ) -> Result<Option<&blend::SupportPairCarrier>, cadmpeg_core::CodecError> {
        ctx.get_btree_map(
            &self.blend_support_pairs,
            &attr,
            "lookup SLDPRT blend_support_pair carrier",
        )
    }

    fn insert_intersection(
        &mut self,
        ctx: &DecodeContext<'_>,
        intersection: intersection::IntersectionCarrier,
    ) -> Result<(), cadmpeg_core::CodecError> {
        ctx.entry_btree_map(
            &mut self.curves,
            intersection.carrier.attr,
            "index SLDPRT intersection carriers",
        )?
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
        merge_missing_map(
            ctx,
            &mut self.curves,
            other.curves,
            "merge SLDPRT curve carriers",
        )?;
        merge_missing_map(
            ctx,
            &mut self.surfaces,
            other.surfaces,
            "merge SLDPRT surface carriers",
        )?;
        merge_missing_map(
            ctx,
            &mut self.sweeps,
            other.sweeps,
            "merge SLDPRT sweep carriers",
        )?;
        merge_missing_map(
            ctx,
            &mut self.blends,
            other.blends,
            "merge SLDPRT blend carriers",
        )?;
        merge_missing_map(
            ctx,
            &mut self.offsets,
            other.offsets,
            "merge SLDPRT offset carriers",
        )?;
        merge_missing_map(
            ctx,
            &mut self.blend_support_pairs,
            other.blend_support_pairs,
            "merge SLDPRT blend support pairs",
        )?;
        let mut lane_refusals = other.lane_refusals;
        ctx.append_vec(
            &mut self.lane_refusals,
            &mut lane_refusals,
            "merge SLDPRT lane refusals",
        )?;
        Ok(())
    }
}

fn merge_missing_map<K: Ord + cadmpeg_core::decode::cost::DecodeCost, V>(
    ctx: &DecodeContext<'_>,
    target: &mut BTreeMap<K, V>,
    source: BTreeMap<K, V>,
    operation: &'static str,
) -> Result<(), cadmpeg_core::CodecError> {
    for (key, value) in ctx.admit_iter(source, operation)? {
        if let std::collections::btree_map::Entry::Vacant(entry) =
            ctx.entry_btree_map(target, key, operation)?
        {
            entry.insert(value);
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
    // Each start parses at most two fixed-width framings, so one unit per
    // start pays for it.
    let starts = 0..body.len().checked_sub(1).map_or(0, |end| end);
    for i in ctx.admit_iter(starts, "scan SLDPRT analytic carriers")? {
        if let Some(carrier) = parse_carrier(body, i) {
            out.insert(ctx, carrier)?;
        }
    }
    let mut lane_refusals = Vec::new();
    for (_, carrier) in ctx.admit_iter(
        spline::scan_curve_carriers(ctx, body, &mut lane_refusals)?,
        "index SLDPRT spline curve carriers",
    )? {
        out.insert(ctx, Carrier::Curve(carrier))?;
    }
    for (_, carrier) in ctx.admit_iter(
        spline::scan_surface_carriers(ctx, body, &mut lane_refusals)?,
        "index SLDPRT spline surface carriers",
    )? {
        out.insert(ctx, Carrier::Surface(carrier))?;
    }
    for carrier in ctx.admit_iter(
        subset::scan(ctx, body, &out)?,
        "index SLDPRT subset carriers",
    )? {
        out.insert(ctx, Carrier::Curve(carrier))?;
    }
    out.sweeps = sweep::scan_sweep_carriers(ctx, body)?;
    let blend::BlendCarriers { blends, pairs } = blend::scan(ctx, body)?;
    out.blends = blends;
    out.blend_support_pairs = pairs;
    out.offsets = offset::scan(ctx, body)?;
    for (_, intersection) in ctx.admit_iter(
        intersection::scan_intersection_carriers(ctx, body, &mut lane_refusals)?,
        "index SLDPRT intersection carriers",
    )? {
        out.insert_intersection(ctx, intersection)?;
    }
    out.lane_refusals = lane_refusals;
    Ok(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn carrier_lookup_admits_key_work() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let index = super::scan_carriers(&ctx, &blend_body()).expect("carrier scan");
        crate::test_support::work_refusal_at("lookup SLDPRT blend carrier", |ctx| {
            index.blend(ctx, 9)
        });
        assert!(index.blend(&ctx, 9).unwrap().is_some());
    }

    #[test]
    fn empty_carrier_indexes_do_not_charge_key_comparisons() {
        let index = CarrierIndex::default();
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(index.curve(&ctx, 7).unwrap().is_none());
        assert!(index.surface(&ctx, 7).unwrap().is_none());
        assert!(index.sweep(&ctx, 7).unwrap().is_none());
        assert!(index.blend(&ctx, 7).unwrap().is_none());
        assert!(index.offset(&ctx, 7).unwrap().is_none());
        assert!(index.blend_support_pair(&ctx, 7).unwrap().is_none());
        ctx.finish_session().unwrap();
    }

    #[test]
    fn analytic_carrier_scan_admits_work_before_empty_pass() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let body = [0xff; 4096];
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&body, &arena, &policy).unwrap();
        let (result, allocations) = crate::test_support::allocation::count_allocations(|| {
            super::scan_carriers(&ctx, &body)
        });
        let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = result else {
            panic!("work refusal");
        };
        assert_eq!(limit.operation, "scan SLDPRT analytic carriers");
        assert_eq!(
            limit.dimension,
            cadmpeg_core::decode::ResourceDimension::WorkUnits
        );
        assert_eq!(ctx.resource_refusal(), Some(limit));
        assert_eq!(allocations, 0);
    }

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
        let error = cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            match dimension {
                cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                    policy.limits.max_collection_items = cap
                }
                cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                    policy.limits.max_work_units = cap
                }
                _ => panic!("carrier test selects a collection or work limit"),
            }
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
                    .expect("test carrier bytes fit the root limit");
            super::scan_carriers(&ctx, &bytes)
        });
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            panic!("carrier resource refusal");
        };
        assert_eq!(limit.dimension, dimension);
        assert_eq!(limit.operation, operation);
        limit
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
        )
        .expect("test merge fits service policy");
        base.merge_missing(&ctx, delta)
            .expect("test merge fits service policy");

        let pair = base
            .blend_support_pair(&ctx, 9)
            .expect("support lookup")
            .expect("support pair");
        assert_eq!(pair.supports, [11, 12]);
        assert_eq!(pair.intersection, 13);
    }
}
