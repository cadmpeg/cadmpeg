// SPDX-License-Identifier: Apache-2.0
use proptest::prelude::{prop_oneof, Just, Strategy};
use proptest::{prop_assert_eq, proptest};

use super::Homogeneous;
use crate::eval::admission::{EvaluationAdmission, EvaluationRefusal};
use crate::eval::decode::Scratch;
use crate::features::geometry::FinitePoint3;

mod separate_traversals;

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
        let ctx = cadmpeg_core::decode::test_support::service_decode_context();
        let scratch = Scratch::new(&ctx);
        assert_eq!(Homogeneous::weights(&scratch, values).unwrap(), expected);
        ctx.finish_session().unwrap();
    }
}

#[test]
fn homogeneous_sum_projects_unit_weight_poles_and_keeps_exact_cancellation() {
    use super::finite_lanes;
    use crate::features::geometry::FinitePoint3;
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
        let ctx = cadmpeg_core::decode::test_support::service_decode_context();
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
        let ctx = cadmpeg_core::decode::test_support::service_decode_context();
        let scratch = Scratch::new(&ctx);
        assert_eq!(Homogeneous::weights(&scratch, values).unwrap(), expected);
        ctx.finish_session().unwrap();
    }
}

type Term = Option<([f64; 2], f64, FinitePoint3)>;

/// Each lane as its exponent and signed mantissa, and each constant
/// coordinate, bit for bit.
type Lanes = ([Option<(i32, u64)>; 4], [Option<u64>; 3]);

fn lanes(sum: Homogeneous) -> Lanes {
    (
        sum.values.map(|value| {
            value.map(|value| {
                let mantissa = value
                    .rescale(value.exponent())
                    .expect("a mantissa in [0.5, 1)");
                (value.exponent(), mantissa.get().to_bits())
            })
        }),
        sum.constant
            .map(|value| value.map(|value| value.get().to_bits())),
    )
}

fn sum_of<I>(
    scratch: &Scratch<'_, '_>,
    terms: I,
    shared: bool,
) -> Result<Option<Lanes>, EvaluationRefusal>
where
    I: Iterator<Item = Term> + Clone,
{
    let sum = if shared {
        Homogeneous::sum(scratch, terms)
    } else {
        separate_traversals::sum(scratch, terms)
    };
    sum.map(|sum| sum.map(lanes))
}

/// The terms in one of three traversal shapes: exact-sized, by rows whose
/// count is unknown until the last row, and of unknown size throughout,
/// which draws once more at the end.
fn shaped_sum(
    scratch: &Scratch<'_, '_>,
    terms: &[Term],
    shape: u8,
    shared: bool,
) -> Result<Option<Lanes>, EvaluationRefusal> {
    match shape {
        0 => sum_of(scratch, terms.iter().copied(), shared),
        1 => sum_of(
            scratch,
            terms.chunks(3).flat_map(|row| row.iter().copied()),
            shared,
        ),
        _ => {
            let mut index = 0;
            let terms = std::iter::from_fn(move || {
                let term = terms.get(index).copied();
                index += 1;
                term
            });
            sum_of(scratch, terms, shared)
        }
    }
}

/// The sum under a work slice of `budget` steps, and the budget it leaves:
/// its remainder, or `None` once exhausted.
fn budgeted_sum(
    terms: &[Term],
    shape: u8,
    shared: bool,
    budget: usize,
) -> (Result<Option<Lanes>, EvaluationRefusal>, Option<usize>) {
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let parent = cadmpeg_core::decode::WorkBudget::new(budget);
    let sum = EvaluationAdmission::Decode(&ctx)
        .within_work_slice(&parent, |admission| {
            Ok::<_, crate::eval::EvaluationFailure<()>>(shaped_sum(
                &Scratch::new(admission),
                terms,
                shape,
                shared,
            ))
        })
        .expect("no session refusal");
    ctx.finish_session().expect("the session finishes");
    (sum, (!parent.exhausted()).then(|| parent.remaining()))
}

fn any_real() -> impl Strategy<Value = f64> {
    prop_oneof![
        4 => (-4_i32..=4).prop_map(f64::from),
        1 => Just(-0.0),
        2 => (proptest::bool::ANY, 0_u64..0x7ff, 0_u64..1 << 52).prop_map(
            |(negative, field, fraction)| {
                f64::from_bits((u64::from(negative) << 63) | (field << 52) | fraction)
            }
        ),
        1 => proptest::sample::select(vec![
            f64::MAX,
            -f64::MAX,
            f64::MIN_POSITIVE,
            f64::from_bits(1),
            1.0e300,
            1.0e-300,
            0.1,
        ]),
    ]
}

fn any_blend() -> impl Strategy<Value = f64> {
    prop_oneof![
        24 => any_real(),
        1 => Just(f64::NAN),
        1 => Just(f64::INFINITY),
    ]
}

