// SPDX-License-Identifier: Apache-2.0
#[cfg(test)]
use super::{blend, offset, sweep, Carrier, SurfaceCarrier};
use super::{intersection, CurveCarrier};
#[cfg(test)]
use cadmpeg_core::decode::DecodeContext;
#[cfg(test)]
use cadmpeg_ir::report::loss::LossNote;
#[cfg(test)]
use std::collections::{BTreeMap, BTreeSet};

pub(in crate::brep) mod publication;
pub(in crate::brep) mod scan;
pub(in crate::brep) mod stream;
pub(in crate::brep) mod view;

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

#[cfg(test)]
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

#[cfg(test)]
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
            self.curves.keys().copied(),
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

}

/// Retained fixture adapter over the production stream scanner.
#[cfg(test)]
pub(super) fn scan_carriers(
    ctx: &DecodeContext<'_>, body: &[u8],
) -> Result<CarrierIndex, cadmpeg_core::CodecError> {
    scan::scan_stream_carriers(ctx, body)?.into_retained(ctx)
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
                    policy.limits.max_collection_items = cap;
                }
                cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                    policy.limits.max_work_units = cap;
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
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .expect("test merge fits service policy");
        let mut base = super::stream::StreamCarrierIndex::empty(&ctx).unwrap();
        let mut delta = super::stream::StreamCarrierIndex::empty(&ctx).unwrap();
        delta.blend_support_pairs.replace(&ctx, 9, blend::SupportPairCarrier {
            supports: [11, 12], intersection: 13,
        }, "test blend support pair").unwrap();
        base.merge_missing(&ctx, delta)
            .expect("test merge fits service policy");

        let view = super::view::CarrierView::Stream(&base);
        let pair = view
            .blend_support_pair(&ctx, 9)
            .expect("support lookup")
            .expect("support pair");
        assert_eq!(pair.supports, [11, 12]);
        assert_eq!(pair.intersection, 13);
    }
}

#[cfg(test)]
mod composition_tests;
