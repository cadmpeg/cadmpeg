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
