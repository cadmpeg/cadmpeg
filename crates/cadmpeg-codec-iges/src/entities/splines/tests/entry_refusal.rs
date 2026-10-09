// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::CadIr;
use crate::entities::geometry::SourceSequences;

#[test]
fn spline_invalid_edge_interval_preserves_original_refusal() {
    let nurbs = crate::test_support::with_service_context(&[], |ctx| {
        NurbsCurve::from_lanes(ctx, 1, vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)], None, false)
            .expect("fixture admission").expect("valid fixture")
    });
    // Prebuild all consumed arguments outside the measured caller sessions.
    // Seven sessions each call the entry sixty-four times.
    let mut fixtures: Vec<_> = (0..7 * 64).map(|_| nurbs.clone()).collect();
    let entry = crate::test_support::directory_target(1, 112);
    let mut ir = CadIr::empty();
    let before = ir.model.clone();
    let mut sequences = SourceSequences::default();
    crate::test_support::with_entry_context(|ctx, original| {
        let result = super::super::add_edge(&mut ir, &entry,
            fixtures.pop().expect("prebuilt fixture"),
            [FiniteReal::ZERO, FiniteReal::ZERO], &mut sequences, ctx);
        match original {
            Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
            None => assert!(matches!(result, Ok(None))),
        }
        assert_eq!(ir.model, before);
    });
    assert!(fixtures.is_empty());
}
