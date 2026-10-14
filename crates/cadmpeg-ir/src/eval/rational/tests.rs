// SPDX-License-Identifier: Apache-2.0
use super::Homogeneous;
use crate::eval::decode::Scratch;

fn weight_sum(weight: Option<crate::math::sum::ScaledValue>) -> Homogeneous {
    Homogeneous {
        values: [None, None, None, weight],
        constant: [None; 3],
    }
}

#[test]
fn homogeneous_weights_preserve_early_missing_and_empty_input() {
    let missing = [
        weight_sum(None),
        weight_sum(crate::math::sum::scaled_finite(1.0)),
    ];
    for (values, expected) in [(&missing[..], None), (&[][..], Some(Vec::<f64>::new()))] {
        let ctx = cadmpeg_test_support::service_decode_context();
        let scratch = Scratch::new(&ctx);
        assert_eq!(Homogeneous::weights(&scratch, values).unwrap(), expected);
        ctx.finish_session().unwrap();
    }
}

#[test]
fn homogeneous_sum_projects_unit_weight_poles_and_keeps_exact_cancellation() {
    use super::finite_lanes;
    use crate::features::FinitePoint3;
    use crate::math::Point3;
    for (points, expected) in [
        (
            [
                Point3::new(0.0, 2.0, 4.0),
                Point3::new(3.0, 2.0, 1.0),
                Point3::new(6.0, 2.0, -2.0),
            ],
            [3.0, 2.0, 1.0],
        ),
        (
            [
                Point3::new(-1.0, 2.0, 0.0),
                Point3::new(0.0, 2.0, 0.0),
                Point3::new(1.0, 2.0, 0.0),
            ],
            [0.0, 2.0, 0.0],
        ),
    ] {
        let ctx = cadmpeg_test_support::service_decode_context();
        let scratch = Scratch::new(&ctx);
        let poles = points.map(|point| FinitePoint3::new(point).expect("finite pole"));
        let sum = Homogeneous::sum(
            &scratch,
            poles
                .into_iter()
                .map(|point| Some(([1.0, 1.0], 1.0, point))),
        )
        .expect("no refusal")
        .expect("sum");
        assert_eq!(
            finite_lanes(sum.project(sum, &[]).expect("nonzero denominator"))
                .expect("finite quotient")
                .map(crate::scalar::FiniteReal::get),
            expected
        );
        ctx.finish_session().unwrap();
    }
}

#[test]
fn homogeneous_weights_rescale_the_net_when_a_weight_leaves_the_normal_range() {
    use crate::math::sum::scaled_finite;
    let normal = [
        weight_sum(scaled_finite(1.0)),
        weight_sum(scaled_finite(2.0)),
    ];
    let tiny = [
        weight_sum(scaled_finite(f64::from_bits(1))),
        weight_sum(scaled_finite(f64::from_bits(2))),
    ];
    let large = scaled_finite(f64::MAX).unwrap().doubled();
    let overflow = [weight_sum(Some(large)); 2];
    let impossible = [
        weight_sum(Some(large)),
        weight_sum(scaled_finite(f64::from_bits(1))),
    ];
    for (values, expected) in [
        (&normal[..], Some(vec![1.0, 2.0])),
        (
            &tiny[..],
            Some(vec![f64::MIN_POSITIVE, f64::MIN_POSITIVE * 2.0]),
        ),
        (&overflow[..], Some(vec![f64::MAX * 0.5; 2])),
        (&impossible[..], None),
    ] {
        let ctx = cadmpeg_test_support::service_decode_context();
        let scratch = Scratch::new(&ctx);
        assert_eq!(Homogeneous::weights(&scratch, values).unwrap(), expected);
        ctx.finish_session().unwrap();
    }
}
