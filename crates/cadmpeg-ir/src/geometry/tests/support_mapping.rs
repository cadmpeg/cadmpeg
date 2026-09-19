// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::geometry::{
    pcurve::PcurveGeometry, DirectedParameterRange, IntcurveSupportSide, SupportPcurve,
};
use crate::test_support::nurbs::pcurve;

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
    assert_eq!(side.pcurve_parameter([0.0, 1.0], 0.0), Some(5.0));
    assert_eq!(side.pcurve_parameter([0.0, 1.0], 1.0), Some(2.0));
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
        assert_eq!(side.pcurve_parameter(solved, solved[0]), Some(mapped[0]));
        assert_eq!(side.pcurve_parameter(solved, 0.0), Some(midpoint));
        assert_eq!(side.pcurve_parameter(solved, solved[1]), Some(mapped[1]));
        assert_eq!(side.pcurve_parameter(solved, f64::NAN), None);
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
    assert_eq!(side.pcurve_parameter([0.0, 1.0], 0.0), Some(1e16));
    assert_eq!(side.pcurve_parameter([0.0, 1.0], 1.0), Some(1.0));
}
