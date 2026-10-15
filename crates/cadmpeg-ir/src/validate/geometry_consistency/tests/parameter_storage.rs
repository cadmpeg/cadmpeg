// SPDX-License-Identifier: Apache-2.0

use crate::scalar::FiniteReal;

#[test]
fn parameter_uniqueness_preserves_first_values_and_negative_zero() {
    let values = super::super::unique([3.0, f64::NAN, -0.0, 0.0, 3.0, 2.0], FiniteReal::new);
    assert_eq!(
        values.iter().map(|value| value.get()).collect::<Vec<_>>(),
        [3.0, -0.0, 2.0]
    );
    assert!(values[1].get().is_sign_negative());
}

#[test]
fn declared_parameter_candidates_keep_order() {
    let pcurve = crate::geometry::pcurve::Pcurve {
        id: "test:model:pcurve#ranges".try_into().unwrap(),
        geometry: crate::geometry::pcurve::PcurveGeometry::Line(
            crate::geometry::pcurve::LinePcurve::try_new(
                crate::math::Point2::new(0.0, 0.0),
                crate::math::Point2::new(1.0, 0.0),
            )
            .unwrap(),
        ),
        metadata: crate::geometry::pcurve::PcurveMetadata::default(),
    };
    let ctx = cadmpeg_test_support::service_decode_context();
    let ranges =
        super::super::pcurve_parameter_ranges(&ctx, &pcurve, Some([2.0, 3.0]), Some([0.0, 1.0]))
            .unwrap()
            .unwrap();
    assert_eq!(&ranges[..], [[2.0, 3.0], [0.0, 1.0], [-0.0, -1.0]]);
    assert!(ranges[2][0].is_sign_negative());
    let pairs = super::super::parameter_pairs(
        &[FiniteReal::ONE],
        &[FiniteReal::new(3.0).unwrap(), FiniteReal::new(4.0).unwrap()],
    );
    assert_eq!(&pairs[..], [[1.0, 3.0], [1.0, 4.0]]);
    ctx.finish_session().unwrap();
}