fn any_term() -> impl Strategy<Value = Term> {
    prop_oneof![
        24 => ([any_blend(), any_blend()], any_blend(), [any_real(), any_real(), any_real()])
            .prop_map(|(basis, weight, [x, y, z])| {
                let point = crate::math::Point3::new(x, y, z);
                Some((basis, weight, FinitePoint3::new(point).expect("finite pole")))
            }),
        1 => Just(None),
    ]
}

proptest! {
    #[test]
    fn one_shared_traversal_sums_and_charges_as_separate_traversals(
        terms in proptest::collection::vec(any_term(), 0..10),
        shape in 0_u8..3,
    ) {
        let ctx = cadmpeg_core::decode::test_support::service_decode_context();
        let unbudgeted = Scratch::new(&ctx);
        prop_assert_eq!(
            shaped_sum(&unbudgeted, &terms, shape, true),
            shaped_sum(&unbudgeted, &terms, shape, false)
        );
        ctx.finish_session().expect("the session finishes");

        let unlimited = 1 << 20;
        let (_, remaining) = budgeted_sum(&terms, shape, false, unlimited);
        let charged = unlimited - remaining.expect("an unlimited budget remains");
        for budget in (0..=charged + 1).chain([unlimited]) {
            prop_assert_eq!(
                budgeted_sum(&terms, shape, true, budget),
                budgeted_sum(&terms, shape, false, budget),
                "budget {}",
                budget
            );
        }
    }
}

#[test]
fn project_with_subtract_matches_exact_across_normal_and_cancelling_inputs() {
    use crate::math::sum::{scaled_finite, ExactSignedSum};
    use crate::scalar::FiniteReal;

    let real = |v: f64| FiniteReal::new(v).expect("finite");
    let make_h = |coords: [f64; 3], w: f64| Homogeneous {
        values: [
            scaled_finite(coords[0]),
            scaled_finite(coords[1]),
            scaled_finite(coords[2]),
            scaled_finite(w),
        ],
        constant: [None; 3],
    };

    let cases = [
        // Normal non-cancelling derivative subtraction (hits fast path).
        (
            make_h([12.5, -3.25, 7.0], 2.0),
            make_h([4.0, 2.0, -1.0], 1.5),
            vec![(
                make_h([0.0, 0.0, 0.0], 0.5),
                [real(2.0), real(-1.0), real(3.0)],
            )],
        ),
        // Exact cancellation (falls back to ExactSignedSum).
        (
            make_h([6.0, -4.0, 10.0], 2.0),
            make_h([1.0, 1.0, 1.0], 2.0),
            vec![(
                make_h([0.0, 0.0, 0.0], 2.0),
                [real(3.0), real(-2.0), real(5.0)],
            )],
        ),
        // Near-cancellation within 1e-12 (falls back to ExactSignedSum).
        (
            make_h([1.0 + 1e-12, 2.0, 0.0], 1.0),
            make_h([1.0, 1.0, 1.0], 1.0),
            vec![(
                make_h([0.0, 0.0, 0.0], 1.0),
                [real(1.0), real(2.0), real(0.0)],
            )],
        ),
        // Extreme scale outside normal product range (falls back to ExactSignedSum).
        (
            make_h([1e200, 1e-200, 0.0], 1e200),
            make_h([1.0, 1.0, 1.0], 1e200),
            vec![(
                make_h([0.0, 0.0, 0.0], 1e200),
                [real(1e150), real(1e-200), real(0.0)],
            )],
        ),
    ];

    for (deriv, base, subtract) in cases {
        let actual = deriv.project(base, &subtract).expect("nonzero denominator");
        let denom = base.values[3].expect("denom");
        for (axis, lane) in actual.into_iter().enumerate() {
            let mut exact = ExactSignedSum::default();
            exact
                .add_scaled_product(deriv.values[axis], FiniteReal::ONE)
                .expect("in range");
            for (weight, factor) in &subtract {
                exact
                    .add_scaled_product(weight.values[3], factor[axis].negated())
                    .expect("in range");
            }
            let expected = exact
                .finish()
                .map_or(Ok(FiniteReal::ZERO), |v| v.quotient(denom));
            match (lane, expected) {
                (Ok(a), Ok(e)) => {
                    let tol = 4.0 * f64::EPSILON * e.get().abs().max(f64::MIN_POSITIVE);
                    assert!(
                        (a.get() - e.get()).abs() <= tol,
                        "axis {axis}: got {}, expected {}",
                        a.get(),
                        e.get()
                    );
                }
                (Err(a), Err(e)) => assert_eq!(a.to_bits(), e.to_bits()),
                (a, e) => panic!("axis {axis}: mismatch {a:?} vs {e:?}"),
            }
        }
    }
}
