// SPDX-License-Identifier: Apache-2.0
//! Temporary angular lanes for revolution surfaces.

use super::*;

#[test]
fn revolution_angular_lanes_use_materialized_storage() {
    let profile = crate::test_support::with_service_context(|ctx| {
        NurbsCurve::from_lanes(
            ctx,
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![
                cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 0.0, 1.0),
            ],
            None,
            false,
        )
    })
    .expect("profile construction budget")
    .expect("valid linear profile");
    let Err(CodecError::ResourceLimit(limit)) =
        crate::test_support::with_materialized_limit(0, |ctx| {
            revolve_nurbs(
                ctx,
                &profile,
                [0.0; 3],
                [0.0, 0.0, 1.0],
                [[0.0, std::f64::consts::FRAC_PI_2], [0.0, 1.0]],
                &"test record",
                &mut crate::nurbs::LaneRefusals::new(),
            )
        })
    else {
        panic!("angular storage refusal")
    };
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes
    );
    assert_eq!(limit.operation, "catia b5 revolution angles");
}
