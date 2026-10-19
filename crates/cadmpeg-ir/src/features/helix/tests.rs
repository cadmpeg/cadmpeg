// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn each_authored_pair_derives_the_same_signed_evaluation_without_a_third_field() {
    let pitch = Dimension::Literal(NonZeroLength::new(-2.0).unwrap());
    let height = Dimension::Literal(Length::new(-6.0).unwrap());
    let revolutions = Dimension::Literal(PositiveReal::new(3.0).unwrap());
    let laws = [
        HelixLaw::PitchRevolutions {
            pitch: pitch.clone(),
            revolutions: revolutions.clone(),
        },
        HelixLaw::HeightPitch {
            height: height.clone(),
            pitch,
        },
        HelixLaw::HeightRevolutions {
            height,
            revolutions,
        },
    ];
    for law in laws {
        let evaluation = law.resolved_from(&[]).unwrap();
        assert_eq!(evaluation.pitch.get(), -2.0);
        assert_eq!(evaluation.height.get(), -6.0);
        assert_eq!(evaluation.revolutions.get(), 3.0);
        let wire = serde_json::to_value(&law).unwrap();
        assert_eq!(
            wire.as_object().unwrap().len(),
            3,
            "one kind and exactly two authored controls"
        );
        assert_eq!(serde_json::from_value::<HelixLaw>(wire).unwrap(), law);
    }
}

#[test]
fn height_and_turns_preserve_a_planar_curve_with_zero_derived_pitch() {
    let law = HelixLaw::HeightRevolutions {
        height: Dimension::Literal(Length::ZERO),
        revolutions: Dimension::Literal(PositiveReal::new(1.0).unwrap()),
    };
    assert_eq!(law.resolved_from(&[]).unwrap().pitch.get(), 0.0);
    assert!(law.resolved_pitch(&crate::CadIr::empty()).is_some());
}
