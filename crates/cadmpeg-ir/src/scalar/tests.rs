#[test]
fn feature_scalars_reject_nonfinite_constructor_and_serde_values() {
    use crate::scalar::{Angle, Length};
    use serde::de::value::{Error, F64Deserializer};

    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(Length::new(value).is_none());
        assert!(Angle::new(value).is_none());
        assert!(
            <Length as serde::Deserialize>::deserialize(F64Deserializer::<Error>::new(value))
                .is_err()
        );
        assert!(
            <Angle as serde::Deserialize>::deserialize(F64Deserializer::<Error>::new(value))
                .is_err()
        );
    }
    for value in [-10.0, 0.0, 10.0] {
        let length = Length::new(value).unwrap();
        let angle = Angle::new(value).unwrap();
        assert_eq!(length.get(), value);
        assert_eq!(angle.get(), value);
        assert_eq!(
            serde_json::to_value(length).unwrap(),
            serde_json::json!(value)
        );
        assert_eq!(
            serde_json::to_value(angle).unwrap(),
            serde_json::json!(value)
        );
        assert_eq!(
            serde_json::from_value::<Length>(serde_json::json!(value)).unwrap(),
            length
        );
        assert_eq!(
            serde_json::from_value::<Angle>(serde_json::json!(value)).unwrap(),
            angle
        );
    }
}

#[test]
fn positive_lengths_reject_zero_negative_and_nonfinite_values() {
    use crate::scalar::PositiveLength;
    use serde::de::value::{Error, F64Deserializer};

    for value in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(PositiveLength::new(value).is_none());
        assert!(<PositiveLength as serde::Deserialize>::deserialize(
            F64Deserializer::<Error>::new(value)
        )
        .is_err());
    }
    let length = PositiveLength::new(2.5).unwrap();
    assert_eq!(length.get(), 2.5);
    assert_eq!(
        serde_json::to_value(length).unwrap(),
        serde_json::json!(2.5)
    );
    assert_eq!(
        serde_json::from_str::<PositiveLength>("2.5").unwrap(),
        length
    );
}

#[test]
fn positive_i64_admits_the_full_positive_signed_lane() {
    use crate::scalar::PositiveI64;

    for value in [0_i64, -1] {
        assert!(PositiveI64::new(value).is_none());
        assert!(serde_json::from_value::<PositiveI64>(serde_json::json!(value)).is_err());
    }
    assert!(serde_json::from_value::<PositiveI64>(serde_json::json!(1.5)).is_err());

    let maximum = PositiveI64::new(i64::MAX).unwrap();
    assert_eq!(maximum.get(), i64::MAX);
    assert_eq!(
        serde_json::to_value(maximum).unwrap(),
        serde_json::json!(i64::MAX)
    );
    assert_eq!(
        serde_json::from_value::<PositiveI64>(serde_json::json!(i64::MAX)).unwrap(),
        maximum
    );
}

#[test]
fn bounded_feature_scalars_reject_out_of_domain_values_on_every_admission_route() {
    use crate::scalar::{
        FiniteReal, Fraction, InteriorAngle, NonNegativeLength, NonZeroLength, NonZeroReal,
        PositiveAngle, PositiveReal, SlopeAngle,
    };
    use serde::de::value::{Error, F64Deserializer};
    use std::f64::consts::{FRAC_PI_2, PI, TAU};

    macro_rules! check {
        ($ty:ty, valid: [$($valid:expr),*], invalid: [$($invalid:expr),*]) => {
            for value in [$($valid),*] {
                let admitted = <$ty>::new(value).unwrap();
                assert_eq!(admitted.get().to_bits(), value.to_bits());
                let wire = serde_json::to_string(&admitted).unwrap();
                let decoded: $ty = serde_json::from_str(&wire).unwrap();
                assert_eq!(decoded.get().to_bits(), value.to_bits());
            }
            for value in [$($invalid,)* f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                assert!(<$ty>::new(value).is_none());
                assert!(<$ty as serde::Deserialize>::deserialize(F64Deserializer::<Error>::new(value)).is_err());
            }
        };
    }

    check!(NonNegativeLength, valid: [-0.0, 0.0, 2.0], invalid: [-1.0]);
    check!(SlopeAngle, valid: [-1.0, -0.0, 0.0, 1.0], invalid: [-FRAC_PI_2, FRAC_PI_2, PI]);
    check!(InteriorAngle, valid: [0.5, FRAC_PI_2], invalid: [-1.0, 0.0, PI]);
    check!(PositiveAngle, valid: [1.0, TAU, 2.0 * TAU], invalid: [-1.0, -0.0, 0.0]);
    check!(FiniteReal, valid: [-10.0, -0.0, 0.0, 10.0], invalid: []);
    check!(PositiveReal, valid: [0.5, 10.0], invalid: [-1.0, -0.0, 0.0]);
    check!(NonZeroLength, valid: [-2.0, 0.5], invalid: [-0.0, 0.0]);
    check!(NonZeroReal, valid: [-2.0, 0.5], invalid: [-0.0, 0.0]);
    check!(Fraction, valid: [-0.0, 0.0, 0.5, 1.0], invalid: [-0.5, 1.5]);
}

