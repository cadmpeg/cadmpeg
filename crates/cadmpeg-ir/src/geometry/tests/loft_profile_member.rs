// SPDX-License-Identifier: Apache-2.0
//! The profile member nests its curve under `profile`, so the curve's own
//! deny is reached and the member refuses unknown keys on both levels.

use crate::geometry::{LoftMemberForm, LoftPathCurve, LoftProfileMember, LoftSubdata};
use crate::ids::CurveId;

fn member() -> LoftProfileMember {
    LoftProfileMember {
        profile: LoftPathCurve {
            id: CurveId::mint("test:model:curve#loft").expect("valid identity"),
            endpoints: Some([Some(0.0), Some(1.0)]),
        },
        form: LoftMemberForm::PcurvePair {
            pcurve: None,
            secondary_pcurve: None,
            asm_extension: None,
            subdata: LoftSubdata::type_211([1, 0], [0.0, 1.0]),
            direction: None,
        },
    }
}

#[test]
fn a_loft_profile_member_nests_its_curve_and_refuses_unknown_keys() {
    let value = member();
    let wire = serde_json::to_value(&value).expect("serializes");
    assert_eq!(wire["profile"]["curve"], "test:model:curve#loft");
    assert_eq!(wire["profile"]["endpoints"], serde_json::json!([0.0, 1.0]));
    assert!(wire.get("curve").is_none());
    assert_eq!(
        serde_json::from_value::<LoftProfileMember>(wire.clone()).expect("round trip"),
        value
    );

    let mut beside = wire.clone();
    beside["zz_bogus"] = serde_json::json!(1);
    let error = serde_json::from_value::<LoftProfileMember>(beside)
        .unwrap_err()
        .to_string();
    assert!(error.contains("zz_bogus"), "{error}");

    let mut inside = wire;
    inside["profile"]["zz_bogus"] = serde_json::json!(1);
    let error = serde_json::from_value::<LoftProfileMember>(inside)
        .unwrap_err()
        .to_string();
    assert!(error.contains("zz_bogus"), "{error}");
}

// The classic profile data carries every field it needs and no field it
// cannot use, so the revision-gated `support_bounds` and the type-zero
// `secondary_pcurve` are unknown keys rather than values a reader refuses.
#[test]
fn classic_loft_profile_data_states_only_its_own_fields() {
    use crate::geometry::ClassicLoftProfileData;

    let wire = serde_json::json!({
        "surface": "test:model:surface#loft",
        "first_flag": true,
        "asm_extension": 3,
        "subdata": {
            "form": "type211",
            "dimensions": [1, 0],
            "row": [0.0, 1.0]
        }
    });
    let data: ClassicLoftProfileData =
        serde_json::from_value(wire.clone()).expect("classic loft data reads");
    assert!(data.pcurve.is_none());
    assert!(data.direction.is_none());
    assert_eq!(serde_json::to_value(&data).expect("serializes"), wire);

    for (key, value) in [
        ("support_bounds", serde_json::json!([null, null, null, 2.0])),
        ("secondary_pcurve", serde_json::json!(null)),
        ("zz_bogus", serde_json::json!(true)),
    ] {
        let mut orphan = wire.clone();
        orphan[key] = value;
        let error = serde_json::from_value::<ClassicLoftProfileData>(orphan)
            .expect_err("the classic form states no such field")
            .to_string();
        assert!(error.contains("unknown field"), "{key}: {error}");
        assert!(error.contains(key), "{key}: {error}");
    }

    for required in ["surface", "first_flag", "asm_extension", "subdata"] {
        let mut missing = wire.clone();
        missing
            .as_object_mut()
            .expect("an object")
            .remove(required)
            .expect("the field is present");
        let error = serde_json::from_value::<ClassicLoftProfileData>(missing)
            .expect_err("the classic form requires the field")
            .to_string();
        assert!(error.contains(required), "{required}: {error}");
    }
}
