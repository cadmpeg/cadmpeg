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