#[test]
fn unit_scalar_names_share_const_admitted_domains() {
    use super::{FiniteReal, NonNegativeReal, PositiveReal};
    const SIGNED: Option<FiniteReal> = FiniteReal::new(-2.0);
    const POSITIVE: Option<PositiveReal> = PositiveReal::new(2.0);
    const NONNEGATIVE: Option<NonNegativeReal> = NonNegativeReal::new(0.0);
    const INVALID: Option<NonNegativeReal> = NonNegativeReal::new(-1.0);
    let signed: crate::scalar::FiniteReal = SIGNED.unwrap();
    let positive: crate::scalar::PositiveReal = POSITIVE.unwrap();
    let nonnegative: crate::scalar::NonNegativeReal = NONNEGATIVE.unwrap();
    assert_eq!(signed.get(), -2.0);
    assert_eq!(positive.get(), 2.0);
    assert_eq!(nonnegative.get(), 0.0);
    assert_eq!(INVALID, None);
}

#[test]
fn every_subset_edge_widens_without_a_check_and_restricts_through_admission() {
    use crate::scalar::{
        Angle, FiniteReal, Fraction, InteriorAngle, Length, NonNegativeLength, NonNegativeReal,
        NonZeroAngle, NonZeroLength, NonZeroReal, PositiveAngle, PositiveLength, PositiveReal,
        SlopeAngle,
    };

    // Each row is one subset edge: the source domain, a value inside it, and a
    // value the destination accepts that the source refuses.
    macro_rules! edge {
        ($from:ty => $to:ty, inside: $inside:expr, outside: $outside:expr) => {{
            let inside: f64 = $inside;
            let outside: f64 = $outside;
            let source = <$from>::new(inside).expect("an inside value");
            let widened: $to = <$to>::from(source);
            assert_eq!(widened.get().to_bits(), inside.to_bits());
            let restricted = <$from>::try_from(widened).expect("the value stays inside");
            assert_eq!(restricted.get().to_bits(), inside.to_bits());
            let refused = <$to>::new(outside).expect("an outside value the destination accepts");
            assert!(<$from>::try_from(refused).is_err());
            1_usize
        }};
    }

    let edges = edge!(PositiveLength => NonZeroLength, inside: 2.0, outside: -2.0)
        + edge!(PositiveLength => NonNegativeLength, inside: 2.0, outside: 0.0)
        + edge!(PositiveLength => Length, inside: 2.0, outside: -2.0)
        + edge!(NonZeroLength => Length, inside: -2.0, outside: 0.0)
        + edge!(NonNegativeLength => Length, inside: 0.0, outside: -2.0)
        + edge!(InteriorAngle => PositiveAngle, inside: 1.0, outside: 4.0)
        + edge!(InteriorAngle => NonZeroAngle, inside: 1.0, outside: -1.0)
        + edge!(InteriorAngle => Angle, inside: 1.0, outside: 0.0)
        + edge!(PositiveAngle => NonZeroAngle, inside: 1.0, outside: -1.0)
        + edge!(PositiveAngle => Angle, inside: 1.0, outside: -1.0)
        + edge!(NonZeroAngle => Angle, inside: -1.0, outside: 0.0)
        + edge!(SlopeAngle => Angle, inside: 1.0, outside: 3.0)
        + edge!(PositiveReal => NonZeroReal, inside: 2.0, outside: -2.0)
        + edge!(PositiveReal => NonNegativeReal, inside: 2.0, outside: 0.0)
        + edge!(PositiveReal => FiniteReal, inside: 2.0, outside: -2.0)
        + edge!(NonZeroReal => FiniteReal, inside: -2.0, outside: 0.0)
        + edge!(NonNegativeReal => FiniteReal, inside: 0.0, outside: -2.0)
        + edge!(Fraction => NonNegativeReal, inside: 0.5, outside: 2.0)
        + edge!(Fraction => FiniteReal, inside: 0.5, outside: -0.5);
    assert_eq!(edges, 19);
}

