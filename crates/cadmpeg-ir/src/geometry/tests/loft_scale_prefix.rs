use super::super::{
    CompoundLoftConstruction, CompoundLoftDirection, CompoundLoftScale, CompoundLoftScales,
    CompoundLoftTail,
};
use crate::math::Vector3;

fn scale() -> CompoundLoftScale {
    CompoundLoftScale {
        members: Vec::new(),
        path: "test:model:curve#0".try_into().unwrap(),
        auxiliaries: Vec::new(),
        tail: [0, 0],
    }
}

fn compound(count: usize) -> CompoundLoftConstruction {
    CompoundLoftConstruction {
        scales: CompoundLoftScales::try_new((0..count).map(|_| scale()).collect()).unwrap(),
        flags: [false; 2],
        tail: CompoundLoftTail::Zero {
            flags: [false; 2],
            direction: CompoundLoftDirection::Vector {
                value: Vector3::new(0.0, 0.0, 0.0),
            },
            trailing_flags: [false; 2],
        },
    }
}

#[test]
fn compound_loft_scales_are_one_dense_array_of_the_present_scales() {
    for count in 0..=5 {
        let construction = compound(count);
        let wire = serde_json::to_value(&construction).unwrap();
        assert_eq!(wire["scales"].as_array().unwrap().len(), count);
        assert!(wire.get("fifth_scale").is_none());
        assert_eq!(
            serde_json::from_value::<CompoundLoftConstruction>(wire).unwrap(),
            construction
        );
    }
}

#[test]
fn compound_loft_scales_reject_more_than_the_native_slot_count() {
    assert!(CompoundLoftScales::<5>::try_new((0..6).map(|_| scale()).collect()).is_err());
    let mut wire = serde_json::to_value(compound(5)).unwrap();
    wire["scales"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::to_value(scale()).unwrap());
    assert!(serde_json::from_value::<CompoundLoftConstruction>(wire).is_err());
}

#[test]
fn compound_loft_construction_rejects_an_unknown_key_by_name() {
    let mut wire = serde_json::to_value(compound(2)).unwrap();
    wire["zz_bogus"] = serde_json::json!(1);
    let error = serde_json::from_value::<CompoundLoftConstruction>(wire)
        .expect_err("an unknown key beside scales must be rejected")
        .to_string();
    assert!(error.contains("zz_bogus"), "{error}");
}

#[test]
fn scaled_loft_scales_hold_at_most_three_and_carry_no_null_slots() {
    for count in 0..=3 {
        let prefix =
            CompoundLoftScales::<3>::try_new((0..count).map(|_| scale()).collect()).unwrap();
        let wire = serde_json::to_value(&prefix).unwrap();
        assert_eq!(wire.as_array().unwrap().len(), count);
        assert_eq!(
            serde_json::from_value::<CompoundLoftScales<3>>(wire).unwrap(),
            prefix
        );
    }
    assert!(CompoundLoftScales::<3>::try_new((0..4).map(|_| scale()).collect()).is_err());
    assert!(CompoundLoftScales::<3>::try_from_slots([None, Some(scale()), None]).is_err());
    assert!(serde_json::from_str::<CompoundLoftScales<3>>("[null]").is_err());
}
