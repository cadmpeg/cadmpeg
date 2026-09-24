// SPDX-License-Identifier: Apache-2.0
use crate::geometry::{
    pcurve::PcurveGeometry, DirectedParameterRange, IntcurveSupportSide, SupportPcurve,
};
use crate::scalar::FiniteReal;
use crate::test_support::nurbs::pcurve;

#[test]
fn numerical_audit_identity_map_keeps_a_small_finite_parameter() {
    let side = IntcurveSupportSide {
        surface: None,
        pcurve: Some(SupportPcurve::new(
            PcurveGeometry::Nurbs { nurbs: pcurve() },
            Some(DirectedParameterRange::new([0.0, 1e308]).unwrap()),
        )),
    };
    let parameter = 1e-308;
    let mapped = side
        .pcurve_parameter([0.0, 1e308], parameter)
        .unwrap()
        .get();
    assert!((mapped / parameter - 1.0).abs() <= 8.0 * f64::EPSILON);
}

#[test]
fn support_mapping_preserves_decreasing_parameter_direction() {
    for endpoints in [[0.0, 0.0], [0.0, f64::NAN], [f64::INFINITY, 0.0]] {
        assert!(DirectedParameterRange::new(endpoints).is_err());
    }
    assert!(serde_json::from_str::<DirectedParameterRange>("[2.0,2.0]").is_err());
    let side = IntcurveSupportSide {
        surface: None,
        pcurve: Some(SupportPcurve::new(
            PcurveGeometry::Nurbs { nurbs: pcurve() },
            Some(DirectedParameterRange::new([5.0, 2.0]).unwrap()),
        )),
    };
    assert_eq!(
        side.pcurve_parameter([0.0, 1.0], 0.0).map(FiniteReal::get),
        Some(5.0)
    );
    assert_eq!(
        side.pcurve_parameter([0.0, 1.0], 1.0).map(FiniteReal::get),
        Some(2.0)
    );
    let range = side
        .pcurve_parameter_range()
        .expect("the side states its pcurve range");
    assert_eq!(range.endpoints(), [5.0, 2.0]);
    assert_eq!(range.finite_endpoints().map(FiniteReal::get), [5.0, 2.0]);
    assert_eq!(
        serde_json::from_value::<IntcurveSupportSide>(serde_json::to_value(&side).unwrap())
            .unwrap(),
        side
    );
}

#[test]
fn finite_support_endpoints_do_not_require_a_representable_span() {
    for (solved, mapped, midpoint) in [
        ([-f64::MAX, f64::MAX], [5.0, 2.0], 3.5),
        ([-1.0, 1.0], [-f64::MAX, f64::MAX], 0.0),
        ([-f64::MAX, f64::MAX], [f64::MAX, -f64::MAX], 0.0),
    ] {
        let side = IntcurveSupportSide {
            surface: None,
            pcurve: Some(SupportPcurve::new(
                PcurveGeometry::Nurbs { nurbs: pcurve() },
                Some(DirectedParameterRange::new(mapped).unwrap()),
            )),
        };
        assert_eq!(
            side.pcurve_parameter(solved, solved[0])
                .map(FiniteReal::get),
            Some(mapped[0])
        );
        assert_eq!(
            side.pcurve_parameter(solved, 0.0).map(FiniteReal::get),
            Some(midpoint)
        );
        assert_eq!(
            side.pcurve_parameter(solved, solved[1])
                .map(FiniteReal::get),
            Some(mapped[1])
        );
        assert_eq!(
            side.pcurve_parameter(solved, f64::NAN).map(FiniteReal::get),
            None
        );
    }
}

#[test]
fn support_mapping_preserves_endpoints_despite_subtraction_cancellation() {
    let side = IntcurveSupportSide {
        surface: None,
        pcurve: Some(SupportPcurve::new(
            PcurveGeometry::Nurbs { nurbs: pcurve() },
            Some(DirectedParameterRange::new([1e16, 1.0]).unwrap()),
        )),
    };
    assert_eq!(
        side.pcurve_parameter([0.0, 1.0], 0.0).map(FiniteReal::get),
        Some(1e16)
    );
    assert_eq!(
        side.pcurve_parameter([0.0, 1.0], 1.0).map(FiniteReal::get),
        Some(1.0)
    );
}

#[test]
fn a_support_context_over_an_increasing_interval_matches_its_raw_admission() {
    use crate::geometry::IntcurveSupportContext;
    use crate::topology::IncreasingParameterInterval;

    let side = |range| IntcurveSupportSide {
        surface: None,
        pcurve: Some(SupportPcurve::new(
            PcurveGeometry::Nurbs { nurbs: pcurve() },
            range,
        )),
    };
    let sides = [
        side(Some(DirectedParameterRange::new([1.0, 0.0]).unwrap())),
        side(None),
    ];
    let interval = IncreasingParameterInterval::new([0.25, 3.0]).unwrap();
    assert_eq!(
        IntcurveSupportContext::try_new(
            sides.clone(),
            interval.endpoints(),
            std::array::from_fn(|_| Vec::new()),
        ),
        Ok(IntcurveSupportContext::over_interval(sides, interval))
    );
}