#[test]
fn a_widening_edge_carries_a_signed_zero_and_a_domain_boundary_unchanged() {
    use crate::scalar::{Fraction, Length, NonNegativeLength, NonNegativeReal};

    let zero = NonNegativeLength::new(-0.0).expect("a negative zero is nonnegative");
    assert!(Length::from(zero).get().is_sign_negative());

    let one = Fraction::new(1.0).expect("one is a fraction");
    assert_eq!(NonNegativeReal::from(one).get(), 1.0);
}

#[test]
fn scalar_constants_are_the_admitted_literals() {
    use crate::scalar::{Angle, FiniteReal, NonZeroReal};

    let constants = [
        (
            Angle::QUARTER_TURN.get(),
            Angle::new(std::f64::consts::FRAC_PI_2).map(Angle::get),
        ),
        (
            Angle::HALF_TURN.get(),
            Angle::new(std::f64::consts::PI).map(Angle::get),
        ),
        (
            Angle::THREE_QUARTER_TURN.get(),
            Angle::new(3.0 * std::f64::consts::FRAC_PI_2).map(Angle::get),
        ),
        (
            FiniteReal::ZERO.get(),
            FiniteReal::new(0.0).map(FiniteReal::get),
        ),
        (
            NonZeroReal::ONE.get(),
            NonZeroReal::new(1.0).map(NonZeroReal::get),
        ),
        (
            NonZeroReal::FRAC_1_SQRT_2.get(),
            NonZeroReal::new(std::f64::consts::FRAC_1_SQRT_2).map(NonZeroReal::get),
        ),
    ];
    for (constant, admitted) in constants {
        assert_eq!(
            Some(constant.to_bits()),
            admitted.map(f64::to_bits),
            "{constant} is the admitted literal"
        );
    }
}

#[test]
fn finite_scalar_magnitudes_stay_admitted() {
    use crate::scalar::{Angle, FiniteReal, Length};

    for value in [
        -0.0,
        0.0,
        -2.5,
        3.0,
        f64::MIN,
        f64::MAX,
        -f64::MIN_POSITIVE,
        -f64::from_bits(1),
    ] {
        let magnitude = value.abs().to_bits();
        let length = Length::new(value).expect("a finite length").abs();
        let angle = Angle::new(value).expect("a finite angle").abs();
        let real = FiniteReal::new(value).expect("a finite real").abs();
        assert_eq!(length.get().to_bits(), magnitude);
        assert_eq!(angle.get().to_bits(), magnitude);
        assert_eq!(real.get().to_bits(), magnitude);
        assert_eq!(Length::new(length.get()), Some(length));
        assert_eq!(Angle::new(angle.get()), Some(angle));
        assert_eq!(FiniteReal::new(real.get()), Some(real));
    }
}

#[test]
fn an_assigned_real_keeps_its_bits_in_the_length_and_angle_families() {
    use crate::scalar::{Angle, FiniteReal, Length};

    for value in [-0.0, 0.0, -2.5, 3.0, f64::MIN, f64::MAX, -f64::from_bits(1)] {
        let real = FiniteReal::new(value).expect("a finite real");
        let length = Length::from_assigned_real(real);
        let angle = Angle::from_assigned_real(real);
        assert_eq!(length.get().to_bits(), value.to_bits());
        assert_eq!(angle.get().to_bits(), value.to_bits());
        assert_eq!(Length::new(value), Some(length));
        assert_eq!(Angle::new(value), Some(angle));
    }
}

#[test]
fn a_nonzero_length_magnitude_is_the_positive_length_it_admits() {
    use crate::scalar::{NonZeroLength, PositiveLength};

    for value in [
        -2.5,
        3.0,
        f64::MIN,
        f64::MAX,
        -f64::MIN_POSITIVE,
        -f64::from_bits(1),
        f64::from_bits(1),
    ] {
        let magnitude = NonZeroLength::new(value)
            .expect("a finite nonzero length")
            .abs();
        assert_eq!(magnitude.get().to_bits(), value.abs().to_bits());
        assert_eq!(PositiveLength::new(value.abs()), Some(magnitude));
    }
}

