use super::super::{
    CompoundLoftConstruction, CompoundLoftDirection, CompoundLoftScale, CompoundLoftScales,
    CompoundLoftTail,
};
use crate::math::Vector3;

fn scale() -> CompoundLoftScale {
    CompoundLoftScale {
        members: Vec::new(),
        path: "test:curve#0".try_into().unwrap(),
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
fn compound_loft_prefix_preserves_all_six_legal_lengths_and_wire_slots() {
    for count in 0..=5 {
        let construction = compound(count);
        let wire = serde_json::to_value(&construction).unwrap();
        let slots = wire["scales"].as_array().unwrap();
        assert_eq!(slots.len(), 4);
        assert_eq!(
            slots.iter().filter(|value| !value.is_null()).count(),
            count.min(4)
        );
        assert_eq!(wire.get("fifth_scale").is_some(), count == 5);
        assert_eq!(
            serde_json::from_value::<CompoundLoftConstruction>(wire).unwrap(),
            construction
        );
    }
    assert!(CompoundLoftScales::<5>::try_new((0..6).map(|_| scale()).collect()).is_err());
}

#[test]
fn compound_loft_wire_rejects_a_gap_before_any_later_scale() {
    for slot in 1..4 {
        let mut wire = serde_json::to_value(compound(0)).unwrap();
        wire["scales"][slot] = serde_json::to_value(scale()).unwrap();
        assert!(serde_json::from_value::<CompoundLoftConstruction>(wire).is_err());
    }
    for count in 0..4 {
        let mut wire = serde_json::to_value(compound(count)).unwrap();
        wire["fifth_scale"] = serde_json::to_value(scale()).unwrap();
        assert!(serde_json::from_value::<CompoundLoftConstruction>(wire).is_err());
    }
}

#[test]
fn scaled_loft_prefix_preserves_three_slots_and_rejects_gaps_or_excess() {
    for count in 0..=3 {
        let prefix =
            CompoundLoftScales::<3>::try_new((0..count).map(|_| scale()).collect()).unwrap();
        let wire = serde_json::to_value(&prefix).unwrap();
        assert_eq!(wire.as_array().unwrap().len(), 3);
        assert_eq!(
            serde_json::from_value::<CompoundLoftScales<3>>(wire).unwrap(),
            prefix
        );
    }
    assert!(CompoundLoftScales::<3>::try_new((0..4).map(|_| scale()).collect()).is_err());
    assert!(CompoundLoftScales::<3>::try_from_slots([None, Some(scale()), None]).is_err());
    assert!(serde_json::from_str::<CompoundLoftScales<3>>("[null,null]").is_err());
    let wire = serde_json::json!([null, scale(), null]);
    assert!(serde_json::from_value::<CompoundLoftScales<3>>(wire).is_err());
}
