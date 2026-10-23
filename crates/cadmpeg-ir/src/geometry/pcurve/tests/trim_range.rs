// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::geometry::pcurve::PcurveGeometry;
use crate::math::Point2;

fn nurbs_pcurve_leaf() -> PcurveGeometry {
    PcurveGeometry::Nurbs {
        nurbs: crate::geometry::pcurve::PcurveNurbs::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point2::new(0.0, 0.0), Point2::new(1.0, 1.0)],
            None,
            false,
        )
        .unwrap(),
    }
}

fn placed_pcurve(placements: usize, leaf: PcurveGeometry) -> Result<PcurveGeometry, &'static str> {
    let mut geometry = leaf;
    for _ in 0..placements {
        geometry = PcurveGeometry::Transformed(crate::geometry::pcurve::PlacedPcurve::try_new(
            Box::new(geometry),
            crate::transform::Transform2::identity(),
        )?);
    }
    Ok(geometry)
}

#[test]
fn pcurve_trim_range_stops_at_the_admitted_nesting_depth() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_recursion_depth =
        cadmpeg_core::decode::u64_from_index(crate::geometry::MAX_GEOMETRY_NESTING + 1);
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let trimmed = PcurveGeometry::Trimmed(
        crate::geometry::pcurve::TrimmedPcurve::try_new(
            [0.25, 0.75],
            true,
            Box::new(nurbs_pcurve_leaf()),
        )
        .unwrap(),
    );
    // The trim itself is one carrier, so the chain over it holds one less.
    let accepted = placed_pcurve(crate::geometry::MAX_GEOMETRY_NESTING - 1, trimmed.clone())
        .expect("admitted nesting");
    assert_eq!(accepted.trim_range(&ctx).unwrap(), Some([0.25, 0.75]));

    // Constructor admission also rejects a carrier beyond the inline bound.
    assert_eq!(
        placed_pcurve(crate::geometry::MAX_GEOMETRY_NESTING, trimmed),
        Err("PlacedPcurve.basis nests past the admitted inline basis depth")
    );
}

#[test]
fn pcurve_geometry_trim_range_preserves_session_depth_refusals() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let geometry = placed_pcurve(2, nurbs_pcurve_leaf()).unwrap();
    for (cap, held_frame) in [(0, false), (2, false), (2, true)] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let guard = held_frame.then(|| ctx.enter_nested_limit("caller frame").unwrap());
        let limit = geometry.trim_range(&ctx).unwrap_err();
        assert_eq!(limit.dimension, ResourceDimension::RecursionDepth);
        assert_eq!(limit.limit, cap);
        assert_eq!(limit.used, cap);
        assert_eq!(limit.additional, 1);
        assert_eq!(limit.operation, "pcurve trim range nesting");
        drop(guard);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
        );
    }
}