#[test]
fn a_length_scaled_by_a_sine_stays_admitted_and_not_above_the_length() {
    use crate::scalar::{Angle, NonNegativeLength};

    let length = NonNegativeLength::new(f64::MAX).unwrap();
    for radians in [
        0.25,
        -0.25,
        std::f64::consts::FRAC_PI_2,
        -std::f64::consts::FRAC_PI_2,
        1.0e300,
        0.0,
    ] {
        let angle = Angle::new(radians).unwrap();
        let scaled = length.scaled_by_sine(angle);
        assert_eq!(NonNegativeLength::new(scaled.get()), Some(scaled));
        assert!(scaled.get() <= length.get());
        assert_eq!(scaled.get(), f64::MAX * radians.sin().abs());
    }
    let slant = NonNegativeLength::new(2.0).unwrap();
    let half_angle = Angle::new(0.25).unwrap();
    assert_eq!(slant.scaled_by_sine(half_angle).get(), 2.0 * 0.25_f64.sin());
    assert_eq!(
        NonNegativeLength::ZERO.scaled_by_sine(half_angle),
        NonNegativeLength::ZERO
    );
}

#[test]
fn an_increasing_interval_hands_its_endpoints_on_as_finite_reals() {
    use crate::topology::IncreasingParameterInterval;

    for endpoints in [
        [-2.5, 7.0],
        [-f64::MAX, f64::MAX],
        [-0.0, f64::MIN_POSITIVE],
    ] {
        let interval = IncreasingParameterInterval::new(endpoints).unwrap();
        let [lower, upper] = interval.finite_endpoints();
        assert_eq!(
            [lower.get(), upper.get()].map(f64::to_bits),
            endpoints.map(f64::to_bits)
        );
    }
}

#[test]
fn finite_lanes_optional_bounds_and_arrays_admit_all_or_nothing() {
    use super::FiniteReal;

    let lanes = FiniteReal::lanes([vec![1.0, -2.0], vec![]]).unwrap();
    assert_eq!(FiniteReal::raw_lanes(&lanes), [vec![1.0, -2.0], vec![]]);
    assert!(FiniteReal::lanes([vec![1.0], vec![f64::NAN]]).is_none());
    assert_eq!(
        FiniteReal::optional([Some(3.0), None]),
        Some([FiniteReal::new(3.0), None])
    );
    assert!(FiniteReal::optional([None, Some(f64::INFINITY)]).is_none());
    assert_eq!(
        FiniteReal::array([4.0, 5.0]),
        Some([FiniteReal::new(4.0).unwrap(), FiniteReal::new(5.0).unwrap()])
    );
    assert!(FiniteReal::array([4.0, f64::NEG_INFINITY]).is_none());
}

#[test]
fn a_finite_point_carries_its_coordinates_into_a_finite_vector() {
    use crate::math::Point2;
    use crate::units::{FinitePoint2, FiniteVector};

    let point = FinitePoint2::new(Point2::new(-1.5, 4.0)).unwrap();
    assert_eq!(FiniteVector::from(point).get(), [-1.5, 4.0]);
}

#[test]
fn an_interval_projects_a_value_onto_its_nearest_point() {
    use crate::scalar::{ExtendedReal, FiniteReal};
    use crate::topology::ParameterInterval;

    let interval = ParameterInterval::new([-1.0, 2.0]).unwrap();
    let project = |value| interval.project(ExtendedReal::new(value).unwrap()).get();
    assert_eq!(project(0.5), 0.5);
    assert_eq!(project(-3.0), -1.0);
    assert_eq!(project(5.0), 2.0);
    assert_eq!(project(f64::INFINITY), 2.0);
    assert_eq!(project(f64::NEG_INFINITY), -1.0);
    assert!(ExtendedReal::new(f64::NAN).is_none());

    // A step past the finite range projects onto the nearer endpoint.
    let far = FiniteReal::new(-f64::MAX).unwrap();
    let step = FiniteReal::new(f64::MAX).unwrap();
    assert_eq!(
        interval
            .project(ExtendedReal::stepped(far, FiniteReal::ONE, step))
            .get(),
        -1.0
    );
    assert_eq!(
        interval
            .project(ExtendedReal::stepped(step, FiniteReal::ONE, far))
            .get(),
        2.0
    );
    assert_eq!(
        interval
            .project(ExtendedReal::stepped(
                FiniteReal::ONE,
                FiniteReal::new(0.5).unwrap(),
                FiniteReal::ONE
            ))
            .get(),
        0.5
    );
}

#[test]
fn finite_reals_halve_average_and_count_without_a_check() {
    use crate::scalar::FiniteReal;
    use crate::units::FiniteVector;

    let max = FiniteReal::new(f64::MAX).unwrap();
    assert_eq!(max.midpoint(max).get(), f64::MAX);
    assert_eq!(max.midpoint(max.negated()).get(), 0.0);
    assert_eq!(max.halved().get(), f64::MAX / 2.0);
    assert_eq!(FiniteReal::from_index(7).get(), 7.0);
    assert_eq!(
        FiniteVector::<2>::new([1.5, -2.0])
            .unwrap()
            .finite_components()
            .map(FiniteReal::get),
        [1.5, -2.0]
    );
}

