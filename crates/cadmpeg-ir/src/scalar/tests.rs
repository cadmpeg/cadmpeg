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
