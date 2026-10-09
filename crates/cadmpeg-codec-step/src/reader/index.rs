// SPDX-License-Identifier: Apache-2.0
//! Numeric indexes for STEP geometry carriers.

use std::collections::HashMap;

use super::step_instance_id;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::math::Point3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct CurveIndex(pub(super) usize);

impl cadmpeg_core::decode::cost::DecodeCost for CurveIndex {
    const FIXED_BYTES: Option<u64> =
        Some(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()));
    fn decode_cost(
        &self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        Ok(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct SurfaceIndex(pub(super) usize);

impl cadmpeg_core::decode::cost::DecodeCost for SurfaceIndex {
    const FIXED_BYTES: Option<u64> =
        Some(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()));
    fn decode_cost(
        &self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        Ok(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct PointIndex(pub(super) usize);

impl cadmpeg_core::decode::cost::DecodeCost for PointIndex {
    const FIXED_BYTES: Option<u64> =
        Some(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()));
    fn decode_cost(
        &self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        Ok(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()))
    }
}

pub(super) struct PointCarrier {
    pub(super) index: PointIndex,
    position: Point3,
}

/// Decode-local lookup tables for STEP carrier instance ids.
pub(super) struct CarrierIndex {
    pub(super) curves: HashMap<u64, CurveIndex>,
    pub(super) points: HashMap<u64, PointCarrier>,
    pub(super) surfaces: HashMap<u64, SurfaceIndex>,
}

impl CarrierIndex {
    pub(super) fn from_ir(ir: &CadIr, ctx: &DecodeContext<'_>) -> Result<Self, CodecError> {
        // Preserve the original empty-index route's sticky session gate.
        ctx.charge_work(0, "STEP from ir traversal")?;
        let mut curves = HashMap::new();
        let mut curve_source = ir.model.curves.iter().enumerate();
        for _ in 0..curve_source.len() {
            let (index, curve) = ctx.next_charged(&mut curve_source, "STEP from ir traversal")?
                .ok_or_else(|| CodecError::malformed("STEP carrier traversal source ended early"))?;
            if let Some(id) = step_instance_id(ctx, curve.id.as_str())? {
                ctx.insert_hash_map(
                    &mut curves,
                    id,
                    CurveIndex(index),
                    "step_carrier_curve_index",
                )?;
            }
        }
        let mut points = HashMap::new();
        let mut point_source = ir.model.points.iter().enumerate();
        for _ in 0..point_source.len() {
            let (index, point) = ctx.next_charged(&mut point_source, "STEP from ir traversal")?
                .ok_or_else(|| CodecError::malformed("STEP carrier traversal source ended early"))?;
            if let Some(id) = step_instance_id(ctx, point.id.as_str())? {
                ctx.insert_hash_map(
                    &mut points,
                    id,
                    PointCarrier {
                        index: PointIndex(index),
                        position: point.position().get(),
                    },
                    "step_carrier_point_index",
                )?;
            }
        }
        let mut surfaces = HashMap::new();
        let mut surface_source = ir.model.surfaces.iter().enumerate();
        for _ in 0..surface_source.len() {
            let (index, surface) = ctx.next_charged(&mut surface_source, "STEP from ir traversal")?
                .ok_or_else(|| CodecError::malformed("STEP carrier traversal source ended early"))?;
            if let Some(id) = step_instance_id(ctx, surface.id.as_str())? {
                ctx.insert_hash_map(
                    &mut surfaces,
                    id,
                    SurfaceIndex(index),
                    "step_carrier_surface_index",
                )?;
            }
        }
        Ok(Self {
            curves,
            points,
            surfaces,
        })
    }

    pub(super) fn get(&self, id: u64, ctx: &DecodeContext<'_>) -> Result<Option<&Point3>, CodecError> {
        Ok(ctx.get_hash_map(&self.points, &id, "step_carrier_point_lookup")?
            .map(|point| &point.position))
    }

    pub(super) fn contains_key(&self, id: u64, ctx: &DecodeContext<'_>) -> Result<bool, CodecError> {
        ctx.contains_key_hash_map(&self.points, &id, "step_carrier_point_lookup")
    }
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::geometry::{
        analytic::{LineCurve, PlaneSurface},
        Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{CurveId, PointId, SurfaceId};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::topology::Point;
    use cadmpeg_ir::CadIr;

    use super::CarrierIndex;

    fn carriers() -> CadIr {
        let mut ir = CadIr::empty();
        ir.model.curves.push(Curve {
            id: CurveId::from(crate::ids::data(crate::ids::kind!("curve"), 1)),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                LineCurve::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0))
                    .expect("finite line"),
            )),
            source_object: None,
        });
        ir.model.points.push(Point::new(
            PointId::from(crate::ids::data(crate::ids::kind!("point"), 2)),
            cadmpeg_ir::features::FinitePoint3::ZERO,
            None,
        ));
        ir.model.surfaces.push(Surface {
            id: SurfaceId::from(crate::ids::data(crate::ids::kind!("surface"), 3)),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("finite plane"),
            )),
            source_object: None,
        });
        ir
    }

    fn assert_index_refusal(limit: u64, operation: &str) {
        let ir = carriers();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
            .expect("empty root fits selected policy");
        let error = CarrierIndex::from_ir(&ir, &ctx)
            .err()
            .expect("index exceeds item limit");
        assert!(matches!(
            error,
            CodecError::ResourceLimit(refusal)
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == operation
        ));
    }

    #[test]
    fn curve_carrier_index_refuses_collection_limit() {
        assert_index_refusal(0, "step_carrier_curve_index");
    }

    #[test]
    fn point_carrier_index_refuses_collection_limit() {
        assert_index_refusal(1, "step_carrier_point_index");
    }

    #[test]
    fn surface_carrier_index_refuses_collection_limit() {
        assert_index_refusal(2, "step_carrier_surface_index");
    }

    fn assert_point_lookup_work(
        lookup: impl Fn(&CarrierIndex, u64, &DecodeContext<'_>) -> Result<bool, CodecError>,
    ) {
        let index = CarrierIndex {
            curves: std::collections::HashMap::new(),
            points: std::collections::HashMap::from([(2, super::PointCarrier {
                index: super::PointIndex(0), position: Point3::new(0.0, 0.0, 0.0),
            })]),
            surfaces: std::collections::HashMap::new(),
        };
        // Core hash lookup admits one hash of the actual u64 key bytes.
        let key_bytes = u64::try_from(std::mem::size_of::<u64>()).unwrap();
        for id in [2, 9] {
            for cap in [key_bytes - 1, key_bytes] {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                policy.limits.max_collection_items = 0;
                policy.limits.max_materialized_bytes = 0;
                policy.limits.max_retained_bytes = 0;
                let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
                if cap == key_bytes {
                    assert_eq!(lookup(&index, id, &ctx).unwrap(), id == 2);
                    ctx.finish_session().unwrap();
                } else {
                    let error = lookup(&index, id, &ctx).expect_err("key hash exceeds work limit");
                    let CodecError::ResourceLimit(refusal) = error else {
                        panic!("point lookup resource refusal");
                    };
                    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
                    assert_eq!(refusal.operation, "step_carrier_point_lookup");
                    assert_eq!(refusal.used, 0);
                    assert_eq!(refusal.additional, key_bytes);
                    assert_eq!(ctx.resource_refusal(), Some(refusal));
                    assert!(matches!(lookup(&index, id, &ctx),
                        Err(CodecError::ResourceLimit(sticky)) if sticky == refusal));
                    assert!(matches!(ctx.finish_session(),
                        Err(CodecError::ResourceLimit(sticky)) if sticky == refusal));
                }
            }
        }
    }

    #[test]
    fn point_carrier_lookup_preserves_work_refusal() {
        assert_point_lookup_work(|index, id, ctx| Ok(index.get(id, ctx)?.is_some()));
    }

    #[test]
    fn point_carrier_membership_preserves_work_refusal() {
        assert_point_lookup_work(|index, id, ctx| index.contains_key(id, ctx));
    }

    #[test]
    fn carrier_index_refuses_only_first_curve_visit() {
        let mut ir = carriers();
        ir.model.curves = vec![ir.model.curves[0].clone(); 1024];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
        let error = CarrierIndex::from_ir(&ir, &ctx).err().expect("first curve visit refuses");
        let CodecError::ResourceLimit(refusal) = error else {
            panic!("carrier traversal resource refusal");
        };
        assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
        assert_eq!(refusal.operation, "STEP from ir traversal");
        assert_eq!(refusal.used, 0);
        assert_eq!(refusal.additional, 1);
        assert_eq!(ctx.resource_refusal(), Some(refusal));
        assert!(matches!(ctx.finish_session(),
            Err(CodecError::ResourceLimit(sticky)) if sticky == refusal));
    }

    #[test]
    fn empty_carrier_index_preserves_original_sticky_refusal() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
        let CodecError::ResourceLimit(refusal) = ctx.charge_work(1, "test original carrier refusal")
            .expect_err("original session refusal") else {
                panic!("work resource refusal");
            };
        assert!(matches!(CarrierIndex::from_ir(&CadIr::empty(), &ctx),
            Err(CodecError::ResourceLimit(sticky)) if sticky == refusal));
        assert!(matches!(ctx.finish_session(),
            Err(CodecError::ResourceLimit(sticky)) if sticky == refusal));
    }
}