#[test]
fn a_positive_length_reads_as_its_magnitude_and_half_is_one_half() {
    use crate::scalar::{FiniteReal, PositiveLength};

    assert_eq!(PositiveLength::new(2.5).unwrap().magnitude().get(), 2.5);
    assert_eq!(FiniteReal::HALF.get(), 0.5);
}

#[test]
fn a_magnification_is_a_finite_scale_of_at_least_one() {
    use crate::scalar::{Magnification, PositiveReal};

    for value in [0.5, 0.0, -2.0, f64::NAN, f64::INFINITY] {
        assert!(Magnification::new(value).is_none());
    }
    let one = Magnification::new(1.0).expect("one is a magnification");
    assert_eq!(PositiveReal::from(one).get(), 1.0);
    assert_eq!(Magnification::MILLIMETERS_PER_METER.get(), 1000.0);
    let half = PositiveReal::new(0.5).expect("a positive fixture");
    assert!(Magnification::try_from(half).is_err());
}

/// A positive scale keeps a nonnegative value nonnegative, a signed zero
/// included; only a product that overflows is refused.
#[test]
fn scaled_nonnegative_values_are_refused_only_on_overflow() {
    use crate::scalar::{NonNegativeLength, NonNegativeReal, PositiveReal};

    let tiny = PositiveReal::new(1.0e-300).expect("a positive scale");
    let huge = PositiveReal::new(f64::MAX).expect("a positive scale");
    for value in [-0.0, 0.0, 5.0e-324] {
        let length = NonNegativeLength::new(value).expect("a nonnegative fixture");
        let scaled = length
            .scaled(tiny)
            .expect("a product that does not overflow");
        assert_eq!(scaled.get().to_bits(), (value * 1.0e-300).to_bits());
        let real = NonNegativeReal::new(value).expect("a nonnegative fixture");
        assert_eq!(
            real.scaled(tiny).map(|scaled| scaled.get().to_bits()),
            Some((value * 1.0e-300).to_bits())
        );
    }
    let two = NonNegativeLength::new(2.0).expect("a nonnegative fixture");
    assert!(two.scaled(huge).is_none());
    assert!(NonNegativeReal::new(2.0)
        .expect("a nonnegative fixture")
        .scaled(huge)
        .is_none());
}

/// A factor of at least one cannot round a nonzero length to zero, the
/// smallest subnormal included; only a product that overflows is refused.
#[test]
fn a_magnified_nonzero_length_is_refused_only_on_overflow() {
    use crate::scalar::{Magnification, NonZeroLength};

    let factor = Magnification::MILLIMETERS_PER_METER;
    for value in [5.0e-324, -5.0e-324, 0.003, -0.003] {
        let length = NonZeroLength::new(value).expect("a nonzero fixture");
        let magnified = length.magnified(factor).expect("a nonzero finite product");
        assert_eq!(magnified.get(), value * 1000.0);
    }
    let one = Magnification::new(1.0).expect("one is a magnification");
    let smallest = NonZeroLength::new(5.0e-324).expect("a nonzero fixture");
    assert_eq!(smallest.magnified(one), Some(smallest));
    let large = NonZeroLength::new(-f64::MAX).expect("a nonzero fixture");
    assert!(large.magnified(factor).is_none());
}

/// The first of two ordered values is admitted positive and finite, the
/// second only positive; both can round to one value.
#[test]
fn an_ordered_pair_scales_and_refuses_the_first_failing_value() {
    use crate::scalar::{PositiveLength, PositiveReal};

    let scale = |value: f64| PositiveReal::new(value).expect("a positive scale");
    let minor = 1.9_f64;
    let major = f64::from_bits(minor.to_bits() + 1);
    let pair = PositiveLength::scale_ordered_pair([major, minor], scale(25.4))
        .expect("the pair keeps its order");
    assert_eq!(pair.map(PositiveLength::get), [48.26, 48.26]);
    assert_eq!(
        PositiveLength::scale_ordered_pair([f64::MAX, 1.0], scale(2.0)),
        Err(0)
    );
    assert_eq!(
        PositiveLength::scale_ordered_pair([1.0e-320, 1.0e-320], scale(1.0e-10)),
        Err(0)
    );
    assert_eq!(
        PositiveLength::scale_ordered_pair([1.0, 1.0e-320], scale(1.0e-10)),
        Err(1)
    );
    assert_eq!(
        PositiveLength::scale_ordered_pair([1.0, f64::NEG_INFINITY], scale(2.0)),
        Err(1)
    );
}
