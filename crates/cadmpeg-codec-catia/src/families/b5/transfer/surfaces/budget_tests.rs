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

#[test]
fn revolution_raw_input_lanes_leave_only_selected_output_retained() {
    use crate::test_support::test_b5::{increasing, point};
    let profile = B5Profile::Line {
        point: point([1.0, 0.0, 0.0]),
        direction: crate::checked::ExactUnitVector3::new([0.0, 0.0, 1.0]).expect("unit direction"),
        parameter_range: increasing([0.0, 1.0]),
    };
    // Four f64 knots and two finite three-coordinate poles.
    let profile = crate::test_support::with_retained_limit(4 * 8 + 2 * 24, |ctx| {
        profile_nurbs(
            ctx,
            &profile,
            [0.0, 1.0],
            &"line",
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
    .expect("only final profile lanes are retained")
    .expect("linear profile");
    assert_eq!(profile.knots().as_slice(), &[0.0, 0.0, 1.0, 1.0]);
    // Six f64 knots and three finite weighted three-coordinate poles.
    let arc = crate::test_support::with_retained_limit(6 * 8 + 3 * 32, |ctx| {
        rational_arc(
            ctx,
            [0.0; 3],
            ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            1.0,
            [0.0, std::f64::consts::FRAC_PI_2],
            &"arc",
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
    .expect("only final arc lanes are retained")
    .expect("quarter arc");
    assert_eq!(arc.pole_count(), 3);
    // Four U knots, six V knots, two Vec row descriptors and six weighted poles.
    let surface =
        crate::test_support::with_retained_limit(4 * 8 + 6 * 8 + 2 * 24 + 6 * 32, |ctx| {
            revolve_nurbs(
                ctx,
                &profile,
                [0.0; 3],
                [0.0, 0.0, 1.0],
                [[0.0, std::f64::consts::FRAC_PI_2], [0.0, 1.0]],
                &"surface",
                &mut crate::nurbs::LaneRefusals::new(),
            )
        })
        .expect("only final surface lanes are retained")
        .expect("quarter revolution");
    assert_eq!(surface.u_knots().as_slice(), &[0.0, 0.0, 1.0, 1.0]);
    assert_eq!(
        surface.v_knots().as_slice(),
        &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]
    );
}
