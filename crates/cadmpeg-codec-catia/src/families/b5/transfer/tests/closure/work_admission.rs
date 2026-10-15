// SPDX-License-Identifier: Apache-2.0
//! Work admission of owner operations.

use super::{
    b5_edge_support_definition, charged_b5_edge_support_definition,
    charged_curve_on_parameter_range, charged_rational_arc, rational_arc,
};
use cadmpeg_ir::geometry::{
    nurbs::NurbsCurve,
    pcurve::{PcurveGeometry, PcurveNurbs},
    CurveGeometry, SolvedCurveGeometry,
};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point2, Point3};
use std::collections::BTreeMap;

#[test]
fn edge_support_pcurve_copy_refuses_collection_limit() {
    let surfaces = std::collections::BTreeMap::from([(
        10,
        SurfaceId::mint("catia:b5:surface#10".to_string()).expect("identity grammar"),
    )]);
    let pcurves = BTreeMap::from([(
        20,
        (
            PcurveGeometry::Nurbs {
                nurbs: PcurveNurbs::from_lanes(
                    &cadmpeg_test_support::service_decode_context(),
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
                    None,
                    false,
                )
                .expect("fixture pcurve construction admission")
                .expect("valid pcurve"),
            },
            false,
            crate::test_support::test_b5::finite_pair([0.0, 1.0]),
        ),
    )]);
    let supports = [(
        10,
        20,
        crate::test_support::test_b5::finite_pair([0.0, 1.0]),
    )];
    let refused = crate::test_support::with_collection_limit(0, |ctx| {
        charged_b5_edge_support_definition(ctx, &supports, &surfaces, &pcurves, None)
    });
    assert!(
        matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(error))
        if error.operation == "catia_b5_edge_support_pcurve")
    );
    assert!(b5_edge_support_definition(&supports, &surfaces, &pcurves, None).is_some());
}

#[test]
fn rational_arc_refuses_collection_limit_before_control_net() {
    let refused = crate::test_support::with_collection_limit(4, |ctx| {
        charged_rational_arc(
            ctx,
            [0.0; 3],
            ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            1.0,
            [0.0, std::f64::consts::FRAC_PI_2],
            &"arc",
            &mut crate::nurbs::LaneRefusals::new(),
        )
    });
    assert!(matches!(
        refused,
        Err(cadmpeg_core::CodecError::ResourceLimit(_))
    ));
    assert!(rational_arc(
        [0.0; 3],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        1.0,
        [0.0, std::f64::consts::FRAC_PI_2],
        &"arc",
        &mut crate::nurbs::LaneRefusals::new()
    )
    .is_some());
}

#[test]
fn reparameterized_nurbs_knots_refuse_collection_limit_below_need() {
    let curve = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![10.0, 10.0, 20.0, 20.0],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            None,
            false,
        )
        .expect("fixture constructor admission")
        .expect("valid curve"),
    ));
    let target = crate::test_support::test_b5::increasing([0.0, 2.0]);
    let refused = crate::test_support::with_collection_limit(3, |ctx| {
        charged_curve_on_parameter_range(
            ctx,
            curve.clone(),
            [10.0, 20.0],
            target,
            &"curve",
            &mut crate::nurbs::LaneRefusals::new(),
        )
    });
    assert!(matches!(
        refused,
        Err(cadmpeg_core::CodecError::ResourceLimit(_))
    ));
    let admitted = crate::test_support::with_service_context(|ctx| {
        charged_curve_on_parameter_range(
            ctx,
            curve,
            [10.0, 20.0],
            target,
            &"curve",
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
    .expect("service budget");
    assert!(admitted.is_some());
}

#[test]
fn b5_oriented_member_order_propagates_caller_work_refusal() {
    let orientation = super::super::super::OrientedLoop {
        flipped: true,
        members: vec![
            super::super::super::OrientedLoopMember {
                reversed: false,
                pcurve_reversed: false,
            };
            2
        ],
    };
    crate::test_support::with_work_limit(1, |ctx| {
        let Err(error) = orientation.member_order(ctx) else {
            panic!("two member visits exceed the caller work limit");
        };
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            panic!("resource refusal required")
        };
        assert_eq!(limit.operation, "catia_b5_oriented_loop_member_order");
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
    crate::test_support::with_work_limit(2, |ctx| {
        assert_eq!(
            orientation
                .member_order(ctx)
                .expect("two member visits")
                .rev()
                .collect::<Vec<_>>(),
            vec![0, 1]
        );
    });
}
